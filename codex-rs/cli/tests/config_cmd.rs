use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

#[test]
fn config_path_prints_user_config_path() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path();

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CODEX_HOME: "))
        .stdout(predicate::str::contains("User config: "))
        .stdout(predicate::str::contains("user: "));

    Ok(())
}

#[test]
fn config_doctor_reports_unknown_keys() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path();
    fs::write(codex_home.join("config.toml"), "unknown_root_key = 123\n")?;

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home)
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unknown keys"))
        .stdout(predicate::str::contains("unknown_root_key"))
        .stdout(predicate::str::contains("Next step:"));

    Ok(())
}

#[test]
fn config_doctor_reports_parse_errors_with_next_step() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path();
    fs::write(codex_home.join("config.toml"), "model = \n")?;

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home)
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("failed to parse"))
        .stdout(predicate::str::contains("Next step:"));

    Ok(())
}

#[test]
fn config_doctor_reports_missing_profile_with_next_step() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path();
    fs::write(codex_home.join("config.toml"), "profile = \"nope\"\n")?;

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home)
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config profile"))
        .stdout(predicate::str::contains("not found"))
        .stdout(predicate::str::contains("Next step:"));

    Ok(())
}

#[test]
fn config_edit_prints_path_when_editor_missing() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path();

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home)
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .args(["config", "edit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Config file: "))
        .stdout(predicate::str::contains("Cannot open editor:"));

    Ok(())
}

#[test]
fn config_edit_project_creates_dot_codex_config_when_missing() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir)?;

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.current_dir(&project_dir)
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .args(["config", "edit", "--project"])
        .assert()
        .success();

    assert!(project_dir.join(".codex/config.toml").is_file());
    Ok(())
}

#[test]
fn config_doctor_reports_overrides_with_next_step() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let codex_home = tmp.path().join("codex-home");
    let project_dir = tmp.path().join("project");

    fs::create_dir_all(&codex_home)?;
    fs::create_dir_all(project_dir.join(".git"))?;
    fs::create_dir_all(project_dir.join(".codex"))?;
    let project_dir = fs::canonicalize(&project_dir)?;
    let project_key = project_dir.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        codex_home.join("config.toml"),
        format!(
            "hide_agent_reasoning = true\n\n[projects.\"{project_key}\"]\ntrust_level = \"trusted\"\n"
        ),
    )?;
    fs::write(
        project_dir.join(".codex/config.toml"),
        "hide_agent_reasoning = false\n",
    )?;

    let mut cmd = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.current_dir(&project_dir)
        .env("CODEX_HOME", &codex_home)
        .args(["config", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("overridden"))
        .stdout(predicate::str::contains("Next step:"));

    Ok(())
}
