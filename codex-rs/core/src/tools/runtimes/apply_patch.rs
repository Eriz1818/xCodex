//! Apply Patch runtime: executes verified patches under the orchestrator.
//!
//! Assumes `apply_patch` verification/approval happened upstream. Reuses that
//! decision to avoid re-prompting, applies through the remote filesystem when
//! the turn uses a remote environment, uses an xcodex in-process fast path for
//! unrestricted local turns, or builds the self-invocation command for
//! `codex --codex-run-as-apply-patch` and runs it under the current
//! `SandboxAttempt` with a minimal environment for sandboxed local turns.
use crate::exec::ExecCapturePolicy;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::review_approval_request;
use crate::sandboxing::ExecOptions;
use crate::sandboxing::execute_env;
use crate::tools::sandboxing::Approvable;
use crate::tools::sandboxing::ApprovalCtx;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::Sandboxable;
use crate::tools::sandboxing::ToolCtx;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::ToolRuntime;
use crate::tools::sandboxing::with_cached_approval;
use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::ApplyPatchFileUpdate;
use codex_apply_patch::CODEX_CORE_APPLY_PATCH_ARG1;
use codex_exec_server::LOCAL_FS;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::exec_output::StreamOutput;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_sandboxing::SandboxCommand;
use codex_sandboxing::SandboxType;
use codex_sandboxing::SandboxablePreference;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug)]
pub struct ApplyPatchRequest {
    pub action: ApplyPatchAction,
    pub file_paths: Vec<AbsolutePathBuf>,
    pub changes: std::collections::HashMap<PathBuf, FileChange>,
    pub exec_approval_requirement: ExecApprovalRequirement,
    pub additional_permissions: Option<PermissionProfile>,
    pub permissions_preapproved: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Default)]
pub struct ApplyPatchRuntime;

impl ApplyPatchRuntime {
    pub fn new() -> Self {
        Self
    }

    async fn apply_patch_in_process(req: &ApplyPatchRequest) -> ExecToolCallOutput {
        let start = Instant::now();
        let parsed = match codex_apply_patch::parse_patch(&req.action.patch) {
            Ok(parsed) => parsed,
            Err(err) => {
                let message = format!("apply_patch parse failed: {err}\n");
                return Self::error_output(start.elapsed(), message);
            }
        };

        if parsed.hunks.is_empty() {
            return Self::error_output(start.elapsed(), "No files were modified.\n".to_string());
        }

        let mut added: Vec<PathBuf> = Vec::new();
        let mut modified: Vec<PathBuf> = Vec::new();
        let mut deleted: Vec<PathBuf> = Vec::new();

        for hunk in parsed.hunks {
            match hunk {
                codex_apply_patch::Hunk::AddFile { path, contents } => {
                    let abs = req.action.cwd.join(&path);
                    if let Some(parent) = abs.parent()
                        && !parent.as_os_str().is_empty()
                        && let Err(err) = std::fs::create_dir_all(parent)
                    {
                        let display = path.display();
                        return Self::error_output(
                            start.elapsed(),
                            format!("Failed to create parent directories for {display}: {err}\n"),
                        );
                    }
                    if let Err(err) = std::fs::write(&abs, contents) {
                        let display = path.display();
                        return Self::error_output(
                            start.elapsed(),
                            format!("Failed to write file {display}: {err}\n"),
                        );
                    }
                    added.push(path);
                }
                codex_apply_patch::Hunk::DeleteFile { path } => {
                    let abs = req.action.cwd.join(&path);
                    if let Err(err) = std::fs::remove_file(&abs) {
                        let display = path.display();
                        return Self::error_output(
                            start.elapsed(),
                            format!("Failed to delete file {display}: {err}\n"),
                        );
                    }
                    deleted.push(path);
                }
                codex_apply_patch::Hunk::UpdateFile {
                    path,
                    move_path,
                    chunks,
                } => {
                    let abs = req.action.cwd.join(&path);
                    let update: ApplyPatchFileUpdate =
                        match codex_apply_patch::unified_diff_from_chunks(
                            &abs,
                            &chunks,
                            LOCAL_FS.as_ref(),
                            /*sandbox*/ None,
                        )
                        .await
                        {
                            Ok(update) => update,
                            Err(err) => {
                                let abs_display = abs.display().to_string();
                                let rel_display = path.display().to_string();
                                let message = err.to_string().replace(&abs_display, &rel_display);
                                return Self::error_output(start.elapsed(), format!("{message}\n"));
                            }
                        };
                    if let Some(dest) = move_path {
                        let abs_dest = req.action.cwd.join(&dest);
                        if let Some(parent) = abs_dest.parent()
                            && !parent.as_os_str().is_empty()
                            && let Err(err) = std::fs::create_dir_all(parent)
                        {
                            let display = dest.display();
                            return Self::error_output(
                                start.elapsed(),
                                format!(
                                    "Failed to create parent directories for {display}: {err}\n"
                                ),
                            );
                        }
                        if let Err(err) = std::fs::write(&abs_dest, update.content()) {
                            let display = dest.display();
                            return Self::error_output(
                                start.elapsed(),
                                format!("Failed to write file {display}: {err}\n"),
                            );
                        }
                        if let Err(err) = std::fs::remove_file(&abs) {
                            let display = path.display();
                            return Self::error_output(
                                start.elapsed(),
                                format!("Failed to remove original {display}: {err}\n"),
                            );
                        }
                        modified.push(dest);
                    } else {
                        if let Err(err) = std::fs::write(&abs, update.content()) {
                            let display = path.display();
                            return Self::error_output(
                                start.elapsed(),
                                format!("Failed to write file {display}: {err}\n"),
                            );
                        }
                        modified.push(path);
                    }
                }
            }
        }

        let mut stdout = String::new();
        stdout.push_str("Success. Updated the following files:\n");
        for path in &added {
            stdout.push_str(&format!("A {}\n", path.display()));
        }
        for path in &modified {
            stdout.push_str(&format!("M {}\n", path.display()));
        }
        for path in &deleted {
            stdout.push_str(&format!("D {}\n", path.display()));
        }

        ExecToolCallOutput {
            exit_code: 0,
            stdout: StreamOutput::new(stdout.clone()),
            stderr: StreamOutput::new(String::new()),
            aggregated_output: StreamOutput::new(stdout),
            duration: start.elapsed(),
            timed_out: false,
        }
    }

    fn error_output(duration: std::time::Duration, stderr: String) -> ExecToolCallOutput {
        ExecToolCallOutput {
            exit_code: 1,
            stdout: StreamOutput::new(String::new()),
            stderr: StreamOutput::new(stderr.clone()),
            aggregated_output: StreamOutput::new(stderr),
            duration,
            timed_out: false,
        }
    }

    fn build_guardian_review_request(
        req: &ApplyPatchRequest,
        call_id: &str,
    ) -> GuardianApprovalRequest {
        GuardianApprovalRequest::ApplyPatch {
            id: call_id.to_string(),
            cwd: req.action.cwd.to_path_buf(),
            files: req.file_paths.clone(),
            patch: req.action.patch.clone(),
        }
    }

    #[cfg(target_os = "windows")]
    fn build_sandbox_command(
        req: &ApplyPatchRequest,
        codex_home: &std::path::Path,
    ) -> Result<SandboxCommand, ToolError> {
        Ok(Self::build_sandbox_command_with_program(
            req,
            codex_windows_sandbox::resolve_current_exe_for_launch(codex_home, "codex.exe"),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    fn build_sandbox_command(
        req: &ApplyPatchRequest,
        codex_self_exe: Option<&PathBuf>,
    ) -> Result<SandboxCommand, ToolError> {
        let exe = Self::resolve_apply_patch_program(codex_self_exe)?;
        Ok(Self::build_sandbox_command_with_program(req, exe))
    }

    #[cfg(not(target_os = "windows"))]
    fn resolve_apply_patch_program(codex_self_exe: Option<&PathBuf>) -> Result<PathBuf, ToolError> {
        if let Some(path) = codex_self_exe {
            return Ok(path.clone());
        }

        std::env::current_exe()
            .map_err(|e| ToolError::Rejected(format!("failed to determine codex exe: {e}")))
    }

    fn build_sandbox_command_with_program(req: &ApplyPatchRequest, exe: PathBuf) -> SandboxCommand {
        SandboxCommand {
            program: exe.into_os_string(),
            args: vec![
                CODEX_CORE_APPLY_PATCH_ARG1.to_string(),
                req.action.patch.clone(),
            ],
            cwd: req.action.cwd.clone(),
            // Run apply_patch with a minimal environment for determinism and to avoid leaks.
            env: HashMap::new(),
            additional_permissions: req.additional_permissions.clone(),
        }
    }

    fn stdout_stream(ctx: &ToolCtx) -> Option<crate::exec::StdoutStream> {
        Some(crate::exec::StdoutStream {
            sub_id: ctx.turn.sub_id.clone(),
            call_id: ctx.call_id.clone(),
            tx_event: ctx.session.get_tx_event(),
        })
    }
}

impl Sandboxable for ApplyPatchRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
    fn escalate_on_failure(&self) -> bool {
        true
    }
}

impl Approvable<ApplyPatchRequest> for ApplyPatchRuntime {
    type ApprovalKey = ApprovalKey;

    fn approval_keys(&self, req: &ApplyPatchRequest) -> Vec<Self::ApprovalKey> {
        req.file_paths
            .iter()
            .map(|path| ApprovalKey(path.to_string_lossy().into_owned()))
            .collect()
    }

    fn start_approval_async<'a>(
        &'a mut self,
        req: &'a ApplyPatchRequest,
        ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision> {
        let session = ctx.session;
        let turn = ctx.turn;
        let call_id = ctx.call_id.to_string();
        let retry_reason = ctx.retry_reason.clone();
        let approval_keys = self.approval_keys(req);
        let changes = req.changes.clone();
        let guardian_review_id = ctx.guardian_review_id.clone();
        Box::pin(async move {
            if req.permissions_preapproved && retry_reason.is_none() {
                return ReviewDecision::Approved;
            }
            if let Some(review_id) = guardian_review_id {
                let action = ApplyPatchRuntime::build_guardian_review_request(req, ctx.call_id);
                return review_approval_request(session, turn, review_id, action, retry_reason)
                    .await;
            }
            if let Some(reason) = retry_reason {
                let rx_approve = session
                    .request_patch_approval(
                        turn,
                        call_id,
                        changes.clone(),
                        Some(reason),
                        /*grant_root*/ None,
                    )
                    .await;
                return rx_approve.await.unwrap_or_default();
            }

            with_cached_approval(
                &session.services,
                "apply_patch",
                approval_keys,
                || async move {
                    let rx_approve = session
                        .request_patch_approval(
                            turn, call_id, changes, /*reason*/ None, /*grant_root*/ None,
                        )
                        .await;
                    rx_approve.await.unwrap_or_default()
                },
            )
            .await
        })
    }

    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::Never => false,
            AskForApproval::Granular(granular_config) => granular_config.allows_sandbox_approval(),
            AskForApproval::OnFailure => true,
            AskForApproval::OnRequest => true,
            AskForApproval::UnlessTrusted => true,
        }
    }

    // apply_patch approvals are decided upstream by assess_patch_safety.
    //
    // This override ensures the orchestrator runs the patch approval flow when required instead
    // of falling back to the global exec approval policy.
    fn exec_approval_requirement(
        &self,
        req: &ApplyPatchRequest,
    ) -> Option<ExecApprovalRequirement> {
        Some(req.exec_approval_requirement.clone())
    }
}

#[derive(serde::Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ApprovalKey(String);

impl ToolRuntime<ApplyPatchRequest, ExecToolCallOutput> for ApplyPatchRuntime {
    async fn run(
        &mut self,
        req: &ApplyPatchRequest,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx,
    ) -> Result<ExecToolCallOutput, ToolError> {
        if let Some(environment) = ctx.turn.environment.as_ref().filter(|env| env.is_remote()) {
            let started_at = Instant::now();
            let fs = environment.get_filesystem();
            let sandbox = ctx
                .turn
                .file_system_sandbox_context(req.additional_permissions.clone());
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let result = codex_apply_patch::apply_patch(
                &req.action.patch,
                &req.action.cwd,
                &mut stdout,
                &mut stderr,
                fs.as_ref(),
                Some(&sandbox),
            )
            .await;
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            let exit_code = if result.is_ok() { 0 } else { 1 };
            return Ok(ExecToolCallOutput {
                exit_code,
                stdout: StreamOutput::new(stdout.clone()),
                stderr: StreamOutput::new(stderr.clone()),
                aggregated_output: StreamOutput::new(format!("{stdout}{stderr}")),
                duration: started_at.elapsed(),
                timed_out: false,
            });
        }

        if attempt.sandbox == SandboxType::None
            && matches!(attempt.policy, SandboxPolicy::DangerFullAccess)
        {
            return Ok(Self::apply_patch_in_process(req).await);
        }

        #[cfg(target_os = "windows")]
        let command = Self::build_sandbox_command(req, &ctx.turn.config.codex_home)?;
        #[cfg(not(target_os = "windows"))]
        let command = Self::build_sandbox_command(req, ctx.turn.codex_self_exe.as_ref())?;
        let options = ExecOptions {
            expiration: req.timeout_ms.into(),
            capture_policy: ExecCapturePolicy::ShellTool,
        };
        let env = attempt
            .env_for(command, options, /*network*/ None)
            .map_err(|err| ToolError::Codex(err.into()))?;
        let out = execute_env(env, Self::stdout_stream(ctx))
            .await
            .map_err(ToolError::Codex)?;
        Ok(out)
    }
}

#[cfg(test)]
#[path = "apply_patch_tests.rs"]
mod tests;
