#![allow(clippy::expect_used)]

use super::*;
use codex_apply_patch::MaybeApplyPatchVerified;
use codex_protocol::protocol::GranularApprovalConfig;
use core_test_support::PathBufExt;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::fs;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use tempfile::tempdir;

fn make_test_request(dir: &tempfile::TempDir, patch: &str) -> ApplyPatchRequest {
    let argv = vec!["apply_patch".to_string(), patch.to_string()];
    let action = match codex_apply_patch::maybe_parse_apply_patch_verified(&argv, dir.path()) {
        MaybeApplyPatchVerified::Body(action) => action,
        other => panic!("expected Body apply_patch action, got {other:?}"),
    };

    ApplyPatchRequest {
        action,
        file_paths: Vec::new(),
        changes: HashMap::new(),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
        timeout_ms: None,
    }
}

#[test]
fn wants_no_sandbox_approval_granular_respects_sandbox_flag() {
    let runtime = ApplyPatchRuntime::new();
    assert!(runtime.wants_no_sandbox_approval(AskForApproval::OnRequest));
    assert!(
        !runtime.wants_no_sandbox_approval(AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: false,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }))
    );
    assert!(
        runtime.wants_no_sandbox_approval(AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }))
    );
}

#[test]
fn guardian_review_request_includes_patch_context() {
    let path = std::env::temp_dir()
        .join("guardian-apply-patch-test.txt")
        .abs();
    let action = ApplyPatchAction::new_add_for_test(&path, "hello".to_string());
    let expected_cwd = action.cwd.to_path_buf();
    let expected_patch = action.patch.clone();
    let request = ApplyPatchRequest {
        action,
        file_paths: vec![path.clone()],
        changes: HashMap::from([(
            path.to_path_buf(),
            FileChange::Add {
                content: "hello".to_string(),
            },
        )]),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
        timeout_ms: None,
    };

    let guardian_request = ApplyPatchRuntime::build_guardian_review_request(&request, "call-1");

    assert_eq!(
        guardian_request,
        GuardianApprovalRequest::ApplyPatch {
            id: "call-1".to_string(),
            cwd: expected_cwd,
            files: request.file_paths,
            patch: expected_patch,
        }
    );
}

#[test]
fn apply_patch_in_process_rejects_empty_patch() {
    let dir = tempdir().expect("tempdir");
    let req = make_test_request(&dir, "*** Begin Patch\n*** End Patch");

    let out = ApplyPatchRuntime::apply_patch_in_process(&req);

    assert_eq!(out.exit_code, 1);
    assert_eq!(out.stdout.text, "");
    assert_eq!(out.stderr.text, "No files were modified.\n");
}

#[test]
fn apply_patch_in_process_avoids_absolute_paths_in_errors() {
    let dir = tempdir().expect("tempdir");
    let missing_path = dir.path().join("missing.txt");
    fs::write(&missing_path, "old\n").expect("seed file");
    let req = make_test_request(
        &dir,
        "*** Begin Patch\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch",
    );
    fs::remove_file(&missing_path).expect("delete file after verification");

    let out = ApplyPatchRuntime::apply_patch_in_process(&req);

    assert_eq!(out.exit_code, 1);
    assert!(
        out.stderr.text.contains("missing.txt"),
        "expected missing file name in stderr, got {:?}",
        out.stderr.text
    );
    assert!(
        !out.stderr.text.contains(&dir.path().display().to_string()),
        "expected stderr to avoid absolute cwd, got {:?}",
        out.stderr.text
    );
}

#[test]
fn apply_patch_in_process_updates_files_and_reports_summary() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("update.txt");
    fs::write(&path, "foo\nbar\n").expect("seed file");
    let req = make_test_request(
        &dir,
        "*** Begin Patch\n*** Update File: update.txt\n@@\n-bar\n+baz\n*** End Patch",
    );

    let out = ApplyPatchRuntime::apply_patch_in_process(&req);

    assert_eq!(out.exit_code, 0);
    assert_eq!(
        out.stdout.text,
        "Success. Updated the following files:\nM update.txt\n"
    );
    assert_eq!(
        fs::read_to_string(path).expect("read updated file"),
        "foo\nbaz\n"
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn build_sandbox_command_prefers_configured_codex_self_exe_for_apply_patch() {
    let path = std::env::temp_dir()
        .join("apply-patch-current-exe-test.txt")
        .abs();
    let action = ApplyPatchAction::new_add_for_test(&path, "hello".to_string());
    let request = ApplyPatchRequest {
        action,
        file_paths: vec![path.clone()],
        changes: HashMap::from([(
            path.to_path_buf(),
            FileChange::Add {
                content: "hello".to_string(),
            },
        )]),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
        timeout_ms: None,
    };
    let codex_self_exe = PathBuf::from("/tmp/codex");

    let command = ApplyPatchRuntime::build_sandbox_command(&request, Some(&codex_self_exe))
        .expect("build sandbox command");

    assert_eq!(command.program, codex_self_exe.into_os_string());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn build_sandbox_command_falls_back_to_current_exe_for_apply_patch() {
    let path = std::env::temp_dir()
        .join("apply-patch-current-exe-test.txt")
        .abs();
    let action = ApplyPatchAction::new_add_for_test(&path, "hello".to_string());
    let request = ApplyPatchRequest {
        action,
        file_paths: vec![path.clone()],
        changes: HashMap::from([(
            path.to_path_buf(),
            FileChange::Add {
                content: "hello".to_string(),
            },
        )]),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
        timeout_ms: None,
    };

    let command = ApplyPatchRuntime::build_sandbox_command(&request, /*codex_self_exe*/ None)
        .expect("build sandbox command");

    assert_eq!(
        command.program,
        std::env::current_exe()
            .expect("current exe")
            .into_os_string()
    );
}
