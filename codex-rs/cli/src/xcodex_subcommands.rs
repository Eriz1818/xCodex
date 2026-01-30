use clap::Parser;
use codex_common::CliConfigOverrides;
use codex_core::config::find_codex_home;
use codex_core::config::load_config_as_toml_with_cli_overrides;
use codex_tui::AppExitInfo;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use crate::config_cmd::ConfigCli;
use crate::mcp_cmd::McpCli;
use crate::prepend_config_flags;
use crate::xcodex_entrypoints;

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
pub(crate) struct HooksCommand {
    #[command(subcommand)]
    sub: HooksSubcommand,
}

#[derive(Debug, clap::Subcommand)]
enum HooksSubcommand {
    /// Guided entrypoint for hooks (interactive menu by default).
    Init(HooksInitCommand),

    /// Install hook SDK helpers/templates or runnable sample scripts.
    Install(HooksInstallCommand),

    /// Print diagnostics and next steps for a hook mode.
    Doctor(HooksDoctorCommand),

    /// Run smoke checks for a hook mode.
    Test(HooksTestCommand),

    /// Build/install a hook-mode-specific binary (advanced).
    Build(HooksBuildCommand),

    /// Print a short overview of hooks commands and SDK install options.
    Help(HooksHelpCommand),

    /// List configured hooks from the active config.
    List(HooksListCommand),

    /// Print where hook logs and payload files are written under CODEX_HOME.
    Paths(HooksPathsCommand),

    /// Legacy (will be removed): use `xcodex hooks doctor pyo3` / `xcodex hooks build pyo3`.
    #[clap(hide = true)]
    Pyo3(HooksPyo3Command),
}

#[derive(Debug, Parser)]
struct HooksInitCommand {
    /// Hook mode to initialize. When omitted, shows an interactive menu.
    #[arg(value_name = "MODE")]
    mode: Option<String>,

    /// Print planned changes and exit without writing.
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,

    /// Overwrite any existing files under CODEX_HOME/hooks.
    #[arg(long = "force", default_value_t = false)]
    force: bool,

    /// Skip interactive confirmation prompts.
    #[arg(long = "yes", default_value_t = false)]
    yes: bool,

    /// Don't print a config snippet to paste into config.toml.
    #[arg(long = "no-print-config", default_value_t = false)]
    no_print_config: bool,

    /// Edit CODEX_HOME/config.toml directly (best-effort) instead of only printing a snippet.
    #[arg(long = "edit-config", default_value_t = false)]
    edit_config: bool,
}

#[derive(Debug, Parser)]
struct HooksHelpCommand {}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct HooksInstallCommand {
    #[command(subcommand)]
    sub: Option<HooksInstallSubcommand>,

    /// Legacy: use `xcodex hooks install sdks list`.
    #[arg(long = "list", default_value_t = false, hide = true)]
    legacy_list: bool,

    /// Legacy: use `xcodex hooks install sdks all`.
    #[arg(long = "all", default_value_t = false, hide = true)]
    legacy_all: bool,

    /// Legacy: use `--force` on the new command.
    #[arg(long = "force", default_value_t = false, hide = true)]
    legacy_force: bool,
}

#[derive(Debug, clap::Subcommand)]
enum HooksInstallSubcommand {
    /// Install typed hook SDK helpers/templates under CODEX_HOME/hooks.
    Sdks(HooksInstallSdksCommand),

    /// Install runnable, out-of-the-box hook samples under CODEX_HOME/hooks.
    Samples(HooksInstallSamplesCommand),

    #[command(external_subcommand)]
    Legacy(Vec<String>),
}

#[derive(Debug, Parser)]
struct HooksInstallSdksCommand {
    /// Which SDK to install (or `list` / `all`).
    #[arg(value_name = "SDK")]
    sdk: Option<String>,

    /// Print planned changes and exit without writing.
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,

    /// Overwrite existing SDK files under CODEX_HOME/hooks.
    #[arg(long = "force", default_value_t = false)]
    force: bool,

    /// Skip interactive confirmation prompts.
    #[arg(long = "yes", default_value_t = false)]
    yes: bool,
}

#[derive(Debug, Parser)]
struct HooksInstallSamplesCommand {
    /// Which sample set to install (or `list` / `all`).
    #[arg(value_name = "SAMPLE")]
    sample: Option<String>,

    /// Print planned changes and exit without writing.
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,

    /// Overwrite existing sample files under CODEX_HOME/hooks.
    #[arg(long = "force", default_value_t = false)]
    force: bool,

    /// Skip interactive confirmation prompts.
    #[arg(long = "yes", default_value_t = false)]
    yes: bool,
}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct HooksDoctorCommand {
    #[command(subcommand)]
    sub: Option<HooksDoctorSubcommand>,
}

#[derive(Debug, clap::Subcommand)]
enum HooksDoctorSubcommand {
    External(HooksDoctorExternalCommand),
    #[clap(name = "python-host")]
    PythonHost(HooksDoctorPythonHostCommand),
    Pyo3(HooksPyo3DoctorCommand),

    #[command(external_subcommand)]
    Legacy(Vec<String>),
}

#[derive(Debug, Parser)]
struct HooksDoctorExternalCommand {}

#[derive(Debug, Parser)]
struct HooksDoctorPythonHostCommand {}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct HooksBuildCommand {
    #[command(subcommand)]
    sub: Option<HooksBuildSubcommand>,
}

#[derive(Debug, clap::Subcommand)]
enum HooksBuildSubcommand {
    Pyo3(HooksPyo3BootstrapCommand),

    #[command(external_subcommand)]
    Legacy(Vec<String>),
}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct HooksPyo3Command {
    #[command(subcommand)]
    sub: HooksPyo3Subcommand,
}

#[derive(Debug, clap::Subcommand)]
enum HooksPyo3Subcommand {
    /// Print prerequisite checks and the planned build actions.
    Doctor(HooksPyo3DoctorCommand),

    /// Clone + build + install a PyO3-enabled `xcodex-pyo3` binary.
    Bootstrap(HooksPyo3BootstrapCommand),
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum HooksPyo3Profile {
    #[value(name = "release")]
    Release,
    #[value(name = "debug")]
    Debug,
}

#[derive(Debug, Parser)]
struct HooksPyo3DoctorCommand {
    /// Python executable to embed/link (sets PYO3_PYTHON for the build).
    #[arg(long = "python", value_name = "PATH")]
    python: Option<PathBuf>,

    /// Directory to clone the repo into (default: CODEX_HOME/src/xcodex).
    #[arg(long = "repo-dir", value_name = "PATH")]
    repo_dir: Option<PathBuf>,

    /// Directory to install the resulting binary into (default: CODEX_HOME/bin).
    #[arg(long = "install-dir", value_name = "PATH")]
    install_dir: Option<PathBuf>,

    /// Name of the installed binary (default: xcodex-pyo3).
    #[arg(long = "bin-name", default_value = "xcodex-pyo3")]
    bin_name: String,
}

#[derive(Debug, Parser)]
struct HooksPyo3BootstrapCommand {
    /// Git URL to clone (default: https://github.com/Eriz1818/xCodex.git).
    #[arg(long = "repo-url", value_name = "URL")]
    repo_url: Option<String>,

    /// Directory to clone the repo into (default: CODEX_HOME/src/xcodex).
    #[arg(long = "repo-dir", value_name = "PATH")]
    repo_dir: Option<PathBuf>,

    /// Git ref to checkout and build (commit hash, tag, or branch).
    ///
    /// If omitted, defaults to a pinned commit (use `--ref` to override).
    #[arg(long = "ref", value_name = "REF")]
    git_ref: Option<String>,

    /// Python executable to embed/link (sets PYO3_PYTHON for the build).
    #[arg(long = "python", value_name = "PATH")]
    python: Option<PathBuf>,

    /// Build profile (default: release).
    #[arg(long = "profile", value_enum, default_value_t = HooksPyo3Profile::Release)]
    profile: HooksPyo3Profile,

    /// Directory to install the resulting binary into (default: CODEX_HOME/bin).
    #[arg(long = "install-dir", value_name = "PATH")]
    install_dir: Option<PathBuf>,

    /// Name of the installed binary (default: xcodex-pyo3).
    #[arg(long = "bin-name", default_value = "xcodex-pyo3")]
    bin_name: String,

    /// Path to write a report file to on failure (default: CODEX_HOME/tmp/pyo3-bootstrap-report.txt).
    #[arg(long = "report-path", value_name = "PATH")]
    report_path: Option<PathBuf>,

    /// Skip interactive confirmation prompts.
    #[arg(long = "yes", default_value_t = false)]
    yes: bool,
}

#[derive(Debug, Parser)]
struct HooksListCommand {
    /// Show all hook event keys, even if no commands are configured for them.
    #[arg(long = "all", default_value_t = false)]
    all: bool,
}

#[derive(Debug, Parser)]
struct HooksPathsCommand {}

#[derive(Debug, Parser)]
#[command(disable_help_subcommand = true)]
struct HooksTestCommand {
    #[command(subcommand)]
    sub: Option<HooksTestSubcommand>,
}

#[derive(Debug, clap::Subcommand)]
enum HooksTestSubcommand {
    External(HooksTestExternalCommand),
    #[clap(name = "python-host")]
    PythonHost(HooksTestPythonHostCommand),
    Pyo3(HooksTestPyo3Command),
    All(HooksTestAllCommand),

    #[command(external_subcommand)]
    Legacy(Vec<String>),
}

#[derive(Debug, Parser)]
struct HooksTestExternalCommand {
    /// Which hook events to test. If omitted, tests all events.
    #[arg(long = "event", value_enum)]
    events: Vec<HooksTestEventCli>,

    /// Test only events that have hook commands configured.
    #[arg(long = "configured-only", default_value_t = false)]
    configured_only: bool,

    /// Per-hook timeout.
    #[arg(long = "timeout-seconds", default_value_t = 10)]
    timeout_seconds: u64,
}

#[derive(Debug, Parser)]
struct HooksTestPythonHostCommand {
    /// Per-host timeout.
    #[arg(long = "timeout-seconds", default_value_t = 10)]
    timeout_seconds: u64,

    /// Only run when hooks.host.enabled=true and a command is configured.
    #[arg(long = "configured-only", default_value_t = false)]
    configured_only: bool,
}

#[derive(Debug, Parser)]
struct HooksTestPyo3Command {
    /// Only run when pyo3 hooks are configured.
    #[arg(long = "configured-only", default_value_t = false)]
    configured_only: bool,
}

#[derive(Debug, Parser)]
struct HooksTestAllCommand {
    /// Per-test timeout.
    #[arg(long = "timeout-seconds", default_value_t = 10)]
    timeout_seconds: u64,

    /// Only run checks that are configured/enabled.
    #[arg(long = "configured-only", default_value_t = true)]
    configured_only: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum HooksTestEventCli {
    AgentTurnComplete,
    ApprovalRequestedExec,
    ApprovalRequestedApplyPatch,
    ApprovalRequestedElicitation,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreCompact,
    Notification,
    SubagentStop,
    ModelRequestStarted,
    ModelResponseCompleted,
    ToolCallStarted,
    ToolCallFinished,
}

pub(crate) async fn run_hooks_command(
    root_config_overrides: &CliConfigOverrides,
    cmd: HooksCommand,
) -> anyhow::Result<()> {
    match cmd.sub {
        HooksSubcommand::Init(args) => {
            let codex_home = find_codex_home()?;
            run_hooks_init(&codex_home, args)?;
        }
        HooksSubcommand::Install(args) => {
            let codex_home = find_codex_home()?;
            run_hooks_install(&codex_home, args)?;
        }
        HooksSubcommand::Doctor(cmd) => {
            let codex_home = find_codex_home()?;
            match cmd.sub {
                None => {
                    println!("Usage: xcodex hooks doctor <external|python-host|pyo3>");
                    print_hooks_init_menu();
                }
                Some(HooksDoctorSubcommand::Legacy(_args)) => {
                    println!("Usage: xcodex hooks doctor <external|python-host|pyo3>");
                }
                Some(HooksDoctorSubcommand::External(_args)) => {
                    let config_cwd = AbsolutePathBuf::current_dir()?;
                    let cli_overrides = root_config_overrides
                        .parse_overrides()
                        .map_err(|e| anyhow::anyhow!(e))?;
                    let config_toml = load_config_as_toml_with_cli_overrides(
                        &codex_home,
                        &config_cwd,
                        cli_overrides,
                    )
                    .await?;
                    println!("External hooks (spawn per event):");
                    println!("- Config: {}", codex_home.join("config.toml").display());
                    if config_toml.hooks.agent_turn_complete.is_empty()
                        && config_toml.hooks.approval_requested.is_empty()
                        && config_toml.hooks.session_start.is_empty()
                        && config_toml.hooks.session_end.is_empty()
                        && config_toml.hooks.model_request_started.is_empty()
                        && config_toml.hooks.model_response_completed.is_empty()
                        && config_toml.hooks.tool_call_started.is_empty()
                        && config_toml.hooks.tool_call_finished.is_empty()
                    {
                        println!("- Status: not configured");
                        println!();
                        println!("Try:");
                        println!("- xcodex hooks init external");
                        println!("- xcodex hooks install samples external");
                    } else {
                        println!("- Status: configured");
                        println!("Try:");
                        println!("- xcodex hooks test external --configured-only");
                    }
                }
                Some(HooksDoctorSubcommand::PythonHost(_args)) => {
                    let config_cwd = AbsolutePathBuf::current_dir()?;
                    let cli_overrides = root_config_overrides
                        .parse_overrides()
                        .map_err(|e| anyhow::anyhow!(e))?;
                    let config_toml = load_config_as_toml_with_cli_overrides(
                        &codex_home,
                        &config_cwd,
                        cli_overrides,
                    )
                    .await?;
                    println!("Python Host hooks (long-lived):");
                    println!("- Config: {}", codex_home.join("config.toml").display());
                    if !config_toml.hooks.host.enabled || config_toml.hooks.host.command.is_empty()
                    {
                        println!("- Status: not configured");
                        println!();
                        println!("Try:");
                        println!("- xcodex hooks init python-host");
                        println!("- xcodex hooks install samples python-host");
                    } else {
                        println!("- Status: enabled");
                        println!("- hooks.host.command={:?}", config_toml.hooks.host.command);
                        println!("Try:");
                        println!("- xcodex hooks test python-host --configured-only");
                    }
                }
                Some(HooksDoctorSubcommand::Pyo3(args)) => {
                    run_hooks_pyo3_doctor(&codex_home, args)?;
                }
            }
        }
        HooksSubcommand::Build(cmd) => {
            let codex_home = find_codex_home()?;
            match cmd.sub {
                None => {
                    println!("Usage: xcodex hooks build pyo3");
                }
                Some(HooksBuildSubcommand::Legacy(_args)) => {
                    println!("Usage: xcodex hooks build pyo3");
                }
                Some(HooksBuildSubcommand::Pyo3(args)) => {
                    run_hooks_pyo3_bootstrap(&codex_home, args)?;
                }
            }
        }
        HooksSubcommand::Pyo3(cmd) => {
            println!("This command has moved.");
            match cmd.sub {
                HooksPyo3Subcommand::Doctor(_args) => {
                    println!("Use: xcodex hooks doctor pyo3");
                }
                HooksPyo3Subcommand::Bootstrap(_args) => {
                    println!("Use: xcodex hooks build pyo3");
                }
            }
        }
        HooksSubcommand::Help(_args) => {
            run_hooks_help()?;
        }
        HooksSubcommand::List(args) => {
            let codex_home = find_codex_home()?;
            let config_cwd = AbsolutePathBuf::current_dir()?;
            let cli_overrides = root_config_overrides
                .parse_overrides()
                .map_err(|e| anyhow::anyhow!(e))?;
            let config_toml =
                load_config_as_toml_with_cli_overrides(&codex_home, &config_cwd, cli_overrides)
                    .await?;
            print_hooks_list(&codex_home, &config_toml.hooks, args.all);
        }
        HooksSubcommand::Paths(_args) => {
            let codex_home = find_codex_home()?;
            let config_cwd = AbsolutePathBuf::current_dir()?;
            let cli_overrides = root_config_overrides
                .parse_overrides()
                .map_err(|e| anyhow::anyhow!(e))?;
            let config_toml =
                load_config_as_toml_with_cli_overrides(&codex_home, &config_cwd, cli_overrides)
                    .await?;
            print_hooks_paths(&codex_home, &config_toml.hooks);
        }
        HooksSubcommand::Test(cmd) => {
            let codex_home = find_codex_home()?;
            let resolved_cwd = AbsolutePathBuf::current_dir()?;
            let cli_overrides = root_config_overrides
                .parse_overrides()
                .map_err(|e| anyhow::anyhow!(e))?;
            let config_toml =
                load_config_as_toml_with_cli_overrides(&codex_home, &resolved_cwd, cli_overrides)
                    .await?;

            let Some(sub) = cmd.sub else {
                println!("Usage: xcodex hooks test <external|python-host|pyo3|all>");
                return Ok(());
            };

            match sub {
                HooksTestSubcommand::Legacy(_args) => {
                    println!("Usage: xcodex hooks test <external|python-host|pyo3|all>");
                }
                HooksTestSubcommand::External(args) => {
                    let target = if args.configured_only {
                        codex_core::hooks_test::HooksTestTarget::Configured
                    } else {
                        codex_core::hooks_test::HooksTestTarget::All
                    };
                    let events = args
                        .events
                        .into_iter()
                        .map(|event| match event {
                            HooksTestEventCli::AgentTurnComplete => {
                                codex_core::hooks_test::HooksTestEvent::AgentTurnComplete
                            }
                            HooksTestEventCli::ApprovalRequestedExec => {
                                codex_core::hooks_test::HooksTestEvent::ApprovalRequestedExec
                            }
                            HooksTestEventCli::ApprovalRequestedApplyPatch => {
                                codex_core::hooks_test::HooksTestEvent::ApprovalRequestedApplyPatch
                            }
                            HooksTestEventCli::ApprovalRequestedElicitation => {
                                codex_core::hooks_test::HooksTestEvent::ApprovalRequestedElicitation
                            }
                            HooksTestEventCli::SessionStart => {
                                codex_core::hooks_test::HooksTestEvent::SessionStart
                            }
                            HooksTestEventCli::SessionEnd => {
                                codex_core::hooks_test::HooksTestEvent::SessionEnd
                            }
                            HooksTestEventCli::UserPromptSubmit => {
                                codex_core::hooks_test::HooksTestEvent::UserPromptSubmit
                            }
                            HooksTestEventCli::PreCompact => {
                                codex_core::hooks_test::HooksTestEvent::PreCompact
                            }
                            HooksTestEventCli::Notification => {
                                codex_core::hooks_test::HooksTestEvent::Notification
                            }
                            HooksTestEventCli::SubagentStop => {
                                codex_core::hooks_test::HooksTestEvent::SubagentStop
                            }
                            HooksTestEventCli::ModelRequestStarted => {
                                codex_core::hooks_test::HooksTestEvent::ModelRequestStarted
                            }
                            HooksTestEventCli::ModelResponseCompleted => {
                                codex_core::hooks_test::HooksTestEvent::ModelResponseCompleted
                            }
                            HooksTestEventCli::ToolCallStarted => {
                                codex_core::hooks_test::HooksTestEvent::ToolCallStarted
                            }
                            HooksTestEventCli::ToolCallFinished => {
                                codex_core::hooks_test::HooksTestEvent::ToolCallFinished
                            }
                        })
                        .collect();

                    let report = codex_core::hooks_test::run_hooks_test(
                        codex_home.clone(),
                        config_toml.hooks.clone(),
                        target,
                        events,
                        Duration::from_secs(args.timeout_seconds),
                    )
                    .await?;

                    let total = report.invocations.len();
                    println!("Invoked {total} hook command(s).");
                    println!("Logs: {}", report.logs_dir.display());
                    println!("Payloads: {}", report.payloads_dir.display());
                    for inv in report.invocations {
                        let cmd = inv.command.join(" ");
                        let exit = inv
                            .exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "timeout/error".to_string());
                        println!("- {} exit={exit}: {cmd}", inv.event_type);
                    }
                }
                HooksTestSubcommand::PythonHost(args) => {
                    let host = &config_toml.hooks.host;
                    if args.configured_only && (!host.enabled || host.command.is_empty()) {
                        println!("hooks.host is not enabled; skipping (configured-only).");
                        return Ok(());
                    }
                    if !host.enabled || host.command.is_empty() {
                        anyhow::bail!(
                            "hooks.host is not configured; try: xcodex hooks init python-host"
                        );
                    }

                    let program = host
                        .command
                        .first()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("hooks.host.command is empty"))?;
                    let argsv = host.command.iter().skip(1).cloned().collect::<Vec<_>>();

                    let mut child = std::process::Command::new(&program)
                        .args(&argsv)
                        .current_dir(&codex_home)
                        .env("CODEX_HOME", &codex_home)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::inherit())
                        .spawn()?;

                    let mut stdin = child
                        .stdin
                        .take()
                        .ok_or_else(|| anyhow::anyhow!("failed to open stdin"))?;
                    let msg = serde_json::json!({
                        "schema_version": 1,
                        "type": "hook-event",
                        "seq": 1,
                        "event": {
                            "session_id": "t",
                            "transcript_path": "",
                            "permission_mode": "default",
                            "hook_event_name": "PostToolUse",
                            "event_id": "hooks-test",
                            "timestamp": "1970-01-01T00:00:00Z",
                            "turn_id": "u",
                            "cwd": codex_home.display().to_string(),
                            "tool_name": "Bash",
                            "tool_use_id": "c",
                            "tool_response": null,
                            "schema_version": 1,
                            "xcodex_event_type": "tool-call-finished",
                            "duration_ms": 1,
                            "success": true,
                            "status": "completed",
                            "output_bytes": 0
                        }
                    });
                    use std::io::Write;
                    writeln!(stdin, "{msg}")?;
                    // Close the write end so the host sees EOF and exits cleanly.
                    drop(stdin);

                    let status = tokio::time::timeout(
                        Duration::from_secs(args.timeout_seconds),
                        tokio::task::spawn_blocking(move || child.wait()),
                    )
                    .await
                    .ok()
                    .and_then(std::result::Result::ok)
                    .and_then(std::result::Result::ok);

                    match status {
                        Some(status) if status.success() => {
                            println!("Host exited successfully.")
                        }
                        Some(status) => anyhow::bail!("host exited with {status:?}"),
                        None => anyhow::bail!("host timed out"),
                    }
                }
                HooksTestSubcommand::Pyo3(args) => {
                    let hooks = &config_toml.hooks;
                    let enabled =
                        hooks.enable_unsafe_inproc && hooks.inproc.iter().any(|h| h == "pyo3");
                    if args.configured_only && !enabled {
                        println!("pyo3 hooks are not enabled; skipping (configured-only).");
                        return Ok(());
                    }
                    if !enabled {
                        anyhow::bail!("pyo3 hooks are not enabled; try: xcodex hooks doctor pyo3");
                    }
                    println!("pyo3 hooks appear enabled in config.");
                    println!("Next: xcodex hooks doctor pyo3");
                }
                HooksTestSubcommand::All(args) => {
                    let external_args = HooksTestExternalCommand {
                        events: Vec::new(),
                        configured_only: args.configured_only,
                        timeout_seconds: args.timeout_seconds,
                    };
                    println!("== external ==");
                    {
                        let target = if external_args.configured_only {
                            codex_core::hooks_test::HooksTestTarget::Configured
                        } else {
                            codex_core::hooks_test::HooksTestTarget::All
                        };
                        let events = Vec::new();
                        let report = codex_core::hooks_test::run_hooks_test(
                            codex_home.clone(),
                            config_toml.hooks.clone(),
                            target,
                            events,
                            Duration::from_secs(external_args.timeout_seconds),
                        )
                        .await?;

                        let total = report.invocations.len();
                        println!("Invoked {total} hook command(s).");
                        println!("Logs: {}", report.logs_dir.display());
                        println!("Payloads: {}", report.payloads_dir.display());
                    }

                    println!();
                    println!("== python-host ==");
                    {
                        let host_args = HooksTestPythonHostCommand {
                            timeout_seconds: args.timeout_seconds,
                            configured_only: args.configured_only,
                        };
                        let host = &config_toml.hooks.host;
                        if host_args.configured_only && (!host.enabled || host.command.is_empty()) {
                            println!("hooks.host is not enabled; skipping (configured-only).");
                        } else if !host.enabled || host.command.is_empty() {
                            println!("hooks.host is not configured; skipping.");
                        } else {
                            let program =
                                host.command.first().cloned().ok_or_else(|| {
                                    anyhow::anyhow!("hooks.host.command is empty")
                                })?;
                            let argsv = host.command.iter().skip(1).cloned().collect::<Vec<_>>();

                            let mut child = std::process::Command::new(&program)
                                .args(&argsv)
                                .current_dir(&codex_home)
                                .env("CODEX_HOME", &codex_home)
                                .stdin(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::inherit())
                                .spawn()?;

                            let mut stdin = child
                                .stdin
                                .take()
                                .ok_or_else(|| anyhow::anyhow!("failed to open stdin"))?;
                            use std::io::Write;
                            writeln!(
                                stdin,
                                "{}",
                                serde_json::json!({
                                    "schema_version": 1,
                                    "type": "hook-event",
                                    "seq": 1,
                                    "event": {
                                        "session_id": "t",
                                        "transcript_path": "",
                                        "permission_mode": "default",
                                        "hook_event_name": "PostToolUse",
                                        "event_id": "hooks-test",
                                        "timestamp": "1970-01-01T00:00:00Z",
                                        "turn_id": "u",
                                        "cwd": codex_home.display().to_string(),
                                        "tool_name": "Bash",
                                        "tool_use_id": "c",
                                        "tool_response": null,
                                        "schema_version": 1,
                                        "xcodex_event_type": "tool-call-finished",
                                        "status": "completed",
                                        "duration_ms": 1,
                                        "success": true,
                                        "output_bytes": 0
                                    }
                                })
                            )?;
                            // Close the write end so the host sees EOF and exits cleanly.
                            drop(stdin);

                            let status = tokio::time::timeout(
                                Duration::from_secs(host_args.timeout_seconds),
                                tokio::task::spawn_blocking(move || child.wait()),
                            )
                            .await
                            .ok()
                            .and_then(std::result::Result::ok)
                            .and_then(std::result::Result::ok);

                            match status {
                                Some(status) if status.success() => {
                                    println!("Host exited successfully.")
                                }
                                Some(status) => anyhow::bail!("host exited with {status:?}"),
                                None => anyhow::bail!("host timed out"),
                            }
                        }
                    }

                    println!();
                    println!("== pyo3 ==");
                    {
                        let hooks = &config_toml.hooks;
                        let enabled =
                            hooks.enable_unsafe_inproc && hooks.inproc.iter().any(|h| h == "pyo3");
                        if args.configured_only && !enabled {
                            println!("pyo3 hooks are not enabled; skipping (configured-only).");
                        } else if enabled {
                            println!("pyo3 hooks appear enabled in config.");
                            println!("Next: xcodex hooks doctor pyo3");
                        } else {
                            println!("pyo3 hooks are not enabled; skipping.");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub(crate) async fn run_mcp_command(
    root_config_overrides: &CliConfigOverrides,
    mut mcp_cli: McpCli,
) -> anyhow::Result<()> {
    // Propagate any root-level config overrides (e.g. `-c key=value`).
    prepend_config_flags(&mut mcp_cli.config_overrides, root_config_overrides.clone());
    mcp_cli.run().await
}

pub(crate) async fn run_config_command(
    root_config_overrides: &CliConfigOverrides,
    mut config_cli: ConfigCli,
) -> anyhow::Result<()> {
    prepend_config_flags(
        &mut config_cli.config_overrides,
        root_config_overrides.clone(),
    );
    config_cli.run().await
}

pub(crate) async fn run_tui2_command(
    root_config_overrides: &CliConfigOverrides,
    mut tui2_cli: codex_tui::Cli,
    codex_linux_sandbox_exe: Option<PathBuf>,
) -> anyhow::Result<AppExitInfo> {
    prepend_config_flags(
        &mut tui2_cli.config_overrides,
        root_config_overrides.clone(),
    );
    xcodex_entrypoints::run_tui2(tui2_cli, codex_linux_sandbox_exe).await
}

fn hooks_logs_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("tmp").join("hooks").join("logs")
}

fn hooks_payloads_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("tmp").join("hooks").join("payloads")
}

fn run_hooks_help() -> anyhow::Result<()> {
    println!("Hooks commands:");
    println!("- xcodex hooks init");
    println!("- xcodex hooks install sdks list");
    println!("- xcodex hooks install sdks <sdk|all> [--dry-run] [--force] [--yes]");
    println!("- xcodex hooks install samples list");
    println!(
        "- xcodex hooks install samples <external|python-host|pyo3|all> [--dry-run] [--force] [--yes]"
    );
    println!("- xcodex hooks doctor <external|python-host|pyo3>");
    println!("- xcodex hooks test <external|python-host|pyo3|all>");
    println!("- xcodex hooks build pyo3");
    println!("- xcodex hooks list");
    println!("- xcodex hooks paths");
    println!();
    println!("Supported SDKs:");
    for sdk in codex_common::hooks_sdk_install::all_hook_sdks() {
        println!("- {}: {}", sdk.id(), sdk.description());
    }
    println!();
    println!("Supported sample sets:");
    for sample in [
        codex_common::hooks_samples_install::HookSample::External,
        codex_common::hooks_samples_install::HookSample::PythonHost,
        codex_common::hooks_samples_install::HookSample::Pyo3,
    ] {
        println!("- {}: {}", sample.id(), sample.description());
    }
    println!();
    println!("Docs:");
    println!("- docs/xcodex/hooks.md");
    println!("- docs/xcodex/hooks-sdks.md");
    println!("- docs/xcodex/hooks-python-host.md");
    println!("- docs/xcodex/hooks-pyo3.md");
    println!("- docs/config.md#hooks");
    Ok(())
}

fn pyo3_bootstrap_default_repo_url() -> &'static str {
    "https://github.com/Eriz1818/xCodex.git"
}

fn pyo3_bootstrap_default_git_ref() -> &'static str {
    "31aadee0612bd56d81e22b3973fbdd44d4b5729f"
}

fn pyo3_bootstrap_issues_url() -> &'static str {
    "https://github.com/Eriz1818/xCodex/issues/new"
}

fn pyo3_bootstrap_default_report_path(codex_home: &Path) -> PathBuf {
    codex_home.join("tmp").join("pyo3-bootstrap-report.txt")
}

fn is_interactive_stdin() -> bool {
    std::io::stdin().is_terminal()
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    use std::io::Write;

    print!("{prompt}");
    std::io::stdout().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn prompt_confirm(prompt: &str, default: bool) -> anyhow::Result<bool> {
    let suffix = if default { " [Y/n]" } else { " [y/N]" };
    let line = prompt_line(&format!("{prompt}{suffix} "))?;
    if line.is_empty() {
        return Ok(default);
    }

    match line.to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        other => anyhow::bail!("invalid response: {other}"),
    }
}

fn run_command_capture(mut cmd: std::process::Command) -> anyhow::Result<std::process::Output> {
    let printed = format_command(&cmd);
    cmd.output()
        .map_err(|err| anyhow::anyhow!("failed to run command: {printed}: {err}"))
}

fn run_command_capture_with_echo(
    mut cmd: std::process::Command,
    echo: bool,
) -> anyhow::Result<std::process::Output> {
    use std::io::Write;
    use std::process::Stdio;

    let printed = format_command(&cmd);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| anyhow::anyhow!("failed to run command: {printed}: {err}"))?;

    let Some(stdout) = child.stdout.take() else {
        anyhow::bail!("failed to capture stdout for command: {printed}");
    };
    let Some(stderr) = child.stderr.take() else {
        anyhow::bail!("failed to capture stderr for command: {printed}");
    };

    let (tx, rx) = std::sync::mpsc::channel::<(bool, Vec<u8>)>();

    let stdout_tx = tx.clone();
    let stdout_handle = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut buf = vec![0_u8; 8 * 1024];
        loop {
            let Ok(n) = std::io::Read::read(&mut reader, &mut buf) else {
                break;
            };
            if n == 0 {
                break;
            }
            if stdout_tx.send((false, buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });

    let stderr_tx = tx.clone();
    let stderr_handle = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut buf = vec![0_u8; 8 * 1024];
        loop {
            let Ok(n) = std::io::Read::read(&mut reader, &mut buf) else {
                break;
            };
            if n == 0 {
                break;
            }
            if stderr_tx.send((true, buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });

    drop(tx);

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    while let Ok((is_stderr, bytes)) = rx.recv() {
        if is_stderr {
            stderr_buf.extend_from_slice(&bytes);
            if echo {
                let _ = std::io::stderr().write_all(&bytes);
                let _ = std::io::stderr().flush();
            }
        } else {
            stdout_buf.extend_from_slice(&bytes);
            if echo {
                let _ = std::io::stdout().write_all(&bytes);
                let _ = std::io::stdout().flush();
            }
        }
    }

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let status = child
        .wait()
        .map_err(|err| anyhow::anyhow!("failed to run command: {printed}: {err}"))?;

    Ok(std::process::Output {
        status,
        stdout: stdout_buf,
        stderr: stderr_buf,
    })
}

fn format_command(cmd: &std::process::Command) -> String {
    let mut parts = Vec::new();
    parts.push(cmd.get_program().to_string_lossy().to_string());
    for arg in cmd.get_args() {
        parts.push(arg.to_string_lossy().to_string());
    }
    parts.join(" ")
}

fn write_pyo3_bootstrap_report(report_path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(report_path, contents)?;
    Ok(())
}

fn apply_pyo3_bootstrap_patches(
    codex_rs_dir: &Path,
    transcript: &mut String,
) -> anyhow::Result<()> {
    use toml_edit::DocumentMut;
    use toml_edit::InlineTable;
    use toml_edit::Item;
    use toml_edit::Table;
    use toml_edit::Value;

    let cli_cargo_toml_path = codex_rs_dir.join("cli").join("Cargo.toml");
    let cli_cargo_toml = std::fs::read_to_string(&cli_cargo_toml_path).map_err(|err| {
        anyhow::anyhow!("failed to read {}: {err}", cli_cargo_toml_path.display())
    })?;

    let mut doc = cli_cargo_toml.parse::<DocumentMut>().map_err(|err| {
        anyhow::anyhow!("failed to parse {}: {err}", cli_cargo_toml_path.display())
    })?;

    let root = doc.as_table_mut();
    if !root.contains_key("dependencies") {
        root["dependencies"] = Item::Table(Table::new());
    }

    let deps = root
        .get_mut("dependencies")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} has a non-table [dependencies]",
                cli_cargo_toml_path.display()
            )
        })?;

    let mut changed = false;
    for crate_name in ["ctor", "codex-process-hardening"] {
        if deps.contains_key(crate_name) {
            continue;
        }

        let mut tbl = InlineTable::new();
        tbl.insert("workspace", Value::from(true));
        deps[crate_name] = Item::Value(Value::InlineTable(tbl));
        changed = true;
    }

    if changed {
        std::fs::write(&cli_cargo_toml_path, doc.to_string()).map_err(|err| {
            anyhow::anyhow!("failed to write {}: {err}", cli_cargo_toml_path.display())
        })?;
        transcript.push_str(&format!(
            "applied_bootstrap_patch=added_missing_cli_deps path={}\n",
            cli_cargo_toml_path.display()
        ));
    } else {
        transcript.push_str("applied_bootstrap_patch=none\n");
    }

    Ok(())
}

fn resolve_default_pyo3_python(
    python_arg: Option<PathBuf>,
    interactive: bool,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = python_arg {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("PYO3_PYTHON").map(PathBuf::from) {
        return Ok(path);
    }

    let mut cmd = std::process::Command::new("python3");
    cmd.arg("--version");
    match run_command_capture(cmd) {
        Ok(output) if output.status.success() => Ok(PathBuf::from("python3")),
        Ok(_) | Err(_) => {
            if interactive {
                let line = prompt_line("Python path (for PYO3_PYTHON): ")?;
                if line.is_empty() {
                    anyhow::bail!("missing python; pass --python or set PYO3_PYTHON");
                }
                Ok(PathBuf::from(line))
            } else {
                anyhow::bail!("missing python; pass --python or set PYO3_PYTHON");
            }
        }
    }
}

fn run_hooks_pyo3_doctor(codex_home: &Path, args: HooksPyo3DoctorCommand) -> anyhow::Result<()> {
    let interactive = is_interactive_stdin();
    let repo_url = pyo3_bootstrap_default_repo_url();
    let repo_dir = args
        .repo_dir
        .unwrap_or_else(|| codex_home.join("src").join("xcodex"));
    let install_dir = args.install_dir.unwrap_or_else(|| codex_home.join("bin"));
    let dest_path = install_dir.join(&args.bin_name);

    println!("PyO3 doctor (local-only, advanced)");
    println!();
    println!("This checks basic prerequisites and prints what `xcodex hooks build pyo3` will do.");
    println!();

    let mut ok = true;
    for (label, program, args) in [
        ("git", "git", vec!["--version"]),
        ("cargo", "cargo", vec!["--version"]),
    ] {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        let printed = format_command(&cmd);
        match run_command_capture(cmd) {
            Ok(output) if output.status.success() => {
                println!(
                    "- {label}: ok ({})",
                    String::from_utf8_lossy(&output.stdout).trim()
                );
            }
            Ok(output) => {
                ok = false;
                println!("- {label}: failed ({printed})");
                print!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            Err(err) => {
                ok = false;
                println!("- {label}: failed ({printed})");
                eprintln!("{err:#}");
            }
        }
    }

    let python = match resolve_default_pyo3_python(args.python, interactive) {
        Ok(path) => path,
        Err(err) => {
            ok = false;
            eprintln!("- python: failed");
            eprintln!("{err:#}");
            PathBuf::from("<missing>")
        }
    };

    if python.as_os_str() != "<missing>" {
        let mut cmd = std::process::Command::new(&python);
        cmd.arg("--version");
        let printed = format_command(&cmd);
        match run_command_capture(cmd) {
            Ok(output) if output.status.success() => {
                println!(
                    "- python (PYO3_PYTHON): ok ({})",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(output) => {
                ok = false;
                println!("- python (PYO3_PYTHON): failed ({printed})");
                print!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            Err(err) => {
                ok = false;
                println!("- python (PYO3_PYTHON): failed ({printed})");
                eprintln!("{err:#}");
            }
        }
    }

    println!();
    println!("Planned build defaults:");
    println!("- Repo URL: {repo_url}");
    println!("- Repo dir: {}", repo_dir.display());
    println!("- Git ref: {}", pyo3_bootstrap_default_git_ref());
    println!("- Python (PYO3_PYTHON): {}", python.display());
    println!("- Profile: release");
    println!("- Install dir: {}", install_dir.display());
    println!("- Installed binary: {}", dest_path.display());
    println!(
        "- Failure report path: {}",
        pyo3_bootstrap_default_report_path(codex_home).display()
    );
    println!();
    println!("Command:");
    println!("  xcodex hooks build pyo3");
    println!();

    if ok {
        println!("Status: ok");
    } else {
        println!("Status: missing prerequisites; see errors above.");
    }

    Ok(())
}

fn pyo3_bootstrap_fail(report_path: &Path, transcript: &str, err: anyhow::Error) -> anyhow::Error {
    let _ = write_pyo3_bootstrap_report(report_path, &format!("{transcript}\nerror={err:#}\n"));
    eprintln!("PyO3 build failed.");
    eprintln!("Report written to: {}", report_path.display());
    eprintln!(
        "Please file an issue and attach the report: {}",
        pyo3_bootstrap_issues_url()
    );
    err
}

fn run_hooks_pyo3_bootstrap(
    codex_home: &Path,
    mut args: HooksPyo3BootstrapCommand,
) -> anyhow::Result<()> {
    let interactive = is_interactive_stdin();
    let repo_url = args
        .repo_url
        .take()
        .unwrap_or_else(|| pyo3_bootstrap_default_repo_url().to_string());

    let repo_dir = args
        .repo_dir
        .take()
        .unwrap_or_else(|| codex_home.join("src").join("xcodex"));

    let install_dir = args
        .install_dir
        .take()
        .unwrap_or_else(|| codex_home.join("bin"));
    let report_path = args
        .report_path
        .take()
        .unwrap_or_else(|| pyo3_bootstrap_default_report_path(codex_home));

    let python = resolve_default_pyo3_python(args.python.take(), interactive)?;

    let git_ref = args
        .git_ref
        .take()
        .unwrap_or_else(|| pyo3_bootstrap_default_git_ref().to_string());

    let profile = args.profile;
    let bin_name = args.bin_name;
    let dest_path = install_dir.join(&bin_name);

    println!("PyO3 build (local-only, advanced): builds a side-by-side binary.\n");
    println!("Plan:");
    println!("- Repo URL: {repo_url}");
    println!("- Repo dir: {}", repo_dir.display());
    println!("- Git ref: {git_ref}");
    println!("- Python (PYO3_PYTHON): {}", python.display());
    println!(
        "- Profile: {}",
        match profile {
            HooksPyo3Profile::Release => "release",
            HooksPyo3Profile::Debug => "debug",
        }
    );
    println!("- Install dir: {}", install_dir.display());
    println!("- Installed binary: {}", dest_path.display());
    println!("- Failure report path: {}", report_path.display());
    println!();
    println!("Uninstall: delete {}", dest_path.display());
    println!("(Optional) Cleanup: delete {}", repo_dir.display());
    println!();

    if !args.yes {
        if !interactive {
            anyhow::bail!("non-interactive mode requires --yes");
        }
        let proceed = prompt_confirm("Proceed with clone/build/install?", false)?;
        if !proceed {
            return Ok(());
        }
    }

    let mut transcript = String::new();
    transcript.push_str("xcodex hooks build pyo3 report\n");
    transcript.push_str(&format!("repo_url={repo_url}\n"));
    transcript.push_str(&format!("repo_dir={}\n", repo_dir.display()));
    transcript.push_str(&format!("git_ref={git_ref}\n"));
    transcript.push_str(&format!("python={}\n", python.display()));
    transcript.push_str(&format!(
        "profile={}\n",
        match profile {
            HooksPyo3Profile::Release => "release",
            HooksPyo3Profile::Debug => "debug",
        }
    ));
    transcript.push_str(&format!("install_dir={}\n", install_dir.display()));
    transcript.push_str(&format!("bin_name={bin_name}\n"));
    transcript.push_str(&format!("dest_path={}\n", dest_path.display()));
    transcript.push_str(&format!("issues_url={}\n", pyo3_bootstrap_issues_url()));
    transcript.push('\n');

    let reuse_repo_dir = repo_dir.exists();
    if reuse_repo_dir {
        if !repo_dir.join(".git").exists() {
            return Err(pyo3_bootstrap_fail(
                &report_path,
                &transcript,
                anyhow::anyhow!(
                    "repo dir already exists but does not look like a git repo: {}",
                    repo_dir.display()
                ),
            ));
        }

        println!(
            "Note: repo dir already exists; reusing: {}",
            repo_dir.display()
        );
        transcript.push_str(&format!("reuse_repo_dir={}\n\n", repo_dir.display()));
    }

    // 1) Prereqs (minimal).
    println!("Step 1/4: Checking prerequisites...");
    for (label, program, args) in [
        ("git", "git", vec!["--version"]),
        ("cargo", "cargo", vec!["--version"]),
    ] {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture(cmd) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    return Err(pyo3_bootstrap_fail(
                        &report_path,
                        &transcript,
                        anyhow::anyhow!("{label} prerequisite failed: {printed}"),
                    ));
                }
            }
            Err(err) => return Err(pyo3_bootstrap_fail(&report_path, &transcript, err)),
        }
        transcript.push('\n');
    }

    {
        let mut cmd = std::process::Command::new(&python);
        cmd.args(["--version"]);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture(cmd) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    return Err(pyo3_bootstrap_fail(
                        &report_path,
                        &transcript,
                        anyhow::anyhow!("python prerequisite failed: {printed}"),
                    ));
                }
            }
            Err(err) => return Err(pyo3_bootstrap_fail(&report_path, &transcript, err)),
        }
        transcript.push('\n');
    }

    // 2) Clone + checkout.
    println!();
    println!("Step 2/4: Cloning and checking out {git_ref}...");
    if !reuse_repo_dir {
        let mut cmd = std::process::Command::new("git");
        cmd.args([
            "clone",
            repo_url.as_str(),
            repo_dir
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("repo dir is not valid utf-8"))?,
        ]);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture_with_echo(cmd, interactive) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    return Err(pyo3_bootstrap_fail(
                        &report_path,
                        &transcript,
                        anyhow::anyhow!("git clone failed"),
                    ));
                }
            }
            Err(err) => return Err(pyo3_bootstrap_fail(&report_path, &transcript, err)),
        }
        transcript.push('\n');
    }

    if reuse_repo_dir {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&repo_dir)
            .args(["fetch", "--all", "--tags"]);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture_with_echo(cmd, interactive) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    eprintln!("Warning: git fetch failed; continuing with local refs.");
                }
            }
            Err(err) => {
                eprintln!("Warning: git fetch failed: {err:#}; continuing with local refs.");
            }
        }
        transcript.push('\n');
    }

    {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&repo_dir).args(["checkout", &git_ref]);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture_with_echo(cmd, interactive) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    return Err(pyo3_bootstrap_fail(
                        &report_path,
                        &transcript,
                        anyhow::anyhow!("git checkout failed"),
                    ));
                }
            }
            Err(err) => return Err(pyo3_bootstrap_fail(&report_path, &transcript, err)),
        }
        transcript.push('\n');
    }

    let resolved_commit = {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&repo_dir).args(["rev-parse", "HEAD"]);
        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        let output = run_command_capture(cmd)
            .map_err(|err| pyo3_bootstrap_fail(&report_path, &transcript, err))?;
        transcript.push_str(&String::from_utf8_lossy(&output.stdout));
        transcript.push_str(&String::from_utf8_lossy(&output.stderr));
        transcript.push('\n');
        if !output.status.success() {
            return Err(pyo3_bootstrap_fail(
                &report_path,
                &transcript,
                anyhow::anyhow!("failed to resolve checked out commit"),
            ));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // 3) Build.
    println!();
    println!("Step 3/4: Building {bin_name} (this may take a few minutes)...");
    let codex_rs_dir = repo_dir.join("codex-rs");
    if !codex_rs_dir.exists() {
        return Err(pyo3_bootstrap_fail(
            &report_path,
            &transcript,
            anyhow::anyhow!(
                "expected codex-rs directory under cloned repo: {}",
                codex_rs_dir.display()
            ),
        ));
    }

    println!("Preparing build (applying bootstrap patches if needed)...");
    apply_pyo3_bootstrap_patches(&codex_rs_dir, &mut transcript)
        .map_err(|err| pyo3_bootstrap_fail(&report_path, &transcript, err))?;
    transcript.push('\n');

    {
        let mut cmd = std::process::Command::new("cargo");
        cmd.current_dir(&codex_rs_dir)
            .env("PYO3_PYTHON", &python)
            .args(["build", "-p", "codex-cli", "--bin", "codex"])
            .args(match profile {
                HooksPyo3Profile::Release => vec!["--release"],
                HooksPyo3Profile::Debug => Vec::<&str>::new(),
            })
            .args(["--features", "codex-core/pyo3-hooks"]);

        let printed = format_command(&cmd);
        transcript.push_str(&format!("$ {printed}\n"));
        match run_command_capture_with_echo(cmd, interactive) {
            Ok(output) => {
                transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                if !output.status.success() {
                    return Err(pyo3_bootstrap_fail(
                        &report_path,
                        &transcript,
                        anyhow::anyhow!("cargo build failed"),
                    ));
                }
            }
            Err(err) => return Err(pyo3_bootstrap_fail(&report_path, &transcript, err)),
        }
        transcript.push('\n');
    }

    // 4) Install side-by-side binary.
    println!();
    println!("Step 4/4: Installing {bin_name}...");
    let built_bin = codex_rs_dir
        .join("target")
        .join(match profile {
            HooksPyo3Profile::Release => "release",
            HooksPyo3Profile::Debug => "debug",
        })
        .join("codex");

    if !built_bin.exists() {
        return Err(pyo3_bootstrap_fail(
            &report_path,
            &transcript,
            anyhow::anyhow!("expected built binary at {}", built_bin.display()),
        ));
    }

    std::fs::create_dir_all(&install_dir)
        .map_err(|err| pyo3_bootstrap_fail(&report_path, &transcript, err.into()))?;
    std::fs::copy(&built_bin, &dest_path)
        .map_err(|err| pyo3_bootstrap_fail(&report_path, &transcript, err.into()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| pyo3_bootstrap_fail(&report_path, &transcript, err.into()))?;
    }

    transcript.push_str(&format!("resolved_commit={resolved_commit}\n\n"));

    println!("Installed {bin_name} to: {}", dest_path.display());
    println!("Pinned commit: {resolved_commit}");
    println!("Try: {} --version", dest_path.display());
    println!("Try: {bin_name} --version");
    println!("Regular binary: xcodex");
    println!(
        "If you want to run it as `{bin_name}` from anywhere, add {} to PATH.",
        install_dir.display()
    );

    let _ = write_pyo3_bootstrap_report(&report_path, &format!("{transcript}\nsuccess=1\n"));
    Ok(())
}

fn run_hooks_init(codex_home: &Path, args: HooksInitCommand) -> anyhow::Result<()> {
    use codex_common::hooks_samples_install::HookSample;

    std::fs::create_dir_all(codex_home)?;

    let interactive = is_interactive_stdin();

    let selected = match args.mode.as_deref() {
        Some(raw) => parse_hook_sample(raw),
        None => {
            print_hooks_init_menu();
            if !interactive {
                println!();
                println!("Run one of:");
                println!("- xcodex hooks init external");
                println!("- xcodex hooks init python-host");
                println!("- xcodex hooks init pyo3");
                return Ok(());
            }

            println!();
            let choice = prompt_line("Select a hook mode (1-3): ")?;
            if choice.trim().is_empty() {
                return Ok(());
            }
            parse_hook_sample(&choice)
        }
    };

    let Some(sample) = selected else {
        anyhow::bail!("unknown hook mode; try: xcodex hooks init");
    };

    let plan =
        codex_common::hooks_samples_install::plan_install_samples(codex_home, sample, args.force)?;
    let plan_text = codex_common::hooks_samples_install::format_sample_install_plan(&plan, sample)?;
    println!("{plan_text}");

    if args.dry_run {
        return Ok(());
    }

    if !args.yes {
        if interactive {
            if sample == HookSample::Pyo3 {
                println!();
                println!(
                    "Note: PyO3 hooks require a separately-built binary (not included by default)."
                );
                println!("Next: xcodex hooks doctor pyo3");
            }
            println!();
            if !prompt_confirm("Proceed with these changes?", false)? {
                return Ok(());
            }
        } else {
            println!();
            println!("Re-run with --yes to apply these changes.");
            return Ok(());
        }
    }

    codex_common::hooks_samples_install::apply_install_samples(codex_home, sample, args.force)?;

    if !args.no_print_config {
        println!();
        println!("Paste into {}/config.toml:", codex_home.display());
        println!();
        print!("{}", plan.config_snippet);
        println!();
        println!("Next:");
        match sample {
            HookSample::External => println!("- xcodex hooks test external --configured-only"),
            HookSample::PythonHost => println!("- xcodex hooks test python-host --configured-only"),
            HookSample::Pyo3 => {
                println!("- xcodex hooks doctor pyo3");
                println!("- xcodex hooks build pyo3");
            }
        }
    }

    if args.edit_config {
        let edited = edit_hooks_init_config_toml(codex_home, sample, &plan.config_snippet)?;
        if edited {
            println!();
            println!("Updated {}/config.toml.", codex_home.display());
        } else {
            println!();
            println!(
                "Skipped editing {}/config.toml because it already has the relevant section.",
                codex_home.display()
            );
        }
    }

    Ok(())
}

fn edit_hooks_init_config_toml(
    codex_home: &Path,
    sample: codex_common::hooks_samples_install::HookSample,
    config_snippet: &str,
) -> anyhow::Result<bool> {
    use toml_edit::DocumentMut;
    use toml_edit::Item;
    use toml_edit::Table;
    use toml_edit::Value;

    let config_path = codex_home.join("config.toml");
    let config_str = std::fs::read_to_string(&config_path).unwrap_or_default();

    let mut doc = if config_str.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_str
            .parse::<DocumentMut>()
            .map_err(|err| anyhow::anyhow!("failed to parse {}: {err}", config_path.display()))?
    };

    let snippet = config_snippet
        .parse::<DocumentMut>()
        .map_err(|err| anyhow::anyhow!("failed to parse config snippet: {err}"))?;

    let src_hooks = snippet
        .get("hooks")
        .and_then(Item::as_table)
        .ok_or_else(|| anyhow::anyhow!("config snippet is missing a [hooks] table"))?;

    let root = doc.as_table_mut();
    if !root.contains_key("hooks") {
        root["hooks"] = Item::Table(Table::new());
    }

    let hooks = root
        .get_mut("hooks")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("config has a non-table `hooks` key"))?;

    let changed = match sample {
        codex_common::hooks_samples_install::HookSample::External => {
            if hooks.contains_key("command") {
                false
            } else {
                let src = src_hooks
                    .get("command")
                    .ok_or_else(|| anyhow::anyhow!("config snippet is missing [hooks.command]"))?;
                hooks["command"] = src.clone();
                true
            }
        }
        codex_common::hooks_samples_install::HookSample::PythonHost => {
            if hooks.contains_key("host") {
                false
            } else {
                let src = src_hooks
                    .get("host")
                    .ok_or_else(|| anyhow::anyhow!("config snippet is missing [hooks.host]"))?;
                hooks["host"] = src.clone();
                true
            }
        }
        codex_common::hooks_samples_install::HookSample::Pyo3 => {
            let mut edited = false;

            if !hooks.contains_key("enable_unsafe_inproc") {
                hooks["enable_unsafe_inproc"] = toml_edit::value(true);
                edited = true;
            }

            match hooks.get_mut("inproc") {
                None => {
                    let mut arr = toml_edit::Array::new();
                    arr.push(Value::from("pyo3"));
                    hooks["inproc"] = Item::Value(Value::Array(arr));
                    edited = true;
                }
                Some(item) => {
                    let Some(arr) = item.as_array_mut() else {
                        anyhow::bail!("hooks.inproc exists but is not an array");
                    };

                    if !arr.iter().any(|value| value.as_str() == Some("pyo3")) {
                        arr.push(Value::from("pyo3"));
                        edited = true;
                    }
                }
            }

            if !hooks.contains_key("pyo3") {
                let src = src_hooks
                    .get("pyo3")
                    .ok_or_else(|| anyhow::anyhow!("config snippet is missing [hooks.pyo3]"))?;
                hooks["pyo3"] = src.clone();
                edited = true;
            }

            edited
        }
    };

    if !changed {
        return Ok(false);
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, doc.to_string())?;
    Ok(true)
}

fn run_hooks_install(codex_home: &Path, args: HooksInstallCommand) -> anyhow::Result<()> {
    if args.legacy_list || args.legacy_all || args.legacy_force {
        print_hooks_install_redirect();
        return Ok(());
    }

    let Some(sub) = args.sub else {
        print_hooks_install_usage();
        return Ok(());
    };

    match sub {
        HooksInstallSubcommand::Legacy(_args) => {
            print_hooks_install_redirect();
            Ok(())
        }
        HooksInstallSubcommand::Sdks(cmd) => run_hooks_install_sdks(codex_home, cmd),
        HooksInstallSubcommand::Samples(cmd) => run_hooks_install_samples(codex_home, cmd),
    }
}

fn run_hooks_install_sdks(codex_home: &Path, cmd: HooksInstallSdksCommand) -> anyhow::Result<()> {
    use codex_common::hooks_sdk_install;

    let interactive = is_interactive_stdin();

    let Some(sdk) = cmd.sdk.as_deref() else {
        print_hooks_install_sdks_list();
        return Ok(());
    };

    if sdk.eq_ignore_ascii_case("list") {
        print_hooks_install_sdks_list();
        return Ok(());
    }

    let targets = if sdk.eq_ignore_ascii_case("all") {
        hooks_sdk_install::all_hook_sdks()
    } else {
        vec![
            sdk.parse::<hooks_sdk_install::HookSdk>()
                .map_err(|_| anyhow::anyhow!("unknown SDK: {sdk}"))?,
        ]
    };

    let plan = hooks_sdk_install::plan_install_hook_sdks(codex_home, &targets, cmd.force)?;
    let plan_text = hooks_sdk_install::format_install_plan(&plan)?;
    println!("{plan_text}");

    if cmd.dry_run {
        return Ok(());
    }

    if !cmd.yes {
        if interactive {
            println!();
            if !prompt_confirm("Proceed with these changes?", false)? {
                return Ok(());
            }
        } else {
            println!();
            println!("Re-run with --yes to apply these changes.");
            return Ok(());
        }
    }

    let report = hooks_sdk_install::install_hook_sdks(codex_home, &targets, cmd.force)?;
    print!("{}", hooks_sdk_install::format_install_report(&report)?);
    Ok(())
}

fn run_hooks_install_samples(
    codex_home: &Path,
    cmd: HooksInstallSamplesCommand,
) -> anyhow::Result<()> {
    use codex_common::hooks_samples_install::HookSample;

    let interactive = is_interactive_stdin();

    let Some(sample) = cmd.sample.as_deref() else {
        print_hooks_install_samples_list();
        return Ok(());
    };

    if sample.eq_ignore_ascii_case("list") {
        print_hooks_install_samples_list();
        return Ok(());
    }

    let samples: Vec<HookSample> = if sample.eq_ignore_ascii_case("all") {
        vec![
            HookSample::External,
            HookSample::PythonHost,
            HookSample::Pyo3,
        ]
    } else {
        vec![parse_hook_sample(sample).ok_or_else(|| anyhow::anyhow!("unknown sample: {sample}"))?]
    };

    for sample in samples {
        let plan = codex_common::hooks_samples_install::plan_install_samples(
            codex_home, sample, cmd.force,
        )?;
        let plan_text =
            codex_common::hooks_samples_install::format_sample_install_plan(&plan, sample)?;
        println!("{plan_text}");

        if cmd.dry_run {
            continue;
        }

        if !cmd.yes {
            if interactive {
                println!();
                if sample == HookSample::Pyo3 {
                    println!(
                        "Note: PyO3 hooks require a separately-built binary (not included by default)."
                    );
                    println!("Next: xcodex hooks doctor pyo3");
                    println!();
                }
                if !prompt_confirm("Proceed with these changes?", false)? {
                    continue;
                }
            } else {
                println!();
                println!("Re-run with --yes to apply these changes.");
                break;
            }
        }

        codex_common::hooks_samples_install::apply_install_samples(codex_home, sample, cmd.force)?;
        println!();
        println!("Paste into {}/config.toml:", codex_home.display());
        println!();
        print!("{}", plan.config_snippet);
    }

    Ok(())
}

fn print_hooks_install_usage() {
    println!("Hooks install commands:");
    println!("- xcodex hooks install sdks list");
    println!("- xcodex hooks install sdks <sdk|all> [--dry-run] [--force] [--yes]");
    println!("- xcodex hooks install samples list");
    println!(
        "- xcodex hooks install samples <external|python-host|pyo3|all> [--dry-run] [--force] [--yes]"
    );
    println!();
    println!("Try: xcodex hooks init");
}

fn print_hooks_install_redirect() {
    println!("This install command syntax has changed.");
    print_hooks_install_usage();
}

fn print_hooks_install_sdks_list() {
    println!("Available SDKs:");
    for sdk in codex_common::hooks_sdk_install::all_hook_sdks() {
        println!("- {}: {}", sdk.id(), sdk.description());
    }
    println!("- all: install everything");
}

fn print_hooks_install_samples_list() {
    use codex_common::hooks_samples_install::HookSample;
    println!("Available sample sets:");
    for sample in [
        HookSample::External,
        HookSample::PythonHost,
        HookSample::Pyo3,
    ] {
        println!("- {}: {}", sample.id(), sample.description());
    }
    println!("- all: install everything");
}

fn print_hooks_init_menu() {
    use codex_common::hooks_samples_install::HookSample;
    println!("Hooks init:");
    println!();
    println!(
        "1) {}  (id: {})",
        HookSample::External.title(),
        HookSample::External.id()
    );
    println!("   {}", HookSample::External.description());
    println!();
    println!(
        "2) {}  (id: {})",
        HookSample::PythonHost.title(),
        HookSample::PythonHost.id()
    );
    println!("   {}", HookSample::PythonHost.description());
    println!();
    println!(
        "3) {}  (id: {})",
        HookSample::Pyo3.title(),
        HookSample::Pyo3.id()
    );
    println!("   {}", HookSample::Pyo3.description());
}

fn parse_hook_sample(raw: &str) -> Option<codex_common::hooks_samples_install::HookSample> {
    use codex_common::hooks_samples_install::HookSample;
    let raw = raw.trim().to_ascii_lowercase();
    match raw.as_str() {
        "1" | "external" | "spawn" | "one-shot" | "oneshot" => Some(HookSample::External),
        "2" | "python-host" | "pythonhost" | "python-box" | "py-box" | "pybox" | "host" => {
            Some(HookSample::PythonHost)
        }
        "3" | "pyo3" => Some(HookSample::Pyo3),
        _ => None,
    }
}

fn print_hooks_list(codex_home: &Path, hooks: &codex_core::config::HooksConfig, all: bool) {
    println!("CODEX_HOME: {}", codex_home.display());
    println!("Config: {}", codex_home.join("config.toml").display());
    println!(
        "hooks.max_stdin_payload_bytes={}",
        hooks.max_stdin_payload_bytes
    );
    println!("hooks.keep_last_n_payloads={}", hooks.keep_last_n_payloads);
    println!(
        "hooks.inproc_tool_call_summary={}",
        hooks.inproc_tool_call_summary
    );
    println!("hooks.inproc={:?}", hooks.inproc);
    println!("hooks.host.enabled={}", hooks.host.enabled);
    println!("hooks.host.command={:?}", hooks.host.command);
    println!("hooks.host.sandbox_mode={:?}", hooks.host.sandbox_mode);

    let entries: [(&str, &Vec<Vec<String>>); 8] = [
        ("hooks.agent_turn_complete", &hooks.agent_turn_complete),
        ("hooks.approval_requested", &hooks.approval_requested),
        ("hooks.session_start", &hooks.session_start),
        ("hooks.session_end", &hooks.session_end),
        ("hooks.model_request_started", &hooks.model_request_started),
        (
            "hooks.model_response_completed",
            &hooks.model_response_completed,
        ),
        ("hooks.tool_call_started", &hooks.tool_call_started),
        ("hooks.tool_call_finished", &hooks.tool_call_finished),
    ];

    let configured = entries
        .iter()
        .filter(|(_key, commands)| !commands.is_empty())
        .count();
    println!("Configured events: {configured}");

    for (key, commands) in entries {
        if commands.is_empty() && !all {
            continue;
        }

        println!();
        println!("{key}:");
        if commands.is_empty() {
            println!("- (none)");
            continue;
        }

        for command in commands {
            println!("- {command:?}");
        }
    }
}

fn print_hooks_paths(codex_home: &Path, hooks: &codex_core::config::HooksConfig) {
    println!("CODEX_HOME: {}", codex_home.display());
    println!("Logs: {}", hooks_logs_dir(codex_home).display());
    println!(
        "Host logs: {}",
        codex_home
            .join("tmp")
            .join("hooks")
            .join("host")
            .join("logs")
            .display()
    );
    println!("Payloads: {}", hooks_payloads_dir(codex_home).display());
    println!(
        "Tool call summaries (in-proc): {}",
        codex_home.join("hooks-tool-calls.log").display()
    );
    println!("hooks.keep_last_n_payloads={}", hooks.keep_last_n_payloads);
    println!(
        "hooks.max_stdin_payload_bytes={}",
        hooks.max_stdin_payload_bytes
    );
}
