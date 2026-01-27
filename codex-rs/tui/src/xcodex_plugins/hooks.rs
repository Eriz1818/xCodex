use crate::chatwidget::ChatWidget;
use crate::chatwidget::transcript_spacer_line;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::slash_command::SlashCommand;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) fn handle_hooks_command(chat: &mut ChatWidget, rest: &str) {
    let args: Vec<&str> = rest.split_whitespace().collect();
    match args.as_slice() {
        [] => {
            chat.dispatch_slash_command(SlashCommand::Hooks);
        }
        ["init"] => {
            use codex_common::hooks_samples_install::HookSample;
            let lines = vec![
                vec!["/hooks init".magenta()].into(),
                transcript_spacer_line(),
                vec!["Choose a hook mode:".magenta().bold()].into(),
                vec!["1) ".dim(), HookSample::External.title().into()].into(),
                vec!["   ".into(), HookSample::External.description().dim()].into(),
                transcript_spacer_line(),
                vec!["2) ".dim(), HookSample::PythonHost.title().into()].into(),
                vec!["   ".into(), HookSample::PythonHost.description().dim()].into(),
                transcript_spacer_line(),
                vec!["3) ".dim(), HookSample::Pyo3.title().into()].into(),
                vec!["   ".into(), HookSample::Pyo3.description().dim()].into(),
                transcript_spacer_line(),
                vec!["Run: ".dim(), "/hooks init external".cyan()].into(),
                vec!["Run: ".dim(), "/hooks init python-host".cyan()].into(),
                vec!["Run: ".dim(), "/hooks init pyo3".cyan()].into(),
                transcript_spacer_line(),
                vec![
                    "Note: ".dim(),
                    "installing writes files; re-run with ".dim(),
                    "--yes".cyan(),
                    " to apply.".dim(),
                ]
                .into(),
            ];
            chat.add_plain_history_lines(lines);
        }
        ["init", mode, rest @ ..] => {
            let mut force = false;
            let mut dry_run = false;
            let mut yes = false;
            for arg in rest {
                match *arg {
                    "--force" => force = true,
                    "--dry-run" => dry_run = true,
                    "--yes" => yes = true,
                    _ => {
                        chat.add_info_message(
                            "Usage: /hooks init <external|python-host|pyo3> [--dry-run] [--force] [--yes]".to_string(),
                            None,
                        );
                        return;
                    }
                }
            }

            let sample = match mode.to_ascii_lowercase().as_str() {
                "1" | "external" => codex_common::hooks_samples_install::HookSample::External,
                "2" | "python-host" | "pythonhost" | "python-box" | "py-box" => {
                    codex_common::hooks_samples_install::HookSample::PythonHost
                }
                "3" | "pyo3" => codex_common::hooks_samples_install::HookSample::Pyo3,
                _ => {
                    chat.add_info_message("Unknown hook mode. Try: /hooks init".to_string(), None);
                    return;
                }
            };

            let codex_home = chat.codex_home().to_path_buf();
            let plan = codex_common::hooks_samples_install::plan_install_samples(
                &codex_home,
                sample,
                force,
            );
            let plan = match plan {
                Ok(plan) => plan,
                Err(err) => {
                    chat.add_error_message(format!("hooks init failed: {err}"));
                    return;
                }
            };

            let text =
                codex_common::hooks_samples_install::format_sample_install_plan(&plan, sample)
                    .unwrap_or_else(|_| String::from("failed to format plan"));
            chat.add_plain_history_lines(text.lines().map(|l| Line::from(l.to_string())).collect());

            if dry_run {
                return;
            }
            if !yes {
                chat.add_info_message(
                    "Re-run with --yes to apply these changes.".to_string(),
                    None,
                );
                return;
            }

            if let Err(err) = codex_common::hooks_samples_install::apply_install_samples(
                &codex_home,
                sample,
                force,
            ) {
                chat.add_error_message(format!("hooks init failed: {err}"));
            }
        }
        ["install", "sdks", "list"] | ["install", "sdks", "--list"] => {
            let mut lines = Vec::new();
            lines.push(vec!["/hooks install sdks list".magenta()].into());
            lines.push(transcript_spacer_line());
            lines.push(vec!["Available SDKs:".magenta().bold()].into());
            for sdk in codex_common::hooks_sdk_install::all_hook_sdks() {
                lines.push(
                    vec![
                        "- ".dim(),
                        sdk.id().cyan(),
                        ": ".dim(),
                        sdk.description().into(),
                    ]
                    .into(),
                );
            }
            lines.push(
                vec![
                    "- ".dim(),
                    "all".cyan(),
                    ": ".dim(),
                    "install everything".into(),
                ]
                .into(),
            );
            chat.add_plain_history_lines(lines);
        }
        ["install", "samples", "list"] | ["install", "samples", "--list"] => {
            use codex_common::hooks_samples_install::HookSample;
            let mut lines = Vec::new();
            lines.push(vec!["/hooks install samples list".magenta()].into());
            lines.push(transcript_spacer_line());
            lines.push(vec!["Available sample sets:".magenta().bold()].into());
            for sample in [
                HookSample::External,
                HookSample::PythonHost,
                HookSample::Pyo3,
            ] {
                lines.push(
                    vec![
                        "- ".dim(),
                        sample.id().cyan(),
                        ": ".dim(),
                        sample.description().into(),
                    ]
                    .into(),
                );
            }
            lines.push(
                vec![
                    "- ".dim(),
                    "all".cyan(),
                    ": ".dim(),
                    "install everything".into(),
                ]
                .into(),
            );
            chat.add_plain_history_lines(lines);
        }
        ["install", "sdks", ..] => {
            let mut force = false;
            let mut dry_run = false;
            let mut yes = false;
            let mut sdk_name: Option<&str> = None;
            for arg in &args[2..] {
                match *arg {
                    "--force" => force = true,
                    "--dry-run" => dry_run = true,
                    "--yes" => yes = true,
                    _ if arg.starts_with('-') => {
                        chat.add_info_message(
                            "Usage: /hooks install sdks <sdk|all> [--dry-run] [--force] [--yes] | /hooks install sdks list"
                                .to_string(),
                            None,
                        );
                        return;
                    }
                    _ => {
                        if sdk_name.is_some() {
                            chat.add_info_message(
                                "Usage: /hooks install sdks <sdk|all> [--dry-run] [--force] [--yes] | /hooks install sdks list"
                                    .to_string(),
                                None,
                            );
                            return;
                        }
                        sdk_name = Some(*arg);
                    }
                }
            }

            let Some(sdk_name) = sdk_name else {
                chat.add_info_message(
                    "Usage: /hooks install sdks <sdk|all> [--dry-run] [--force] [--yes] | /hooks install sdks list"
                        .to_string(),
                    None,
                );
                return;
            };

            let targets = if sdk_name.eq_ignore_ascii_case("all") {
                codex_common::hooks_sdk_install::all_hook_sdks()
            } else {
                match sdk_name.parse::<codex_common::hooks_sdk_install::HookSdk>() {
                    Ok(sdk) => vec![sdk],
                    Err(_) => {
                        chat.add_info_message(
                            format!("Unknown SDK `{sdk_name}`. Try: /hooks install sdks list"),
                            None,
                        );
                        return;
                    }
                }
            };

            let codex_home = chat.codex_home().to_path_buf();
            let plan = codex_common::hooks_sdk_install::plan_install_hook_sdks(
                &codex_home,
                &targets,
                force,
            );
            let plan = match plan {
                Ok(plan) => plan,
                Err(err) => {
                    chat.add_error_message(format!("hooks install failed: {err}"));
                    return;
                }
            };
            let text = codex_common::hooks_sdk_install::format_install_plan(&plan)
                .unwrap_or_else(|_| String::from("failed to format plan"));
            chat.add_plain_history_lines(text.lines().map(|l| Line::from(l.to_string())).collect());

            if dry_run {
                return;
            }
            if !yes {
                chat.add_info_message(
                    "Re-run with --yes to apply these changes.".to_string(),
                    None,
                );
                return;
            }

            let report =
                codex_common::hooks_sdk_install::install_hook_sdks(&codex_home, &targets, force);
            match report {
                Ok(report) => {
                    let text = codex_common::hooks_sdk_install::format_install_report(&report)
                        .unwrap_or_else(|_| String::from("installed hook SDK files"));
                    chat.add_plain_history_lines(
                        text.lines().map(|l| Line::from(l.to_string())).collect(),
                    );
                }
                Err(err) => chat.add_error_message(format!("hooks install failed: {err}")),
            }
        }
        ["install", "samples", ..] => {
            let mut force = false;
            let mut dry_run = false;
            let mut yes = false;
            let mut sample_name: Option<&str> = None;
            for arg in &args[2..] {
                match *arg {
                    "--force" => force = true,
                    "--dry-run" => dry_run = true,
                    "--yes" => yes = true,
                    _ if arg.starts_with('-') => {
                        chat.add_info_message(
                            "Usage: /hooks install samples <external|python-host|pyo3|all> [--dry-run] [--force] [--yes] | /hooks install samples list"
                                .to_string(),
                            None,
                        );
                        return;
                    }
                    _ => {
                        if sample_name.is_some() {
                            chat.add_info_message(
                                "Usage: /hooks install samples <external|python-host|pyo3|all> [--dry-run] [--force] [--yes] | /hooks install samples list"
                                    .to_string(),
                                None,
                            );
                            return;
                        }
                        sample_name = Some(*arg);
                    }
                }
            }

            let Some(sample_name) = sample_name else {
                chat.add_info_message(
                    "Usage: /hooks install samples <external|python-host|pyo3|all> [--dry-run] [--force] [--yes] | /hooks install samples list"
                        .to_string(),
                    None,
                );
                return;
            };

            let samples = if sample_name.eq_ignore_ascii_case("all") {
                vec![
                    codex_common::hooks_samples_install::HookSample::External,
                    codex_common::hooks_samples_install::HookSample::PythonHost,
                    codex_common::hooks_samples_install::HookSample::Pyo3,
                ]
            } else {
                let sample = match sample_name.to_ascii_lowercase().as_str() {
                    "external" => codex_common::hooks_samples_install::HookSample::External,
                    "python-host" | "pythonhost" | "python-box" | "py-box" => {
                        codex_common::hooks_samples_install::HookSample::PythonHost
                    }
                    "pyo3" => codex_common::hooks_samples_install::HookSample::Pyo3,
                    _ => {
                        chat.add_info_message(
                            "Unknown sample. Try: /hooks install samples list".to_string(),
                            None,
                        );
                        return;
                    }
                };
                vec![sample]
            };

            let codex_home = chat.codex_home().to_path_buf();
            for sample in samples {
                let plan = codex_common::hooks_samples_install::plan_install_samples(
                    &codex_home,
                    sample,
                    force,
                );
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(err) => {
                        chat.add_error_message(format!("hooks install failed: {err}"));
                        return;
                    }
                };
                let text =
                    codex_common::hooks_samples_install::format_sample_install_plan(&plan, sample)
                        .unwrap_or_else(|_| String::from("failed to format plan"));
                chat.add_plain_history_lines(
                    text.lines().map(|l| Line::from(l.to_string())).collect(),
                );

                if dry_run {
                    continue;
                }
                if !yes {
                    chat.add_info_message(
                        "Re-run with --yes to apply these changes.".to_string(),
                        None,
                    );
                    return;
                }
                if let Err(err) = codex_common::hooks_samples_install::apply_install_samples(
                    &codex_home,
                    sample,
                    force,
                ) {
                    chat.add_error_message(format!("hooks install failed: {err}"));
                }
            }
        }
        _ => {
            chat.add_info_message(
                "Usage: /hooks init | /hooks install sdks ... | /hooks install samples ..."
                    .to_string(),
                None,
            );
        }
    }
}

pub(crate) fn add_hooks_output(chat: &mut ChatWidget) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/hooks".magenta()])]);
    let codex_home = chat.codex_home().to_path_buf();
    let logs_dir = codex_home.join("tmp").join("hooks").join("logs");
    let payloads_dir = codex_home.join("tmp").join("hooks").join("payloads");

    let lines: Vec<Line<'static>> = vec![
        Line::from(vec![
            "Automation hooks run external programs on lifecycle events. ".into(),
            "Treat hook payloads/logs as potentially sensitive.".dim(),
        ]),
        transcript_spacer_line(),
        Line::from(vec!["Quickstart:".magenta().bold()]),
        Line::from(vec!["  xcodex hooks init".cyan()]),
        Line::from(vec!["  xcodex hooks install sdks list".cyan()]),
        Line::from(vec!["  xcodex hooks install sdks python".cyan()]),
        Line::from(vec!["  xcodex hooks install samples list".cyan()]),
        Line::from(vec!["  xcodex hooks install samples external".cyan()]),
        Line::from(vec!["  xcodex hooks help".cyan()]),
        Line::from(vec![
            "  xcodex hooks test external --configured-only".cyan(),
        ]),
        Line::from(vec!["  xcodex hooks list".cyan()]),
        Line::from(vec!["  xcodex hooks paths".cyan()]),
        transcript_spacer_line(),
        Line::from(vec![
            "Config: ".dim(),
            format!("{}/config.toml", codex_home.display()).into(),
        ]),
        Line::from(vec![
            "Logs: ".dim(),
            format!("{}", logs_dir.display()).into(),
        ]),
        Line::from(vec![
            "Payloads: ".dim(),
            format!("{}", payloads_dir.display()).into(),
        ]),
        transcript_spacer_line(),
        Line::from(vec![
            "Docs: ".dim(),
            "docs/xcodex/hooks.md".cyan(),
            " and ".dim(),
            "docs/xcodex/hooks-gallery.md".cyan(),
            " and ".dim(),
            "docs/xcodex/hooks-sdks.md".cyan(),
            " and ".dim(),
            "docs/xcodex/hooks-python-host.md".cyan(),
            " and ".dim(),
            "docs/xcodex/hooks-pyo3.md".cyan(),
        ]),
    ];

    chat.add_to_history(CompositeHistoryCell::new(vec![
        Box::new(command),
        Box::new(PlainHistoryCell::new(lines)),
    ]));
}
