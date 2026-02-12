use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

fn run_plan_command(
    codex_home: &TempDir,
    args: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    run_plan_command_in_dir(codex_home, codex_home.path(), args)
}

fn run_plan_command_in_dir(
    codex_home: &TempDir,
    cwd: &std::path::Path,
    args: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .current_dir(cwd)
        .args(args)
        .output()?;
    Ok(output)
}

#[test]
fn plan_open_creates_template_and_sets_active_pointer() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;
    let output = run_plan_command(&codex_home, &["plan", "open"])?;
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Active plan file set to"));

    let active_pointer = codex_home.path().join("plans").join(".active-plan");
    assert!(active_pointer.exists());

    let active_path = PathBuf::from(fs::read_to_string(&active_pointer)?.trim());
    assert!(active_path.exists());
    let active_canonical = fs::canonicalize(&active_path)?;
    let expected_base = fs::canonicalize(codex_home.path().join("plans"))?;
    assert!(active_canonical.starts_with(&expected_base));

    let content = fs::read_to_string(&active_path)?;
    assert!(content.contains("Status: Draft"));
    assert!(content.contains("## Plan (checklist)"));

    Ok(())
}

#[test]
fn plan_status_reports_active_plan_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;
    let open_output = run_plan_command(&codex_home, &["plan", "open"])?;
    assert!(open_output.status.success());

    let status_output = run_plan_command(&codex_home, &["plan", "status"])?;
    assert!(status_output.status.success());
    let stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(stdout.contains("Plan status"));
    assert!(stdout.contains("Active plan status: Draft"));
    assert!(stdout.contains("TODOs remaining: 3"));

    Ok(())
}

#[test]
fn plan_done_marks_active_plan_done_and_moves_list_scope() -> Result<(), Box<dyn std::error::Error>>
{
    let codex_home = TempDir::new()?;
    let open_output = run_plan_command(&codex_home, &["plan", "open"])?;
    assert!(open_output.status.success());

    let done_output = run_plan_command(&codex_home, &["plan", "done"])?;
    assert!(done_output.status.success());
    let done_stdout = String::from_utf8_lossy(&done_output.stdout);
    assert!(done_stdout.contains("Marked plan as done"));

    let active_pointer = codex_home.path().join("plans").join(".active-plan");
    let active_path = PathBuf::from(fs::read_to_string(active_pointer)?.trim());
    let content = fs::read_to_string(&active_path)?;
    assert!(content.contains("Status: Done"));

    let open_list_output = run_plan_command(&codex_home, &["plan", "list", "open"])?;
    assert!(open_list_output.status.success());
    let open_list_stdout = String::from_utf8_lossy(&open_list_output.stdout);
    assert!(open_list_stdout.contains("- No plans found."));

    let closed_list_output = run_plan_command(&codex_home, &["plan", "list", "closed"])?;
    assert!(closed_list_output.status.success());
    let closed_list_stdout = String::from_utf8_lossy(&closed_list_output.stdout);
    assert!(closed_list_stdout.contains("[Done] /plan task"));

    Ok(())
}

#[test]
fn plan_archive_marks_active_plan_archived_and_filters_lists()
-> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;
    let open_output = run_plan_command(&codex_home, &["plan", "open"])?;
    assert!(open_output.status.success());

    let archive_output = run_plan_command(&codex_home, &["plan", "archive"])?;
    assert!(archive_output.status.success());
    let archive_stdout = String::from_utf8_lossy(&archive_output.stdout);
    assert!(archive_stdout.contains("Archived plan"));

    let active_pointer = codex_home.path().join("plans").join(".active-plan");
    let active_path = PathBuf::from(fs::read_to_string(active_pointer)?.trim());
    let content = fs::read_to_string(&active_path)?;
    assert!(content.contains("Status: Archived"));

    let all_list_output = run_plan_command(&codex_home, &["plan", "list", "all"])?;
    assert!(all_list_output.status.success());
    let all_list_stdout = String::from_utf8_lossy(&all_list_output.stdout);
    assert!(all_list_stdout.contains("- No plans found."));

    let archived_list_output = run_plan_command(&codex_home, &["plan", "list", "archived"])?;
    assert!(archived_list_output.status.success());
    let archived_list_stdout = String::from_utf8_lossy(&archived_list_output.stdout);
    assert!(archived_list_stdout.contains("[Archived] /plan task"));

    Ok(())
}

#[test]
fn plan_mode_adr_lite_uses_repo_docs_impl_plans_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    let codex_home = TempDir::new()?;
    let repo_dir = codex_home.path().join("repo");
    fs::create_dir_all(&repo_dir)?;
    let init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_dir)
        .output()?;
    assert!(init.status.success(), "git init should succeed");

    let state_dir = codex_home.path().join("plans");
    fs::create_dir_all(&state_dir)?;
    fs::write(state_dir.join(".mode"), "adr-lite")?;

    let open_output = run_plan_command_in_dir(&codex_home, &repo_dir, &["plan", "open"])?;
    assert!(open_output.status.success());

    let active_pointer = state_dir.join(".active-plan");
    let active_path = PathBuf::from(fs::read_to_string(active_pointer)?.trim());
    let active_canonical = fs::canonicalize(&active_path)?;
    let expected_base = fs::canonicalize(repo_dir.join("docs/impl-plans"))?;
    assert!(active_canonical.starts_with(&expected_base));

    let content = fs::read_to_string(&active_path)?;
    assert!(content.contains("TODOs remaining: 4"));
    assert!(content.contains("## Learnings"));
    assert!(content.contains("## Memories"));

    Ok(())
}
