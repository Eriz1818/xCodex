use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::function_tool::FunctionCallError;
use crate::hook_runtime::record_additional_contexts;
use crate::hook_runtime::run_post_tool_use_hooks;
use crate::hook_runtime::run_pre_tool_use_hooks;
use crate::memories::usage::emit_metric_for_tool_read;
use crate::protocol::EventMsg;
use crate::protocol::ReviewDecision;
use crate::protocol::WarningEvent;
use crate::sandbox_tags::sandbox_tag;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::McpToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::ToolProvenance;
use codex_hooks::HookEvent;
use codex_hooks::HookEventAfterToolUse;
use codex_hooks::HookPayload;
use codex_hooks::HookResult;
use codex_hooks::HookToolInput;
use codex_hooks::HookToolInputLocalShell;
use codex_hooks::HookToolKind;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_tools::ConfiguredToolSpec;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_utils_readiness::Readiness;
use futures::future::BoxFuture;
use serde_json::Value;
use sha2::Digest as _;
use sha2::Sha256;
use tracing::warn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Function,
    Mcp,
}

pub trait ToolHandler: Send + Sync {
    type Output: ToolOutput + 'static;

    fn kind(&self) -> ToolKind;

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            (self.kind(), payload),
            (ToolKind::Function, ToolPayload::Function { .. })
                | (ToolKind::Function, ToolPayload::ToolSearch { .. })
                | (ToolKind::Mcp, ToolPayload::Mcp { .. })
        )
    }

    /// Returns `true` if the [ToolInvocation] *might* mutate the environment of the
    /// user (through file system, OS operations, ...).
    /// This function must remains defensive and return `true` if a doubt exist on the
    /// exact effect of a ToolInvocation.
    fn is_mutating(
        &self,
        _invocation: &ToolInvocation,
    ) -> impl std::future::Future<Output = bool> + Send {
        async { false }
    }

    fn pre_tool_use_payload(&self, _invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        None
    }

    fn post_tool_use_payload(
        &self,
        _call_id: &str,
        _payload: &ToolPayload,
        _result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload> {
        None
    }

    /// Perform the actual [ToolInvocation] and returns a [ToolOutput] containing
    /// the final output to return to the model.
    fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> impl std::future::Future<Output = Result<Self::Output, FunctionCallError>> + Send;
}

pub(crate) struct AnyToolResult {
    pub(crate) call_id: String,
    pub(crate) payload: ToolPayload,
    pub(crate) result: Box<dyn ToolOutput>,
}

impl AnyToolResult {
    pub(crate) fn into_response(self) -> ResponseInputItem {
        let Self {
            call_id,
            payload,
            result,
        } = self;
        result.to_response_item(&call_id, &payload)
    }

    pub(crate) fn code_mode_result(self) -> serde_json::Value {
        let Self {
            payload, result, ..
        } = self;
        result.code_mode_result(&payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreToolUsePayload {
    pub(crate) command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PostToolUsePayload {
    pub(crate) command: String,
    pub(crate) tool_response: Value,
}

trait AnyToolHandler: Send + Sync {
    fn matches_kind(&self, payload: &ToolPayload) -> bool;

    fn is_mutating<'a>(&'a self, invocation: &'a ToolInvocation) -> BoxFuture<'a, bool>;

    fn pre_tool_use_payload(&self, invocation: &ToolInvocation) -> Option<PreToolUsePayload>;

    fn post_tool_use_payload(
        &self,
        call_id: &str,
        payload: &ToolPayload,
        result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload>;

    fn handle_any<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> BoxFuture<'a, Result<AnyToolResult, FunctionCallError>>;
}

impl<T> AnyToolHandler for T
where
    T: ToolHandler,
{
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        ToolHandler::matches_kind(self, payload)
    }

    fn is_mutating<'a>(&'a self, invocation: &'a ToolInvocation) -> BoxFuture<'a, bool> {
        Box::pin(ToolHandler::is_mutating(self, invocation))
    }

    fn pre_tool_use_payload(&self, invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        ToolHandler::pre_tool_use_payload(self, invocation)
    }

    fn post_tool_use_payload(
        &self,
        call_id: &str,
        payload: &ToolPayload,
        result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload> {
        ToolHandler::post_tool_use_payload(self, call_id, payload, result)
    }

    fn handle_any<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> BoxFuture<'a, Result<AnyToolResult, FunctionCallError>> {
        Box::pin(async move {
            let call_id = invocation.call_id.clone();
            let payload = invocation.payload.clone();
            let output = self.handle(invocation).await?;
            Ok(AnyToolResult {
                call_id,
                payload,
                result: Box::new(output),
            })
        })
    }
}

pub struct ToolRegistry {
    handlers: HashMap<ToolName, Arc<dyn AnyToolHandler>>,
}

impl ToolRegistry {
    fn new(handlers: HashMap<ToolName, Arc<dyn AnyToolHandler>>) -> Self {
        Self { handlers }
    }

    fn handler(&self, name: &ToolName) -> Option<Arc<dyn AnyToolHandler>> {
        self.handlers.get(name).map(Arc::clone)
    }

    #[cfg(test)]
    pub(crate) fn has_handler(&self, name: &ToolName) -> bool {
        self.handler(name).is_some()
    }

    pub(crate) async fn dispatch_any(
        &self,
        invocation: ToolInvocation,
    ) -> Result<AnyToolResult, FunctionCallError> {
        let tool_name = invocation.tool_name.clone();
        let display_name = tool_name.display();
        let call_id_owned = invocation.call_id.clone();
        let session = Arc::clone(&invocation.session);
        let turn = Arc::clone(&invocation.turn);
        let otel = invocation.turn.session_telemetry.clone();
        let log_payload = invocation.payload.log_payload();
        let metric_tags = [
            (
                "sandbox",
                sandbox_tag(
                    &invocation.turn.sandbox_policy,
                    invocation.turn.windows_sandbox_level,
                ),
            ),
            (
                "sandbox_policy",
                sandbox_policy_tag(&invocation.turn.sandbox_policy),
            ),
        ];
        let (mcp_server, mcp_server_origin) = match &invocation.payload {
            ToolPayload::Mcp { server, .. } => {
                let manager = invocation
                    .session
                    .services
                    .mcp_connection_manager
                    .read()
                    .await;
                let origin = manager.server_origin(server).map(str::to_owned);
                (Some(server.clone()), origin)
            }
            _ => (None, None),
        };
        let mcp_server_ref = mcp_server.as_deref();
        let mcp_server_origin_ref = mcp_server_origin.as_deref();

        if let Some(message) =
            plan_mode_tool_block_message(invocation.turn.collaboration_mode.mode, &display_name)
        {
            otel.tool_result_with_tags(
                &display_name,
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                /*success*/ false,
                &message,
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
            );
            return Err(FunctionCallError::RespondToModel(message));
        }

        {
            let mut active = invocation.session.active_turn.lock().await;
            if let Some(active_turn) = active.as_mut() {
                let mut turn_state = active_turn.turn_state.lock().await;
                turn_state.tool_calls = turn_state.tool_calls.saturating_add(1);
            }
        }

        let handler = match self.handler(&tool_name) {
            Some(handler) => handler,
            None => {
                let message = unsupported_tool_call_message(&invocation.payload, &tool_name);
                otel.tool_result_with_tags(
                    &display_name,
                    &call_id_owned,
                    log_payload.as_ref(),
                    Duration::ZERO,
                    /*success*/ false,
                    &message,
                    &metric_tags,
                    mcp_server_ref,
                    mcp_server_origin_ref,
                );
                return Err(FunctionCallError::RespondToModel(message));
            }
        };

        if !handler.matches_kind(&invocation.payload) {
            let message = format!("tool {display_name} invoked with incompatible payload");
            otel.tool_result_with_tags(
                &display_name,
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                /*success*/ false,
                &message,
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
            );
            return Err(FunctionCallError::Fatal(message));
        }

        if let Some(pre_tool_use_payload) = handler.pre_tool_use_payload(&invocation)
            && let Some(reason) = run_pre_tool_use_hooks(
                &invocation.session,
                &invocation.turn,
                invocation.call_id.clone(),
                pre_tool_use_payload.command.clone(),
            )
            .await
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "Command blocked by PreToolUse hook: {reason}. Command: {}",
                pre_tool_use_payload.command
            )));
        }

        let is_mutating = handler.is_mutating(&invocation).await;
        let response_cell = tokio::sync::Mutex::new(None);
        let invocation_for_tool = invocation.clone();

        let started = Instant::now();
        let result = otel
            .log_tool_result_with_tags(
                &display_name,
                &call_id_owned,
                log_payload.as_ref(),
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
                || {
                    let handler = handler.clone();
                    let response_cell = &response_cell;
                    async move {
                        if is_mutating {
                            tracing::trace!("waiting for tool gate");
                            invocation_for_tool.turn.tool_call_gate.wait_ready().await;
                            tracing::trace!("tool gate released");
                        }
                        match handler.handle_any(invocation_for_tool).await {
                            Ok(result) => {
                                let preview = result.result.log_preview();
                                let success = result.result.success_for_logging();
                                let mut guard = response_cell.lock().await;
                                *guard = Some(result);
                                Ok((preview, success))
                            }
                            Err(err) => Err(err),
                        }
                    }
                },
            )
            .await;
        let duration = started.elapsed();
        let (output_preview, success) = match &result {
            Ok((preview, success)) => (preview.clone(), *success),
            Err(err) => (err.to_string(), false),
        };
        emit_metric_for_tool_read(&invocation, success).await;
        let post_tool_use_payload = if success {
            let guard = response_cell.lock().await;
            guard.as_ref().and_then(|result| {
                handler.post_tool_use_payload(
                    &result.call_id,
                    &result.payload,
                    result.result.as_ref(),
                )
            })
        } else {
            None
        };
        let post_tool_use_outcome = if let Some(post_tool_use_payload) = post_tool_use_payload {
            Some(
                run_post_tool_use_hooks(
                    &invocation.session,
                    &invocation.turn,
                    invocation.call_id.clone(),
                    post_tool_use_payload.command,
                    post_tool_use_payload.tool_response,
                )
                .await,
            )
        } else {
            None
        };
        // Deprecated: this is the legacy AfterToolUse hook. Prefer the new PostToolUse
        let hook_abort_error = dispatch_after_tool_use_hook(AfterToolUseHookDispatch {
            invocation: &invocation,
            output_preview,
            success,
            executed: true,
            duration,
            mutating: is_mutating,
        })
        .await;

        if let Some(err) = hook_abort_error {
            return Err(err);
        }

        if let Some(outcome) = &post_tool_use_outcome {
            record_additional_contexts(
                &invocation.session,
                &invocation.turn,
                outcome.additional_contexts.clone(),
            )
            .await;

            let replacement_text = if outcome.should_stop {
                Some(
                    outcome
                        .feedback_message
                        .clone()
                        .or_else(|| outcome.stop_reason.clone())
                        .unwrap_or_else(|| "PostToolUse hook stopped execution".to_string()),
                )
            } else {
                outcome.feedback_message.clone()
            };
            if let Some(replacement_text) = replacement_text {
                let mut guard = response_cell.lock().await;
                if let Some(result) = guard.as_mut() {
                    result.result = Box::new(FunctionToolOutput::from_text(replacement_text, None));
                }
            }
        }

        match result {
            Ok(_) => {
                let mut guard = response_cell.lock().await;
                let mut result = guard.take().ok_or_else(|| {
                    FunctionCallError::Fatal("tool produced no output".to_string())
                })?;
                result.result = enforce_sensitive_send_policy(
                    result.result,
                    session.as_ref(),
                    turn.as_ref(),
                    &display_name,
                    &call_id_owned,
                )
                .await;
                Ok(result)
            }
            Err(err) => Err(err),
        }
    }
}

async fn enforce_sensitive_send_policy(
    mut output: Box<dyn ToolOutput>,
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    tool_name: &str,
    call_id: &str,
) -> Box<dyn ToolOutput> {
    if turn.exclusion.layer_send_firewall_enabled()
        && let Some(ToolProvenance::Filesystem { path }) = output.provenance().cloned()
        && turn.sensitive_paths.decision_send(&path)
            == crate::sensitive_paths::SensitivePathDecision::Deny
    {
        let allow = turn.exclusion.prompt_on_blocked
            && maybe_prompt_for_send(session, turn, call_id, &path).await;
        if !allow {
            let mut counters = turn
                .exclusion_counters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            counters.record(
                crate::exclusion_counters::ExclusionLayer::Layer3SendFirewall,
                crate::exclusion_counters::ExclusionSource::Filesystem,
                tool_name,
                /*redacted*/ false,
                /*blocked*/ true,
            );
            output = Box::new(
                FunctionToolOutput::from_text(
                    turn.sensitive_paths.format_denied_message(),
                    Some(false),
                )
                .with_provenance(ToolProvenance::Filesystem { path }),
            );
        }
    }

    let output = if turn.exclusion.layer_output_sanitization_enabled() {
        enforce_sensitive_content_gateway(output, session, turn, tool_name, call_id).await
    } else {
        output
    };

    if !is_unattested_output(output.as_ref()) {
        return output;
    }

    enforce_unattested_output_policy(
        output,
        turn.unattested_output_policy,
        tool_name,
        call_id,
        |message| async {
            session
                .send_event(turn, EventMsg::Warning(WarningEvent { message }))
                .await;
        },
        |command| async {
            session
                .request_command_approval(
                    turn,
                    call_id.to_string(),
                    None,
                    command,
                    turn.cwd.clone().to_path_buf(),
                    Some("unattested MCP output would be sent to the model".to_string()),
                    None,
                    None,
                    None,
                    None,
                )
                .await
        },
    )
    .await
}

async fn maybe_prompt_for_send(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    path: &std::path::Path,
) -> bool {
    let display = path.display().to_string();
    let question = RequestUserInputQuestion {
        header: "Exclusions".to_string(),
        id: "exclusions_send".to_string(),
        question: format!("Allow xcodex to send this excluded output?\n{display}"),
        is_other: false,
        is_secret: false,
        options: Some(vec![
            RequestUserInputQuestionOption {
                label: "Allow once".to_string(),
                description: "Permit this output for the current request.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Block".to_string(),
                description: "Keep exclusions blocking this output.".to_string(),
            },
        ]),
    };
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = session
        .request_user_input(turn, call_id.to_string(), args)
        .await;
    response
        .and_then(|response| response.answers.get("exclusions_send").cloned())
        .and_then(|answer| answer.answers.first().cloned())
        .is_some_and(|value| value == "Allow once")
}

enum RedactionDecision {
    AllowOnce,
    AllowForSession,
    Redact,
    Block,
    AddAllowlistLiteral(String),
    AddAllowlistRegex(String),
    AddBlocklist(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedactionPromptAnswer {
    AllowOnce,
    AllowForSession,
    Redact,
    Block,
    AddToAllowlist,
    AddToBlocklist,
    RevealMatches,
    HideMatches,
}

fn parse_redaction_prompt_answer(answer: &str) -> Option<RedactionPromptAnswer> {
    match answer {
        "Allow once" => Some(RedactionPromptAnswer::AllowOnce),
        "Allow for this session" => Some(RedactionPromptAnswer::AllowForSession),
        "Redact" => Some(RedactionPromptAnswer::Redact),
        "Block" => Some(RedactionPromptAnswer::Block),
        "Add to allowlist" => Some(RedactionPromptAnswer::AddToAllowlist),
        "Add to blocklist" => Some(RedactionPromptAnswer::AddToBlocklist),
        "Reveal matched values" => Some(RedactionPromptAnswer::RevealMatches),
        "Hide matched values" => Some(RedactionPromptAnswer::HideMatches),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RedactionMatchSummary {
    reason: crate::content_gateway::RedactionReason,
    value: String,
    count: usize,
}

fn redaction_reason_label(reason: crate::content_gateway::RedactionReason) -> &'static str {
    match reason {
        crate::content_gateway::RedactionReason::FingerprintCache => "Fingerprint cache",
        crate::content_gateway::RedactionReason::IgnoredPath => "Ignored path",
        crate::content_gateway::RedactionReason::SecretPattern => "Secret pattern",
    }
}

fn short_sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        use std::fmt::Write as _;
        write!(out, "{byte:02x}").ok();
    }
    out
}

fn truncate_match_value(mut value: String) -> String {
    if value.len() <= 200 {
        return value;
    }
    let mut boundary = 200.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    value.push_str("...");
    value
}

fn redaction_match_value_display(
    summary: &RedactionMatchSummary,
    reveal_secret_matches: bool,
) -> String {
    match summary.reason {
        crate::content_gateway::RedactionReason::SecretPattern => {
            let hash = short_sha256_hex(&summary.value);
            if reveal_secret_matches {
                return format!("{} (sha256:{hash})", summary.value);
            }
            let char_len = summary.value.chars().count();
            if char_len <= 8 {
                format!("[REDACTED sha256:{hash}]")
            } else {
                let prefix = summary
                    .value
                    .char_indices()
                    .nth(4)
                    .map(|(idx, _)| &summary.value[..idx])
                    .unwrap_or(summary.value.as_str());
                let suffix_start_chars = char_len.saturating_sub(4);
                let suffix_idx = summary
                    .value
                    .char_indices()
                    .nth(suffix_start_chars)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);
                let suffix = &summary.value[suffix_idx..];
                format!("[REDACTED {prefix}...{suffix} sha256:{hash}]")
            }
        }
        _ => truncate_match_value(summary.value.clone()),
    }
}

fn redaction_match_label(summary: &RedactionMatchSummary, reveal_secret_matches: bool) -> String {
    let reason = redaction_reason_label(summary.reason);
    let value = redaction_match_value_display(summary, reveal_secret_matches);
    let mut label = format!("{value} (reason: {reason})");
    if summary.count > 1 {
        label.push_str(&format!(" x{}", summary.count));
    }
    label
}

fn summarize_redaction_matches(
    report: &crate::content_gateway::ScanReport,
) -> Vec<RedactionMatchSummary> {
    let mut out: Vec<RedactionMatchSummary> = Vec::new();
    let mut seen: HashMap<(crate::content_gateway::RedactionReason, &str), usize> = HashMap::new();

    for match_info in &report.matches {
        let key = (match_info.reason, match_info.value.as_str());
        if let Some(idx) = seen.get(&key) {
            out[*idx].count = out[*idx].count.saturating_add(1);
        } else {
            seen.insert(key, out.len());
            out.push(RedactionMatchSummary {
                reason: match_info.reason,
                value: match_info.value.clone(),
                count: 1,
            });
        }
    }

    out
}

fn format_redaction_matches(
    report: &crate::content_gateway::ScanReport,
    layer_label: &str,
    reveal_secret_matches: bool,
) -> Option<String> {
    if report.matches.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push(format!("Matched content ({layer_label}):"));
    for match_info in summarize_redaction_matches(report) {
        lines.push(format!(
            "- {}",
            redaction_match_label(&match_info, reveal_secret_matches)
        ));
    }
    Some(lines.join("\n"))
}

async fn prompt_for_redaction_match_selection(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    prompt: &str,
    question_id: &str,
    options: Vec<RequestUserInputQuestionOption>,
) -> Option<String> {
    let question = RequestUserInputQuestion {
        header: "Exclusions".to_string(),
        id: question_id.to_string(),
        question: prompt.to_string(),
        is_other: false,
        is_secret: false,
        options: Some(options),
    };
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = session
        .request_user_input(turn, call_id.to_string(), args)
        .await;
    response
        .and_then(|response| response.answers.get(question_id).cloned())
        .and_then(|answer| answer.answers.first().cloned())
}

async fn maybe_prompt_for_redaction(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    context_label: &str,
    report: &crate::content_gateway::ScanReport,
) -> Option<RedactionDecision> {
    if !turn.exclusion.prompt_on_blocked
        || (!report.redacted && !report.blocked && report.matches.is_empty())
    {
        return None;
    }

    let match_summaries = summarize_redaction_matches(report);
    let has_secret_matches = match_summaries.iter().any(|summary| {
        matches!(
            summary.reason,
            crate::content_gateway::RedactionReason::SecretPattern
        )
    });

    let mut reveal_secret_matches =
        has_secret_matches && turn.exclusion.prompt_reveal_secret_matches;
    let answer = loop {
        let mut question_text =
            format!("Exclusions matched content in {context_label}. How should xcodex proceed?");
        if reveal_secret_matches {
            question_text.push_str("\n(Showing full matched values.)");
        }
        if let Some(summary) =
            format_redaction_matches(report, "L2-output_sanitization", reveal_secret_matches)
        {
            question_text.push('\n');
            question_text.push_str(&summary);
        }

        let mut options = vec![
            RequestUserInputQuestionOption {
                label: "Allow once".to_string(),
                description: "Permit this content for the current request.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Allow for this session".to_string(),
                description: "Permit this exact content for this xcodex session.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Redact".to_string(),
                description: "Redact matching content.".to_string(),
            },
            RequestUserInputQuestionOption {
                label: "Block".to_string(),
                description: "Block matching content.".to_string(),
            },
        ];

        if has_secret_matches {
            options.push(RequestUserInputQuestionOption {
                label: if reveal_secret_matches {
                    "Hide matched values".to_string()
                } else {
                    "Reveal matched values".to_string()
                },
                description: if reveal_secret_matches {
                    "Return to redacted previews for secret matches.".to_string()
                } else {
                    "Show the full matched values in this prompt (may display secrets).".to_string()
                },
            });
        }

        if match_summaries.iter().any(|summary| {
            matches!(
                summary.reason,
                crate::content_gateway::RedactionReason::SecretPattern
                    | crate::content_gateway::RedactionReason::IgnoredPath
            )
        }) {
            options.push(RequestUserInputQuestionOption {
                label: "Add to allowlist".to_string(),
                description: "Allow this matched value through exclusions going forward."
                    .to_string(),
            });
        }

        if has_secret_matches {
            options.push(RequestUserInputQuestionOption {
                label: "Add to blocklist".to_string(),
                description: "Add this value to extra secret patterns to scan.".to_string(),
            });
        }

        let question = RequestUserInputQuestion {
            header: "Exclusions".to_string(),
            id: "exclusions_redaction".to_string(),
            question: question_text,
            is_other: false,
            is_secret: false,
            options: Some(options),
        };
        let args = RequestUserInputArgs {
            questions: vec![question],
        };
        let response = session
            .request_user_input(turn, call_id.to_string(), args)
            .await;
        let answer = response
            .and_then(|response| response.answers.get("exclusions_redaction").cloned())
            .and_then(|answer| answer.answers.first().cloned())?;

        let Some(answer) = parse_redaction_prompt_answer(&answer) else {
            return None;
        };

        match answer {
            RedactionPromptAnswer::RevealMatches => {
                reveal_secret_matches = true;
                continue;
            }
            RedactionPromptAnswer::HideMatches => {
                reveal_secret_matches = false;
                continue;
            }
            other => break other,
        }
    };

    match answer {
        RedactionPromptAnswer::AllowOnce => Some(RedactionDecision::AllowOnce),
        RedactionPromptAnswer::AllowForSession => Some(RedactionDecision::AllowForSession),
        RedactionPromptAnswer::Redact => Some(RedactionDecision::Redact),
        RedactionPromptAnswer::Block => Some(RedactionDecision::Block),
        RedactionPromptAnswer::AddToAllowlist => {
            let candidates: Vec<RedactionMatchSummary> = match_summaries
                .iter()
                .filter(|summary| {
                    matches!(
                        summary.reason,
                        crate::content_gateway::RedactionReason::SecretPattern
                            | crate::content_gateway::RedactionReason::IgnoredPath
                    )
                })
                .cloned()
                .collect();
            let selected = if candidates.len() == 1 {
                candidates.first().cloned()
            } else {
                let prompt = "Select a matched value to add to the allowlist.";
                let options = candidates
                    .iter()
                    .map(|summary| RequestUserInputQuestionOption {
                        label: redaction_match_label(summary, reveal_secret_matches),
                        description: String::new(),
                    })
                    .collect::<Vec<_>>();

                let answer = prompt_for_redaction_match_selection(
                    session,
                    turn,
                    call_id,
                    prompt,
                    "exclusions_allowlist_match",
                    options,
                )
                .await?;

                candidates
                    .into_iter()
                    .find(|summary| redaction_match_label(summary, reveal_secret_matches) == answer)
            }?;

            match selected.reason {
                crate::content_gateway::RedactionReason::IgnoredPath => {
                    Some(RedactionDecision::AddAllowlistLiteral(selected.value))
                }
                _ => Some(RedactionDecision::AddAllowlistRegex(selected.value)),
            }
        }
        RedactionPromptAnswer::AddToBlocklist => {
            let candidates: Vec<RedactionMatchSummary> = match_summaries
                .iter()
                .filter(|summary| {
                    matches!(
                        summary.reason,
                        crate::content_gateway::RedactionReason::SecretPattern
                    )
                })
                .cloned()
                .collect();
            let selected = if candidates.len() == 1 {
                candidates.first().cloned()
            } else {
                let prompt = "Select a matched value to add to the blocklist.";
                let options = candidates
                    .iter()
                    .map(|summary| RequestUserInputQuestionOption {
                        label: redaction_match_label(summary, reveal_secret_matches),
                        description: String::new(),
                    })
                    .collect::<Vec<_>>();

                let answer = prompt_for_redaction_match_selection(
                    session,
                    turn,
                    call_id,
                    prompt,
                    "exclusions_blocklist_match",
                    options,
                )
                .await?;

                candidates
                    .into_iter()
                    .find(|summary| redaction_match_label(summary, reveal_secret_matches) == answer)
            }?;

            Some(RedactionDecision::AddBlocklist(selected.value))
        }
        RedactionPromptAnswer::RevealMatches | RedactionPromptAnswer::HideMatches => None,
    }
}

async fn resolve_redaction_decision(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    call_id: &str,
    context_label: &str,
    original: String,
    sanitized: String,
    mut report: crate::content_gateway::ScanReport,
) -> (String, crate::content_gateway::ScanReport) {
    let Some(decision) =
        maybe_prompt_for_redaction(session, turn, call_id, context_label, &report).await
    else {
        return (sanitized, report);
    };

    match decision {
        RedactionDecision::AllowOnce => (original, crate::content_gateway::ScanReport::safe()),
        RedactionDecision::AllowForSession => {
            crate::content_gateway::remember_safe_report_matches_for_epoch(
                &session.content_gateway_cache,
                &report,
                turn.sensitive_paths.ignore_epoch(),
            );
            session
                .content_gateway_cache
                .remember_safe_text_for_epoch(&original, turn.sensitive_paths.ignore_epoch());
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::Redact => {
            if report.redacted || report.blocked || report.matches.is_empty() {
                return (sanitized, report);
            }

            let mut redact_cfg =
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion);
            redact_cfg.on_match = crate::config::types::ExclusionOnMatch::Redact;
            let redact_gateway = crate::content_gateway::ContentGateway::new(redact_cfg);
            let redact_cache = crate::content_gateway::GatewayCache::new();
            let epoch = turn.sensitive_paths.ignore_epoch();

            redact_gateway.scan_text(&original, &turn.sensitive_paths, &redact_cache, epoch)
        }
        RedactionDecision::Block => {
            report.redacted = false;
            report.blocked = true;
            ("[BLOCKED]".to_string(), report)
        }
        RedactionDecision::AddAllowlistLiteral(value) => {
            session
                .add_exclusion_secret_pattern(regex::escape(&value), true)
                .await;
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::AddAllowlistRegex(value) => {
            session.add_exclusion_secret_pattern(value, true).await;
            (original, crate::content_gateway::ScanReport::safe())
        }
        RedactionDecision::AddBlocklist(value) => {
            session.add_exclusion_secret_pattern(value, false).await;
            report.redacted = false;
            report.blocked = true;
            ("[BLOCKED]".to_string(), report)
        }
    }
}

async fn enforce_sensitive_content_gateway(
    mut output: Box<dyn ToolOutput>,
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    tool_name: &str,
    call_id: &str,
) -> Box<dyn ToolOutput> {
    let epoch = turn.sensitive_paths.ignore_epoch();

    if let Some(provenance) = output.provenance().cloned() {
        if let Some(function_output) = output.as_any_mut().downcast_mut::<FunctionToolOutput>() {
            let mut gateway_cfg =
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion);
            if is_trusted_local_code_output(&provenance) {
                gateway_cfg.secret_patterns = false;
            }
            let gateway = crate::content_gateway::ContentGateway::new(gateway_cfg);
            let source = match &provenance {
                ToolProvenance::Filesystem { .. } => {
                    crate::exclusion_counters::ExclusionSource::Filesystem
                }
                ToolProvenance::Mcp { .. } => crate::exclusion_counters::ExclusionSource::Mcp,
                ToolProvenance::Shell { .. } => crate::exclusion_counters::ExclusionSource::Shell,
                ToolProvenance::Unattested { .. } => {
                    crate::exclusion_counters::ExclusionSource::Other
                }
            };
            let origin_type = provenance.origin_type();
            let origin_path = provenance.origin_path();
            let should_log = turn.exclusion.log_redactions_mode()
                != crate::config::types::LogRedactionsMode::Off;
            let log_context = crate::exclusion_log::RedactionLogContext {
                codex_home: &turn.codex_home,
                layer: crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                source,
                tool_name,
                origin_type,
                origin_path: origin_path.as_deref(),
                log_mode: turn.exclusion.log_redactions_mode(),
                max_bytes: turn.exclusion.log_redactions_max_bytes,
                max_files: turn.exclusion.log_redactions_max_files,
            };
            let context_label = format!("{tool_name} output");

            let record_report = |report: &crate::content_gateway::ScanReport| {
                if report.redacted || report.blocked {
                    let mut counters = turn
                        .exclusion_counters
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    counters.record(
                        crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                        source,
                        tool_name,
                        report.redacted,
                        report.blocked,
                    );
                }
            };

            for item in &mut function_output.body {
                if let FunctionCallOutputContentItem::InputText { text } = item {
                    let original_text = text.clone();
                    let (sanitized, report) = gateway.scan_text(
                        &original_text,
                        &turn.sensitive_paths,
                        &session.content_gateway_cache,
                        epoch,
                    );
                    let (next, report) = resolve_redaction_decision(
                        session,
                        turn,
                        call_id,
                        &context_label,
                        original_text.clone(),
                        sanitized,
                        report,
                    )
                    .await;
                    *text = next;
                    if should_log && (report.redacted || report.blocked) {
                        crate::exclusion_log::log_redaction_event(
                            &log_context,
                            &report,
                            &original_text,
                            text.as_str(),
                        );
                    }
                    record_report(&report);
                    if report.redacted {
                        function_output.success = Some(false);
                    }
                }
            }

            return output;
        }

        if let Some(mcp_output) = output.as_any_mut().downcast_mut::<McpToolOutput>() {
            let gateway = crate::content_gateway::ContentGateway::new(
                crate::content_gateway::GatewayConfig::from_exclusion(&turn.exclusion),
            );
            let mut report = crate::content_gateway::ScanReport::safe();
            let origin_type = provenance.origin_type();
            let origin_path = provenance.origin_path();
            let should_log = turn.exclusion.log_redactions_mode()
                != crate::config::types::LogRedactionsMode::Off;
            let log_context = crate::exclusion_log::RedactionLogContext {
                codex_home: &turn.codex_home,
                layer: crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                source: crate::exclusion_counters::ExclusionSource::Mcp,
                tool_name,
                origin_type,
                origin_path: origin_path.as_deref(),
                log_mode: turn.exclusion.log_redactions_mode(),
                max_bytes: turn.exclusion.log_redactions_max_bytes,
                max_files: turn.exclusion.log_redactions_max_files,
            };

            let mut scan_string = |s: &mut String| {
                let original = s.clone();
                let (next, r) = gateway.scan_text(
                    &original,
                    &turn.sensitive_paths,
                    &session.content_gateway_cache,
                    epoch,
                );
                *s = next;
                report.layers.extend(r.layers.iter().copied());
                report.redacted |= r.redacted;
                report.blocked |= r.blocked;
                report.reasons.extend(r.reasons.iter().copied());
                if should_log && (r.redacted || r.blocked) {
                    crate::exclusion_log::log_redaction_event(
                        &log_context,
                        &r,
                        &original,
                        s.as_str(),
                    );
                }
            };

            for block in &mut mcp_output.result.content {
                scan_json_value(block, &mut scan_string);
            }
            if let Some(structured_content) = &mut mcp_output.result.structured_content {
                scan_json_value(structured_content, &mut scan_string);
            }
            if let Some(meta) = &mut mcp_output.result.meta {
                scan_json_value(meta, &mut scan_string);
            }

            if report.redacted || report.blocked {
                let mut counters = turn
                    .exclusion_counters
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                counters.record(
                    crate::exclusion_counters::ExclusionLayer::Layer2OutputSanitization,
                    crate::exclusion_counters::ExclusionSource::Mcp,
                    tool_name,
                    report.redacted,
                    report.blocked,
                );
            }

            return output;
        }
    }

    output
}

fn scan_json_value(value: &mut serde_json::Value, scan_string: &mut impl FnMut(&mut String)) {
    match value {
        serde_json::Value::String(s) => scan_string(s),
        serde_json::Value::Array(items) => {
            for item in items {
                scan_json_value(item, scan_string);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                scan_json_value(value, scan_string);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn is_unattested_output(output: &dyn ToolOutput) -> bool {
    matches!(
        output.provenance(),
        Some(
            ToolProvenance::Shell { .. }
                | ToolProvenance::Mcp { .. }
                | ToolProvenance::Unattested { .. }
        )
    )
}

fn is_trusted_local_code_output(provenance: &ToolProvenance) -> bool {
    let ToolProvenance::Filesystem { path } = provenance else {
        return false;
    };
    let Some(extension) = path.extension().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "h"
            | "hpp"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "kts"
            | "m"
            | "mm"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "sql"
            | "swift"
            | "toml"
            | "ts"
            | "tsx"
            | "yaml"
            | "yml"
            | "zsh"
    )
}

async fn enforce_unattested_output_policy<WarnFut, WarnFn, ApprovalFut, ApprovalFn>(
    output: Box<dyn ToolOutput>,
    policy: crate::config::types::UnattestedOutputPolicy,
    tool_name: &str,
    call_id: &str,
    mut warn: WarnFn,
    mut request_approval: ApprovalFn,
) -> Box<dyn ToolOutput>
where
    WarnFn: FnMut(String) -> WarnFut,
    WarnFut: std::future::Future<Output = ()>,
    ApprovalFn: FnMut(Vec<String>) -> ApprovalFut,
    ApprovalFut: std::future::Future<Output = ReviewDecision>,
{
    match policy {
        crate::config::types::UnattestedOutputPolicy::Allow => output,
        crate::config::types::UnattestedOutputPolicy::Warn => {
            warn(unattested_output_warning_message(
                output.as_ref(),
                policy,
                tool_name,
                call_id,
            ))
            .await;
            output
        }
        crate::config::types::UnattestedOutputPolicy::Confirm => {
            warn(unattested_output_warning_message(
                output.as_ref(),
                policy,
                tool_name,
                call_id,
            ))
            .await;

            let mut command = vec!["send_unattested_output".to_string(), tool_name.to_string()];
            if let Some(provenance) = output.provenance() {
                command.push(provenance.origin_type().to_string());
                if let Some(path) = provenance.origin_path() {
                    command.push(path);
                }
            }

            let decision = request_approval(command).await;
            match decision {
                ReviewDecision::Approved
                | ReviewDecision::ApprovedForSession
                | ReviewDecision::ApprovedExecpolicyAmendment { .. } => output,
                ReviewDecision::Denied
                | ReviewDecision::Abort
                | ReviewDecision::NetworkPolicyAmendment { .. }
                | ReviewDecision::TimedOut => block_unattested_output(output),
            }
        }
        crate::config::types::UnattestedOutputPolicy::Block => block_unattested_output(output),
    }
}

fn unattested_output_warning_message(
    output: &dyn ToolOutput,
    policy: crate::config::types::UnattestedOutputPolicy,
    tool_name: &str,
    call_id: &str,
) -> String {
    let origin = output
        .provenance()
        .and_then(ToolProvenance::origin_path)
        .unwrap_or_else(|| String::from("<unknown>"));
    format!(
        "unattested tool output ({tool_name}, call_id={call_id}, origin={origin}) may contain sensitive data; policy={policy:?}"
    )
}

fn block_unattested_output(mut output: Box<dyn ToolOutput>) -> Box<dyn ToolOutput> {
    let message = "unattested tool output blocked by policy".to_string();
    let provenance = output.provenance().cloned();
    if let Some(function_output) = output.as_any_mut().downcast_mut::<FunctionToolOutput>() {
        function_output.body = vec![FunctionCallOutputContentItem::InputText { text: message }];
        function_output.success = Some(false);
        return output;
    }
    if output
        .as_any_mut()
        .downcast_mut::<McpToolOutput>()
        .is_some()
    {
        let mut blocked = FunctionToolOutput::from_text(message, Some(false));
        blocked.provenance = provenance;
        return Box::new(blocked);
    }
    output
}

pub struct ToolRegistryBuilder {
    handlers: HashMap<ToolName, Arc<dyn AnyToolHandler>>,
    specs: Vec<ConfiguredToolSpec>,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            specs: Vec::new(),
        }
    }

    pub fn push_spec(&mut self, spec: ToolSpec) {
        self.push_spec_with_parallel_support(spec, /*supports_parallel_tool_calls*/ false);
    }

    pub fn push_spec_with_parallel_support(
        &mut self,
        spec: ToolSpec,
        supports_parallel_tool_calls: bool,
    ) {
        self.specs
            .push(ConfiguredToolSpec::new(spec, supports_parallel_tool_calls));
    }

    pub fn register_handler<H>(&mut self, name: impl Into<ToolName>, handler: Arc<H>)
    where
        H: ToolHandler + 'static,
    {
        let name = name.into();
        let display_name = name.display();
        let handler: Arc<dyn AnyToolHandler> = handler;
        if self.handlers.insert(name, handler).is_some() {
            warn!("overwriting handler for tool {display_name}");
        }
    }

    pub fn build(self) -> (Vec<ConfiguredToolSpec>, ToolRegistry) {
        let registry = ToolRegistry::new(self.handlers);
        (self.specs, registry)
    }
}

fn unsupported_tool_call_message(payload: &ToolPayload, tool_name: &ToolName) -> String {
    let tool_name = tool_name.display();
    match payload {
        ToolPayload::Custom { .. } => format!("unsupported custom tool call: {tool_name}"),
        _ => format!("unsupported call: {tool_name}"),
    }
}

fn plan_mode_tool_block_message(mode: ModeKind, tool_name: &str) -> Option<String> {
    if mode != ModeKind::Plan || !is_plan_mode_file_mutation_tool(tool_name) {
        return None;
    }

    Some(format!(
        "`{tool_name}` is blocked in Plan mode because it can mutate files. Switch to Default mode to run file edits."
    ))
}

fn is_plan_mode_file_mutation_tool(tool_name: &str) -> bool {
    let trailing = tool_name
        .rsplit_once("__")
        .map_or(tool_name, |(_, suffix)| suffix);
    let canonical = trailing
        .rsplit_once('/')
        .map_or(trailing, |(_, suffix)| suffix);
    matches!(canonical, "apply_patch" | "write_file" | "edit_file")
}

fn sandbox_policy_tag(policy: &SandboxPolicy) -> &'static str {
    match policy {
        SandboxPolicy::ReadOnly { .. } => "read-only",
        SandboxPolicy::WorkspaceWrite { .. } => "workspace-write",
        SandboxPolicy::DangerFullAccess => "danger-full-access",
        SandboxPolicy::ExternalSandbox { .. } => "external-sandbox",
    }
}

impl From<&ToolPayload> for HookToolInput {
    fn from(payload: &ToolPayload) -> Self {
        match payload {
            ToolPayload::Function { arguments } => HookToolInput::Function {
                arguments: arguments.clone(),
            },
            ToolPayload::ToolSearch { arguments } => HookToolInput::Function {
                arguments: serde_json::json!({
                    "query": arguments.query,
                    "limit": arguments.limit,
                })
                .to_string(),
            },
            ToolPayload::Custom { input } => HookToolInput::Custom {
                input: input.clone(),
            },
            ToolPayload::LocalShell { params } => HookToolInput::LocalShell {
                params: HookToolInputLocalShell {
                    command: params.command.clone(),
                    workdir: params.workdir.clone(),
                    timeout_ms: params.timeout_ms,
                    sandbox_permissions: params.sandbox_permissions,
                    prefix_rule: params.prefix_rule.clone(),
                    justification: params.justification.clone(),
                },
            },
            ToolPayload::Mcp {
                server,
                tool,
                raw_arguments,
            } => HookToolInput::Mcp {
                server: server.clone(),
                tool: tool.clone(),
                arguments: raw_arguments.clone(),
            },
        }
    }
}

fn hook_tool_kind(tool_input: &HookToolInput) -> HookToolKind {
    match tool_input {
        HookToolInput::Function { .. } => HookToolKind::Function,
        HookToolInput::Custom { .. } => HookToolKind::Custom,
        HookToolInput::LocalShell { .. } => HookToolKind::LocalShell,
        HookToolInput::Mcp { .. } => HookToolKind::Mcp,
    }
}

struct AfterToolUseHookDispatch<'a> {
    invocation: &'a ToolInvocation,
    output_preview: String,
    success: bool,
    executed: bool,
    duration: Duration,
    mutating: bool,
}

async fn dispatch_after_tool_use_hook(
    dispatch: AfterToolUseHookDispatch<'_>,
) -> Option<FunctionCallError> {
    let AfterToolUseHookDispatch { invocation, .. } = dispatch;
    let session = invocation.session.as_ref();
    let turn = invocation.turn.as_ref();
    let tool_input = HookToolInput::from(&invocation.payload);
    let hook_outcomes = session
        .hooks()
        .dispatch(HookPayload {
            session_id: session.conversation_id,
            cwd: turn.cwd.to_path_buf(),
            client: turn.app_server_client_name.clone(),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: turn.sub_id.clone(),
                    call_id: invocation.call_id.clone(),
                    tool_name: invocation.tool_name.display(),
                    tool_kind: hook_tool_kind(&tool_input),
                    tool_input,
                    executed: dispatch.executed,
                    success: dispatch.success,
                    duration_ms: u64::try_from(dispatch.duration.as_millis()).unwrap_or(u64::MAX),
                    mutating: dispatch.mutating,
                    sandbox: sandbox_tag(&turn.sandbox_policy, turn.windows_sandbox_level)
                        .to_string(),
                    sandbox_policy: sandbox_policy_tag(&turn.sandbox_policy).to_string(),
                    output_preview: dispatch.output_preview.clone(),
                },
            },
        })
        .await;

    for hook_outcome in hook_outcomes {
        let hook_name = hook_outcome.hook_name;
        match hook_outcome.result {
            HookResult::Success => {}
            HookResult::FailedContinue(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name.display(),
                    hook_name = %hook_name,
                    error = %error,
                    "after_tool_use hook failed; continuing"
                );
            }
            HookResult::FailedAbort(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name.display(),
                    hook_name = %hook_name,
                    error = %error,
                    "after_tool_use hook failed; aborting operation"
                );
                return Some(FunctionCallError::Fatal(format!(
                    "after_tool_use hook '{hook_name}' failed and aborted operation: {error}"
                )));
            }
        }
    }

    None
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use codex_protocol::mcp::CallToolResult;
    use pretty_assertions::assert_eq;

    fn unattested_output() -> Box<dyn ToolOutput> {
        Box::new(
            FunctionToolOutput::from_text("payload".to_string(), Some(true)).with_provenance(
                ToolProvenance::Unattested {
                    origin_type: "mcp",
                    origin_path: Some("server/tool".to_string()),
                },
            ),
        )
    }

    #[test]
    fn is_unattested_output_matches_expected_provenance() {
        let output = unattested_output();
        assert_eq!(true, super::is_unattested_output(output.as_ref()));

        let output = FunctionToolOutput::from_text("payload".to_string(), Some(true))
            .with_provenance(ToolProvenance::Filesystem {
                path: std::path::PathBuf::from("/tmp/file"),
            });
        assert_eq!(false, super::is_unattested_output(&output));

        let output = McpToolOutput {
            result: CallToolResult::from_error_text("boom".to_string()),
            wall_time: Duration::ZERO,
            provenance: ToolProvenance::Mcp {
                server: "server".to_string(),
                tool: "tool".to_string(),
            },
        };
        assert_eq!(true, super::is_unattested_output(&output));
    }

    #[test]
    fn trusted_local_code_output_matches_only_filesystem_code_extensions() {
        let trusted = ToolProvenance::Filesystem {
            path: std::path::PathBuf::from("/tmp/src/main.rs"),
        };
        assert_eq!(true, super::is_trusted_local_code_output(&trusted));

        let markdown = ToolProvenance::Filesystem {
            path: std::path::PathBuf::from("/tmp/docs/readme.md"),
        };
        assert_eq!(false, super::is_trusted_local_code_output(&markdown));

        let shell = ToolProvenance::Shell {
            cwd: std::path::PathBuf::from("/tmp"),
        };
        assert_eq!(false, super::is_trusted_local_code_output(&shell));
    }

    #[test]
    fn block_unattested_output_replaces_payload_with_policy_message() {
        let mut blocked = super::block_unattested_output(unattested_output());
        let function_output = blocked
            .as_any_mut()
            .downcast_mut::<FunctionToolOutput>()
            .expect("expected function output");
        assert_eq!(
            function_output.body,
            vec![FunctionCallOutputContentItem::InputText {
                text: "unattested tool output blocked by policy".to_string(),
            }]
        );
        assert_eq!(function_output.success, Some(false));
        assert_eq!(
            function_output.provenance(),
            Some(&ToolProvenance::Unattested {
                origin_type: "mcp",
                origin_path: Some("server/tool".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_warn_emits_warning() {
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut output = super::enforce_unattested_output_policy(
            unattested_output(),
            crate::config::types::UnattestedOutputPolicy::Warn,
            "mcp__server__tool",
            "call-1",
            {
                let warnings = std::sync::Arc::clone(&warnings);
                move |message| {
                    let warnings = std::sync::Arc::clone(&warnings);
                    async move {
                        warnings
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(message);
                    }
                }
            },
            |_command| async { ReviewDecision::Abort },
        )
        .await;

        let warnings = warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            "unattested tool output (mcp__server__tool, call_id=call-1, origin=server/tool) may contain sensitive data; policy=Warn"
        );
        let function_output = output
            .as_any_mut()
            .downcast_mut::<FunctionToolOutput>()
            .expect("expected function output");
        assert_eq!(function_output.success, Some(true));
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_confirm_requests_approval_and_blocks_on_denied() {
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let approval_commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut output = super::enforce_unattested_output_policy(
            unattested_output(),
            crate::config::types::UnattestedOutputPolicy::Confirm,
            "mcp__server__tool",
            "call-1",
            {
                let warnings = std::sync::Arc::clone(&warnings);
                move |message| {
                    let warnings = std::sync::Arc::clone(&warnings);
                    async move {
                        warnings
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(message);
                    }
                }
            },
            {
                let approval_commands = std::sync::Arc::clone(&approval_commands);
                move |command| {
                    let approval_commands = std::sync::Arc::clone(&approval_commands);
                    async move {
                        approval_commands
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(command);
                        ReviewDecision::Denied
                    }
                }
            },
        )
        .await;

        let warnings = warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let approval_commands = approval_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(warnings.len(), 1);
        assert_eq!(approval_commands.len(), 1);
        assert_eq!(
            approval_commands[0],
            vec![
                "send_unattested_output".to_string(),
                "mcp__server__tool".to_string(),
                "mcp".to_string(),
                "server/tool".to_string(),
            ]
        );
        let function_output = output
            .as_any_mut()
            .downcast_mut::<FunctionToolOutput>()
            .expect("expected function output");
        assert_eq!(function_output.success, Some(false));
    }

    #[tokio::test]
    async fn enforce_unattested_output_policy_confirm_allows_on_approved() {
        let approvals = std::sync::Arc::new(std::sync::Mutex::new(0_u64));

        let mut output = super::enforce_unattested_output_policy(
            unattested_output(),
            crate::config::types::UnattestedOutputPolicy::Confirm,
            "mcp__server__tool",
            "call-1",
            |_message| async {},
            {
                let approvals = std::sync::Arc::clone(&approvals);
                move |_command| {
                    let approvals = std::sync::Arc::clone(&approvals);
                    async move {
                        let mut guard = approvals
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *guard += 1;
                        ReviewDecision::Approved
                    }
                }
            },
        )
        .await;

        assert_eq!(
            *approvals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            1
        );
        let function_output = output
            .as_any_mut()
            .downcast_mut::<FunctionToolOutput>()
            .expect("expected function output");
        assert_eq!(function_output.success, Some(true));
    }

    #[test]
    fn parse_redaction_prompt_answer_maps_answers() {
        assert!(matches!(
            super::parse_redaction_prompt_answer("Allow once"),
            Some(super::RedactionPromptAnswer::AllowOnce)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Allow for this session"),
            Some(super::RedactionPromptAnswer::AllowForSession)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Redact"),
            Some(super::RedactionPromptAnswer::Redact)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Block"),
            Some(super::RedactionPromptAnswer::Block)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Add to allowlist"),
            Some(super::RedactionPromptAnswer::AddToAllowlist)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Add to blocklist"),
            Some(super::RedactionPromptAnswer::AddToBlocklist)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Reveal matched values"),
            Some(super::RedactionPromptAnswer::RevealMatches)
        ));
        assert!(matches!(
            super::parse_redaction_prompt_answer("Hide matched values"),
            Some(super::RedactionPromptAnswer::HideMatches)
        ));
        assert_eq!(super::parse_redaction_prompt_answer("unknown"), None);
    }

    #[test]
    fn format_redaction_matches_returns_summary() {
        let report = crate::content_gateway::ScanReport {
            layers: Vec::new(),
            redacted: true,
            blocked: false,
            reasons: vec![crate::content_gateway::RedactionReason::SecretPattern],
            matches: vec![crate::content_gateway::RedactionMatch {
                reason: crate::content_gateway::RedactionReason::SecretPattern,
                value: "token_abc123".to_string(),
            }],
        };

        let summary = super::format_redaction_matches(&report, "L2-output_sanitization", false);
        assert_eq!(
            summary,
            Some(
                "Matched content (L2-output_sanitization):\n- [REDACTED toke...c123 sha256:424fdc9e] (reason: Secret pattern)"
                    .to_string(),
            )
        );
    }

    #[test]
    fn format_redaction_matches_can_reveal_secret_values() {
        let report = crate::content_gateway::ScanReport {
            layers: Vec::new(),
            redacted: true,
            blocked: false,
            reasons: vec![crate::content_gateway::RedactionReason::SecretPattern],
            matches: vec![crate::content_gateway::RedactionMatch {
                reason: crate::content_gateway::RedactionReason::SecretPattern,
                value: "token_abc123".to_string(),
            }],
        };

        let summary = super::format_redaction_matches(&report, "L2-output_sanitization", true);
        assert_eq!(
            summary,
            Some(
                "Matched content (L2-output_sanitization):\n- token_abc123 (sha256:424fdc9e) (reason: Secret pattern)"
                    .to_string(),
            )
        );
    }

    #[test]
    fn plan_mode_blocks_file_mutation_tools_and_allows_read_only_tools() {
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "apply_patch").is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "mcp__filesystem__write_file")
                .is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "mcp__filesystem__edit_file")
                .is_some(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Plan, "read_file").is_none(),
            true
        );
        assert_eq!(
            super::plan_mode_tool_block_message(ModeKind::Default, "apply_patch").is_none(),
            true
        );
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
