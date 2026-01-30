use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

use crate::config::CONFIG_TOML_FILE;
use crate::config::HooksConfig;
use crate::config::types::Themes;
use crate::config::types::Tui;
use crate::config::types::Worktrees;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

const XCODEX_DEFAULT_HOME_DIRNAME: &str = ".xcodex";
const XCODEX_EXE_STEM: &str = "xcodex";

/// How TUI2 should interpret mouse scroll events.
///
/// Terminals generally encode both mouse wheels and trackpads as the same "scroll up/down" mouse
/// button events, without a magnitude. This setting controls whether Codex uses a heuristic to
/// infer wheel vs trackpad per stream, or forces a specific behavior.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScrollInputMode {
    /// Infer wheel vs trackpad behavior per scroll stream.
    #[default]
    Auto,
    /// Always treat scroll events as mouse-wheel input (fixed lines per tick).
    Wheel,
    /// Always treat scroll events as trackpad input (fractional accumulation).
    Trackpad,
}

/// How the TUI should render xcodex "xtreme mode" styling.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum XtremeMode {
    /// Enable xtreme styling when invoked as `xcodex`.
    Auto,
    /// Always enable xtreme styling.
    #[default]
    On,
    /// Disable xtreme styling (prefer upstream-like visuals).
    Off,
}

pub fn xcodex_default_home_dirname() -> &'static str {
    XCODEX_DEFAULT_HOME_DIRNAME
}

pub fn default_exclusion_files() -> Vec<String> {
    Vec::new()
}

#[derive(Debug, Clone, PartialEq)]
pub struct XcodexRuntimeConfig {
    pub notify: Option<Vec<String>>,
    pub hooks: HooksConfig,
    pub tui_xtreme_mode: XtremeMode,
    pub tui_ramps_rotate: bool,
    pub tui_ramps_build: bool,
    pub tui_ramps_devops: bool,
    pub tui_confirm_exit_with_running_hooks: bool,
    pub themes: Themes,
    pub worktrees_auto_link_shared_dirs: bool,
}

impl XcodexRuntimeConfig {
    pub fn from_toml(
        notify: Option<Vec<String>>,
        hooks: HooksConfig,
        tui: Option<&Tui>,
        themes: Option<Themes>,
        worktrees: Option<&Worktrees>,
    ) -> Self {
        Self {
            notify,
            hooks,
            tui_xtreme_mode: tui.map(|t| t.xtreme_mode).unwrap_or_default(),
            tui_ramps_rotate: tui.map(|t| t.ramps_rotate).unwrap_or(true),
            tui_ramps_build: tui.map(|t| t.ramps_build).unwrap_or(true),
            tui_ramps_devops: tui.map(|t| t.ramps_devops).unwrap_or(true),
            tui_confirm_exit_with_running_hooks: tui
                .map(|t| t.confirm_exit_with_running_hooks)
                .unwrap_or(true),
            themes: themes.unwrap_or_default(),
            worktrees_auto_link_shared_dirs: worktrees
                .map(|w| w.auto_link_shared_dirs)
                .unwrap_or(false),
        }
    }
}

fn is_xcodex_exe_name(name: &OsStr) -> bool {
    let Some(stem) = Path::new(name).file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    stem == XCODEX_EXE_STEM || stem.starts_with("xcodex-")
}

/// Returns `true` when this process appears to have been invoked via the
/// `xcodex` binary name (e.g. installed as `~/.local/bin/xcodex`).
pub fn is_xcodex_invocation() -> bool {
    if let Some(argv0) = std::env::args_os().next()
        && is_xcodex_exe_name(&argv0)
    {
        return true;
    }

    if let Ok(exe) = std::env::current_exe()
        && is_xcodex_exe_name(exe.as_os_str())
    {
        return true;
    }

    false
}

pub fn xcodex_first_run_wizard_marker_path(codex_home: &Path) -> PathBuf {
    codex_home.join(".xcodex-first-run-wizard.complete")
}

fn should_run_xcodex_first_run_wizard_impl(
    codex_home: &Path,
    is_xcodex: bool,
) -> std::io::Result<bool> {
    if !is_xcodex {
        return Ok(false);
    }

    let config_toml = codex_home.join(CONFIG_TOML_FILE);
    if config_toml.exists() {
        return Ok(false);
    }

    Ok(!xcodex_first_run_wizard_marker_path(codex_home).exists())
}

pub fn should_run_xcodex_first_run_wizard(codex_home: &Path) -> std::io::Result<bool> {
    should_run_xcodex_first_run_wizard_impl(codex_home, is_xcodex_invocation())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn default_home_dirname_switches_for_xcodex_invocation() {
        assert_eq!(XCODEX_DEFAULT_HOME_DIRNAME, xcodex_default_home_dirname());
    }

    #[test]
    fn xcodex_exe_name_matches_prefixed_names() {
        assert_eq!(true, is_xcodex_exe_name(OsStr::new("xcodex")));
        assert_eq!(
            true,
            is_xcodex_exe_name(OsStr::new("xcodex-x86_64-unknown-linux-musl"))
        );
        assert_eq!(false, is_xcodex_exe_name(OsStr::new("codex")));
    }

    #[test]
    fn xcodex_first_run_wizard_requires_missing_config_and_marker() -> std::io::Result<()> {
        let dir = tempdir()?;
        let codex_home = dir.path();

        assert_eq!(
            false,
            should_run_xcodex_first_run_wizard_impl(codex_home, false)?
        );
        assert_eq!(
            true,
            should_run_xcodex_first_run_wizard_impl(codex_home, true)?
        );

        std::fs::write(codex_home.join(CONFIG_TOML_FILE), "")?;
        assert_eq!(
            false,
            should_run_xcodex_first_run_wizard_impl(codex_home, true)?
        );

        std::fs::remove_file(codex_home.join(CONFIG_TOML_FILE))?;
        std::fs::write(xcodex_first_run_wizard_marker_path(codex_home), "")?;
        assert_eq!(
            false,
            should_run_xcodex_first_run_wizard_impl(codex_home, true)?
        );

        Ok(())
    }
}
