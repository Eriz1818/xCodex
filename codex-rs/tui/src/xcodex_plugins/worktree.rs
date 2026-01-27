use crate::chatwidget::ChatWidget;
use crate::chatwidget::transcript_spacer_line;
use crate::history_cell;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::slash_command::SlashCommand;
use codex_core::git_info::GitWorktreeEntry;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::PathBuf;

use super::PluginSubcommandHintOrder;
use super::PluginSubcommandNode;
use super::PluginSubcommandRoot;

const WORKTREE_SHARED_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "add",
        full_name: "worktree shared add",
        description: "add a repo-relative shared dir to config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "rm",
        full_name: "worktree shared rm",
        description: "remove a shared dir from config",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "list",
        full_name: "worktree shared list",
        description: "show configured shared dirs",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const WORKTREE_LINK_SHARED_CHILDREN: &[PluginSubcommandNode] = &[PluginSubcommandNode {
    token: "--migrate",
    full_name: "worktree link-shared --migrate",
    description: "migrate untracked files into workspace root, then link",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const WORKTREE_SUBCOMMANDS: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "detect",
        full_name: "worktree detect",
        description: "refresh git worktree list and open picker",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "doctor",
        full_name: "worktree doctor",
        description: "show shared-dir + untracked status for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "link-shared",
        full_name: "worktree link-shared",
        description: "apply shared-dir links for this worktree",
        run_on_enter: true,
        insert_trailing_space: false,
        children: WORKTREE_LINK_SHARED_CHILDREN,
    },
    PluginSubcommandNode {
        token: "init",
        full_name: "worktree init",
        description: "create a new worktree and switch to it",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "shared",
        full_name: "worktree shared",
        description: "manage `worktrees.shared_dirs` from the TUI",
        run_on_enter: false,
        insert_trailing_space: true,
        children: WORKTREE_SHARED_CHILDREN,
    },
];

const WORKTREE_HINT_ORDER: &[PluginSubcommandHintOrder] = &[
    PluginSubcommandHintOrder {
        token: "detect",
        order: 0,
    },
    PluginSubcommandHintOrder {
        token: "doctor",
        order: 1,
    },
    PluginSubcommandHintOrder {
        token: "init",
        order: 2,
    },
    PluginSubcommandHintOrder {
        token: "shared",
        order: 3,
    },
    PluginSubcommandHintOrder {
        token: "link-shared",
        order: 4,
    },
];

pub(crate) const WORKTREE_SUBCOMMAND_ROOT: PluginSubcommandRoot = PluginSubcommandRoot {
    root: "worktree",
    anchor: SlashCommand::Worktree,
    children: WORKTREE_SUBCOMMANDS,
    list_hint_order: Some(WORKTREE_HINT_ORDER),
};

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    handle_worktree_command(chat, args);
    true
}

pub(crate) fn handle_worktree_command(chat: &mut ChatWidget, rest: &str) {
    let args: Vec<&str> = rest.split_whitespace().collect();
    match args.as_slice() {
        [] => {
            chat.dispatch_slash_command(SlashCommand::Worktree);
        }
        ["detect"] | ["refresh"] => {
            chat.spawn_worktree_detection(true);
        }
        ["shared"] | ["shared", "list"] => {
            chat.add_worktree_shared_dirs_output();
        }
        ["shared", "add", dir] => {
            fn normalize_shared_dir_arg(raw: &str) -> Result<String, String> {
                use std::path::Component;
                use std::path::Path;

                let mut value = raw.trim().trim_end_matches(['/', '\\']).to_string();
                while value.starts_with("./") {
                    value = value.trim_start_matches("./").to_string();
                }
                if value.is_empty() {
                    return Err(String::from("shared dir is empty"));
                }
                if value.starts_with('~') {
                    return Err(String::from("shared dirs must be repo-relative (no '~')"));
                }

                let path = Path::new(&value);
                if path.is_absolute() {
                    return Err(String::from("shared dirs must be repo-relative"));
                }

                for component in path.components() {
                    match component {
                        Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                            return Err(String::from(
                                "shared dirs must not contain parent/root components",
                            ));
                        }
                        Component::CurDir => {}
                        Component::Normal(_) => {}
                    }
                }

                Ok(value)
            }

            let dir = match normalize_shared_dir_arg(dir) {
                Ok(dir) => dir,
                Err(err) => {
                    chat.add_error_message(format!("`/worktree shared add` — {err}"));
                    return;
                }
            };

            let mut next = chat.worktrees_shared_dirs().to_vec();
            if next.contains(&dir) {
                chat.add_info_message(
                    format!("Shared dir already configured: `{dir}`"),
                    Some(String::from("Tip: run `/worktree shared list`")),
                );
                return;
            }
            next.push(dir);
            chat.update_worktrees_shared_dirs(next);
            chat.add_worktree_shared_dirs_output();
        }
        ["shared", "rm", dir] | ["shared", "remove", dir] => {
            fn normalize_shared_dir_arg(raw: &str) -> Result<String, String> {
                use std::path::Component;
                use std::path::Path;

                let mut value = raw.trim().trim_end_matches(['/', '\\']).to_string();
                while value.starts_with("./") {
                    value = value.trim_start_matches("./").to_string();
                }
                if value.is_empty() {
                    return Err(String::from("shared dir is empty"));
                }
                if value.starts_with('~') {
                    return Err(String::from("shared dirs must be repo-relative (no '~')"));
                }

                let path = Path::new(&value);
                if path.is_absolute() {
                    return Err(String::from("shared dirs must be repo-relative"));
                }

                for component in path.components() {
                    match component {
                        Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                            return Err(String::from(
                                "shared dirs must not contain parent/root components",
                            ));
                        }
                        Component::CurDir => {}
                        Component::Normal(_) => {}
                    }
                }

                Ok(value)
            }

            let dir = match normalize_shared_dir_arg(dir) {
                Ok(dir) => dir,
                Err(err) => {
                    chat.add_error_message(format!("`/worktree shared rm` — {err}"));
                    return;
                }
            };

            let mut next: Vec<String> = Vec::new();
            let mut removed = 0usize;
            for entry in chat.worktrees_shared_dirs() {
                let normalized_entry =
                    normalize_shared_dir_arg(entry).unwrap_or_else(|_| entry.clone());
                if normalized_entry == dir {
                    removed += 1;
                    continue;
                }
                next.push(entry.clone());
            }
            if removed == 0 {
                chat.add_error_message(format!(
                    "`/worktree shared rm` — `{dir}` is not in `worktrees.shared_dirs`"
                ));
                return;
            }
            chat.update_worktrees_shared_dirs(next);
            chat.add_worktree_shared_dirs_output();
        }
        ["init"] => {
            chat.spawn_worktree_init_wizard();
        }
        ["init", name, branch] | ["init", name, branch, ..] => {
            let provided_path = args.get(3).copied();
            if args.len() > 4 {
                chat.add_info_message(
                    "Usage: /worktree init <name> <branch> [<path>]".to_string(),
                    None,
                );
                return;
            }

            let name = name.to_string();
            let branch = branch.to_string();
            let path: Option<PathBuf> = provided_path.map(PathBuf::from);
            let invoked = if let Some(path) = provided_path {
                format!("/worktree init {name} {branch} {path}")
            } else {
                format!("/worktree init {name} {branch}")
            };
            chat.spawn_worktree_init_command(name, branch, path, invoked);
        }
        ["doctor"] => {
            chat.spawn_worktree_doctor();
        }
        ["link-shared", "migrate"] | ["link-shared", "--migrate"] => {
            if chat.worktrees_shared_dirs().is_empty() {
                let command = PlainHistoryCell::new(vec![Line::from(vec![
                    "/worktree link-shared --migrate".magenta(),
                ])]);
                let lines: Vec<Line<'static>> = vec![
                    Line::from(vec![
                        "No shared dirs configured.".into(),
                        " Add them first:".dim(),
                    ]),
                    Line::from(vec!["  /worktree shared add docs/impl-plans".cyan()]),
                    Line::from(vec!["  /worktree shared add docs/personal".cyan()]),
                    transcript_spacer_line(),
                    Line::from(vec!["Then: ".dim(), "/worktree link-shared".cyan()]),
                    Line::from(vec!["Docs: ".dim(), "docs/xcodex/worktrees.md".cyan()]),
                ];
                chat.add_to_history(CompositeHistoryCell::new(vec![
                    Box::new(command),
                    Box::new(PlainHistoryCell::new(lines)),
                ]));
                return;
            }

            let show_notice = chat.take_shared_dirs_write_notice();

            let cwd = chat.session_cwd().to_path_buf();
            let Some(worktree_root) = codex_core::git_info::resolve_git_worktree_head(&cwd)
                .map(|head| head.worktree_root)
            else {
                chat.add_error_message(String::from(
                    "`/worktree link-shared` — not inside a git worktree (start xcodex in a repo/worktree directory, or switch via `/worktree`)",
                ));
                return;
            };

            let Some(workspace_root) =
                codex_core::git_info::resolve_root_git_project_for_trust(&worktree_root)
            else {
                chat.add_error_message(String::from(
                    "`/worktree link-shared` — failed to resolve workspace root (the main worktree root for this repo)",
                ));
                return;
            };

            if worktree_root == workspace_root {
                chat.add_to_history(history_cell::new_info_event(
                    String::from("Already in the workspace root worktree."),
                    None,
                ));
                return;
            }

            chat.open_worktree_link_shared_wizard(
                worktree_root,
                workspace_root,
                chat.worktrees_shared_dirs().to_vec(),
                true,
                show_notice,
                String::from("/worktree link-shared --migrate"),
            );
        }
        ["link-shared"] => {
            if chat.worktrees_shared_dirs().is_empty() {
                let command = PlainHistoryCell::new(vec![Line::from(vec![
                    "/worktree link-shared".magenta(),
                ])]);
                let lines: Vec<Line<'static>> = vec![
                    Line::from(vec![
                        "No shared dirs configured.".into(),
                        " Add them first:".dim(),
                    ]),
                    Line::from(vec!["  /worktree shared add docs/impl-plans".cyan()]),
                    Line::from(vec!["  /worktree shared add docs/personal".cyan()]),
                    transcript_spacer_line(),
                    Line::from(vec![
                        "Then: ".dim(),
                        "/worktree link-shared".cyan(),
                        " ".dim(),
                        "(and choose migrate+link if needed)".dim(),
                    ]),
                    Line::from(vec!["Docs: ".dim(), "docs/xcodex/worktrees.md".cyan()]),
                ];
                chat.add_to_history(CompositeHistoryCell::new(vec![
                    Box::new(command),
                    Box::new(PlainHistoryCell::new(lines)),
                ]));
                return;
            }

            let show_notice = chat.take_shared_dirs_write_notice();

            let cwd = chat.session_cwd().to_path_buf();
            let Some(worktree_root) = codex_core::git_info::resolve_git_worktree_head(&cwd)
                .map(|head| head.worktree_root)
            else {
                chat.add_error_message(String::from(
                    "`/worktree link-shared` — not inside a git worktree (start xcodex in a repo/worktree directory, or switch via `/worktree`)",
                ));
                return;
            };

            let Some(workspace_root) =
                codex_core::git_info::resolve_root_git_project_for_trust(&worktree_root)
            else {
                chat.add_error_message(String::from(
                    "`/worktree link-shared` — failed to resolve workspace root (the main worktree root for this repo)",
                ));
                return;
            };

            if worktree_root == workspace_root {
                chat.add_to_history(history_cell::new_info_event(
                    String::from("Already in the workspace root worktree."),
                    None,
                ));
                return;
            }

            chat.open_worktree_link_shared_wizard(
                worktree_root,
                workspace_root,
                chat.worktrees_shared_dirs().to_vec(),
                false,
                show_notice,
                String::from("/worktree link-shared"),
            );
        }
        [target] => {
            let matches: Vec<&GitWorktreeEntry> = chat
                .worktree_list()
                .iter()
                .filter(|entry| {
                    entry
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case(target))
                })
                .collect();

            let selected_path = if matches.len() == 1 {
                Some(matches[0].path.clone())
            } else if matches.len() > 1 {
                chat.add_info_message(
                    format!(
                        "Multiple worktrees match `{target}`. Use a full path or run `/worktree` to pick."
                    ),
                    None,
                );
                None
            } else {
                let candidate = PathBuf::from(target);
                let candidate = if candidate.is_absolute() {
                    candidate
                } else {
                    chat.session_cwd().join(candidate)
                };
                if candidate.is_dir() {
                    Some(candidate)
                } else {
                    if chat.worktree_list().is_empty() && !chat.worktree_list_refresh_in_progress()
                    {
                        chat.spawn_worktree_detection(true);
                        chat.add_info_message(
                            format!(
                                "Unknown worktree `{target}`. Refreshing worktrees; run `/worktree` to pick."
                            ),
                            None,
                        );
                    } else {
                        chat.add_info_message(
                            format!(
                                "Unknown worktree `{target}`. Run `/worktree` to pick or `/worktree detect` to refresh."
                            ),
                            None,
                        );
                    }
                    None
                }
            };

            if let Some(path) = selected_path {
                chat.emit_worktree_switch(path);
            }
        }
        _ => {
            chat.add_info_message(
                "Usage: /worktree [detect|doctor|shared|init|link-shared [--migrate]|<name|path>]"
                    .to_string(),
                None,
            );
        }
    }
}
