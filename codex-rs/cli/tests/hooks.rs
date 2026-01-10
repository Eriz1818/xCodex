use std::fs;

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn hooks_init_is_idempotent_without_force() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init"])
        .output()?;
    assert!(output.status.success());

    let hooks_dir = codex_home.path().join("hooks");
    let script = hooks_dir.join("log_all_jsonl.py");
    assert!(script.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&script)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    fs::write(
        &script,
        format!("{}\n# marker\n", fs::read_to_string(&script)?),
    )?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init"])
        .output()?;
    assert!(output.status.success());

    let contents = fs::read_to_string(&script)?;
    assert!(contents.contains("# marker"));

    Ok(())
}

#[test]
fn hooks_init_force_overwrites_existing_files() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init"])
        .output()?;
    assert!(output.status.success());

    let hooks_dir = codex_home.path().join("hooks");
    let script = hooks_dir.join("log_all_jsonl.py");
    assert!(script.exists());

    fs::write(
        &script,
        format!("{}\n# marker\n", fs::read_to_string(&script)?),
    )?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init", "--force"])
        .output()?;
    assert!(output.status.success());

    let contents = fs::read_to_string(&script)?;
    assert!(!contents.contains("# marker"));

    Ok(())
}

#[test]
fn hooks_init_no_print_config_suppresses_snippet() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init", "--no-print-config"])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("[hooks]"));
    assert!(!stdout.contains("Paste this into"));
    assert!(!stdout.contains("Then run:"));

    let hooks_dir = codex_home.path().join("hooks");
    let script = hooks_dir.join("log_all_jsonl.py");
    assert!(script.exists());

    Ok(())
}

#[test]
fn hooks_init_printed_snippet_is_valid_toml() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "init"])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout
        .find("[hooks]")
        .expect("expected hooks init output to include a [hooks] snippet");
    let snippet_plus = &stdout[start..];
    let end = snippet_plus
        .find("\nThen run:")
        .unwrap_or(snippet_plus.len());
    let snippet = &snippet_plus[..end];

    let parsed: toml::Value = toml::from_str(snippet)?;
    let hooks = parsed
        .as_table()
        .and_then(|t| t.get("hooks"))
        .and_then(toml::Value::as_table)
        .expect("expected snippet to contain a [hooks] table");
    assert!(hooks.contains_key("agent_turn_complete"));
    assert!(hooks.contains_key("tool_call_finished"));

    Ok(())
}

#[test]
fn hooks_list_prints_configured_events_in_stable_order() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;
    fs::write(
        codex_home.path().join("config.toml"),
        r#"
[hooks]
agent_turn_complete = [["python3", "/tmp/hook1.py"]]
tool_call_finished = [["python3", "/tmp/hook2.py"]]
"#,
    )?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["hooks", "list"])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("hooks.session_start:"));
    assert!(stdout.contains("hooks.agent_turn_complete:"));
    assert!(stdout.contains("hooks.tool_call_finished:"));

    let agent_idx = stdout
        .find("hooks.agent_turn_complete:")
        .expect("agent_turn_complete present");
    let tool_idx = stdout
        .find("hooks.tool_call_finished:")
        .expect("tool_call_finished present");
    assert!(tool_idx > agent_idx);

    Ok(())
}
