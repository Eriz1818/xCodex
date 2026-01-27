use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::WorktreeInitWizardView;
use crate::bottom_pane::WorktreeLinkSharedWizardView;
use crate::bottom_pane::WorktreesSettingsView;
use crate::chatwidget::ChatWidget;
use crate::chatwidget::transcript_spacer_line;
use crate::history_cell;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::key_hint;
use crate::slash_command::SlashCommand;
use codex_core::git_info::GitHeadState;
use codex_core::git_info::GitWorktreeEntry;
use codex_core::protocol::Op;
use crossterm::event::KeyCode;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::Path;
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
            spawn_worktree_detection(chat, true);
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
            spawn_worktree_init_wizard(chat);
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
            spawn_worktree_init_command(chat, name, branch, path, invoked);
        }
        ["doctor"] => {
            spawn_worktree_doctor(chat);
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

            open_worktree_link_shared_wizard(
                chat,
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

            open_worktree_link_shared_wizard(
                chat,
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
                        spawn_worktree_detection(chat, true);
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

pub(crate) fn add_worktree_shared_dirs_output(chat: &mut ChatWidget) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/worktree shared".magenta()])]);
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(
        vec![
            "worktrees.shared_dirs".cyan().bold(),
            " (shared across worktrees)".dim(),
        ]
        .into(),
    );

    let shared_dirs = chat.worktrees_shared_dirs();
    if shared_dirs.is_empty() {
        lines.push(vec!["(none)".dim()].into());
    } else {
        for dir in shared_dirs {
            lines.push(vec!["- ".dim(), dir.clone().into()].into());
        }
    }

    lines.push(transcript_spacer_line());
    lines.push(vec!["Add: ".dim(), "/worktree shared add <dir>".cyan()].into());
    lines.push(vec!["Remove: ".dim(), "/worktree shared rm <dir>".cyan()].into());
    lines.push(
        vec![
            "Apply: ".dim(),
            "/worktree link-shared".cyan(),
            " ".dim(),
            "(in current worktree)".dim(),
        ]
        .into(),
    );
    lines.push(vec!["Docs: ".dim(), "docs/xcodex/worktrees.md".cyan()].into());

    chat.add_to_history(CompositeHistoryCell::new(vec![
        Box::new(command),
        Box::new(PlainHistoryCell::new(lines)),
    ]));
}

pub(crate) fn open_worktree_link_shared_wizard(
    chat: &mut ChatWidget,
    worktree_root: PathBuf,
    workspace_root: PathBuf,
    shared_dirs: Vec<String>,
    prefer_migrate: bool,
    show_notice: bool,
    invoked_from: String,
) {
    let view = WorktreeLinkSharedWizardView::new(
        worktree_root,
        workspace_root,
        shared_dirs,
        prefer_migrate,
        show_notice,
        invoked_from,
        chat.app_event_tx(),
    );
    chat.show_view(Box::new(view));
}

pub(crate) fn open_worktrees_settings_view(chat: &mut ChatWidget) {
    let view = WorktreesSettingsView::new(
        chat.worktrees_shared_dirs().to_vec(),
        chat.worktrees_pinned_paths().to_vec(),
        chat.app_event_tx(),
    );
    chat.show_view(Box::new(view));
}

pub(crate) fn open_worktree_init_wizard(
    chat: &mut ChatWidget,
    worktree_root: PathBuf,
    workspace_root: PathBuf,
    current_branch: Option<String>,
    shared_dirs: Vec<String>,
    branches: Vec<String>,
) {
    let view = WorktreeInitWizardView::new(
        worktree_root,
        workspace_root,
        current_branch,
        shared_dirs,
        branches,
        chat.app_event_tx(),
    );
    chat.show_view(Box::new(view));
}

pub(crate) fn spawn_worktree_detection(chat: &mut ChatWidget, open_picker: bool) {
    if chat.worktree_list_refresh_in_progress() {
        return;
    }
    if codex_core::git_info::resolve_git_worktree_head(chat.session_cwd()).is_none() {
        chat.worktree_state_clear_no_repo();
        if open_picker {
            open_worktree_picker(chat);
        }
        return;
    }

    chat.worktree_state_mark_refreshing();

    let cwd = chat.session_cwd().to_path_buf();
    let tx = chat.app_event_tx();
    tokio::spawn(async move {
        match codex_core::git_info::try_list_git_worktrees(&cwd).await {
            Ok(worktrees) => {
                tx.send(AppEvent::WorktreeListUpdated {
                    worktrees,
                    open_picker,
                });
            }
            Err(error) => {
                tx.send(AppEvent::WorktreeListUpdateFailed { error, open_picker });
            }
        }
    });
}

pub(crate) fn set_worktree_list(
    chat: &mut ChatWidget,
    worktrees: Vec<GitWorktreeEntry>,
    open_picker: bool,
) {
    chat.worktree_state_set_list(worktrees);

    if open_picker {
        open_worktree_picker(chat);
    }
}

pub(crate) fn on_worktree_list_update_failed(
    chat: &mut ChatWidget,
    error: String,
    open_picker: bool,
) {
    chat.worktree_state_set_error(error);

    if open_picker {
        open_worktree_picker(chat);
    }
}

pub(crate) fn spawn_worktree_init_wizard(chat: &mut ChatWidget) {
    let cwd = chat.session_cwd().to_path_buf();
    let Some(head) = codex_core::git_info::resolve_git_worktree_head(&cwd) else {
        chat.add_error_message(String::from(
            "`/worktree init` — not inside a git worktree (start xcodex in a repo/worktree directory, or switch via `/worktree`)",
        ));
        return;
    };
    let Some(workspace_root) =
        codex_core::git_info::resolve_root_git_project_for_trust(&head.worktree_root)
    else {
        chat.add_error_message(String::from(
            "`/worktree init` — failed to resolve workspace root (the main worktree root for this repo)",
        ));
        return;
    };

    let current_branch =
        codex_core::git_info::read_git_head_state(&head.head_path).and_then(|state| match state {
            GitHeadState::Branch(branch) => Some(branch),
            GitHeadState::Detached => None,
        });
    let shared_dirs = chat.worktrees_shared_dirs().to_vec();
    let tx = chat.app_event_tx();
    let worktree_root = head.worktree_root;
    tokio::spawn(async move {
        let branches = codex_core::git_info::local_git_branches(&cwd).await;
        tx.send(AppEvent::OpenWorktreeInitWizard {
            worktree_root,
            workspace_root,
            current_branch,
            shared_dirs,
            branches,
        });
    });
}

pub(crate) fn spawn_worktree_init_command(
    chat: &mut ChatWidget,
    name: String,
    branch: String,
    path: Option<PathBuf>,
    invoked: String,
) {
    let cwd = chat.session_cwd().to_path_buf();
    let shared_dirs = chat.worktrees_shared_dirs().to_vec();
    let tx = chat.app_event_tx();
    tokio::spawn(async move {
        let Some(current_root) =
            codex_core::git_info::resolve_git_worktree_head(&cwd).map(|head| head.worktree_root)
        else {
            tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(String::from(
                    "`/worktree init` — not inside a git worktree (start xcodex in a repo/worktree directory, or switch via `/worktree`)",
                )),
            )));
            return;
        };

        let Some(workspace_root) =
            codex_core::git_info::resolve_root_git_project_for_trust(&current_root)
        else {
            tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(String::from(
                    "`/worktree init` — failed to resolve workspace root (the main worktree root for this repo)",
                )),
            )));
            return;
        };

        let result = codex_core::git_info::init_git_worktree(
            &workspace_root,
            &name,
            &branch,
            path.as_deref(),
        )
        .await;

        let path = match result {
            Ok(path) => path,
            Err(err) => {
                let resolved_path = if let Some(path) = &path {
                    if path.is_absolute() {
                        path.clone()
                    } else {
                        workspace_root.join(path)
                    }
                } else {
                    workspace_root.join(".worktrees").join(&name)
                };
                let mut lines: Vec<Line<'static>> = Vec::new();
                lines.push(Line::from(format!("error: {err}")));
                lines.push(transcript_spacer_line());
                lines.push(Line::from("Try running this outside xcodex:"));
                lines.push(Line::from(format!(
                    "  git -C {} worktree add -b {} {}",
                    workspace_root.display(),
                    branch,
                    resolved_path.display()
                )));
                lines.push(Line::from(format!(
                    "  git -C {} worktree add {} {}   (if branch already exists)",
                    workspace_root.display(),
                    resolved_path.display(),
                    branch
                )));
                lines.push(Line::from(format!(
                    "  git -C {} worktree list --porcelain",
                    workspace_root.display()
                )));
                let command = PlainHistoryCell::new(vec![Line::from(vec![invoked.magenta()])]);
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    CompositeHistoryCell::new(vec![
                        Box::new(command),
                        Box::new(PlainHistoryCell::new(lines)),
                    ]),
                )));
                return;
            }
        };

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(format!(
            "workspace root: {}",
            workspace_root.display()
        )));
        lines.push(Line::from(format!("created: {}", path.display())));

        if !shared_dirs.is_empty() {
            let actions = codex_core::git_info::link_worktree_shared_dirs(
                &path,
                &workspace_root,
                &shared_dirs,
            )
            .await;

            let mut linked_dirs: Vec<(String, PathBuf)> = Vec::new();
            for action in actions {
                if matches!(
                    action.outcome,
                    codex_core::git_info::SharedDirLinkOutcome::Linked
                        | codex_core::git_info::SharedDirLinkOutcome::AlreadyLinked
                ) {
                    linked_dirs.push((action.shared_dir, action.target_path));
                }
            }

            if !linked_dirs.is_empty() {
                lines.push(transcript_spacer_line());
                lines.push(Line::from("Shared dirs (writes land in workspace root):"));
                for (dir, target) in linked_dirs {
                    lines.push(Line::from(format!("- {dir} -> {}", target.display())));
                }
            }
        }

        let command = PlainHistoryCell::new(vec![Line::from(vec![invoked.magenta()])]);
        tx.send(AppEvent::InsertHistoryCell(Box::new(
            CompositeHistoryCell::new(vec![
                Box::new(command),
                Box::new(PlainHistoryCell::new(lines)),
            ]),
        )));

        tx.send(AppEvent::WorktreeSwitched(path.clone()));
        tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
            cwd: Some(path.clone()),
            approval_policy: None,
            sandbox_policy: None,
            model: None,
            effort: None,
            summary: None,
            collaboration_mode: None,
        }));
        tx.send(AppEvent::CodexOp(Op::ListSkills {
            cwds: vec![path],
            force_reload: true,
        }));
    });
}

pub(crate) fn spawn_worktree_doctor(chat: &mut ChatWidget) {
    let cwd = chat.session_cwd().to_path_buf();
    let shared_dirs = chat.worktrees_shared_dirs().to_vec();
    let tx = chat.app_event_tx();
    tokio::spawn(async move {
        let mut lines = codex_core::git_info::worktree_doctor_lines(&cwd, &shared_dirs, 5).await;
        if lines.first().is_some_and(|line| line == "worktree doctor") {
            lines.remove(0);
        }
        while lines.first().is_some_and(|line| line.trim().is_empty()) {
            lines.remove(0);
        }
        let lines = lines.into_iter().map(Line::from).collect();
        let command = PlainHistoryCell::new(vec![Line::from(vec!["/worktree doctor".magenta()])]);
        tx.send(AppEvent::InsertHistoryCell(Box::new(
            CompositeHistoryCell::new(vec![
                Box::new(command),
                Box::new(PlainHistoryCell::new(lines)),
            ]),
        )));
    });
}

pub(crate) fn open_worktree_picker(chat: &mut ChatWidget) {
    fn build_worktree_picker_rows(
        worktrees: &[GitWorktreeEntry],
        current_root: Option<&Path>,
        workspace_root: Option<&Path>,
    ) -> Vec<WorktreePickerRow> {
        let mut rows: Vec<WorktreePickerRow> = worktrees
            .iter()
            .map(|entry| {
                let display = crate::exec_command::relativize_to_home(&entry.path)
                    .map(|path| {
                        if path.as_os_str().is_empty() {
                            String::from("~")
                        } else {
                            format!("~/{}", path.display())
                        }
                    })
                    .unwrap_or_else(|| entry.path.display().to_string());

                let branch_label = match &entry.head {
                    GitHeadState::Branch(name) => name.clone(),
                    GitHeadState::Detached => String::from("(detached)"),
                };

                let is_current = current_root.is_some_and(|root| root == entry.path.as_path());
                let is_workspace_root = !entry.is_bare
                    && workspace_root.is_some_and(|root| root == entry.path.as_path());

                let mut search_value = branch_label.clone();
                search_value.push(' ');
                search_value.push_str(&display);

                let mut description = format!("branch: {branch_label}");
                if entry.is_bare {
                    description.push_str(" (bare repository)");
                } else if is_workspace_root {
                    description.push_str(" (workspace root)");
                }

                let mut selected_description = description.clone();
                if is_current {
                    selected_description.push_str(" (current session)");
                }

                WorktreePickerRow {
                    path: entry.path.clone(),
                    display,
                    search_value,
                    description,
                    selected_description,
                    is_current,
                    is_workspace_root,
                    is_bare: entry.is_bare,
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            b.is_current
                .cmp(&a.is_current)
                .then_with(|| b.is_workspace_root.cmp(&a.is_workspace_root))
                .then_with(|| a.is_bare.cmp(&b.is_bare))
                .then_with(|| a.path.cmp(&b.path))
        });

        rows
    }

    let mut items: Vec<SelectionItem> = Vec::new();

    items.push(SelectionItem {
        name: "Refresh worktrees".to_string(),
        display_shortcut: Some(key_hint::alt(KeyCode::Char('r'))),
        description: Some("Re-detect worktrees for this session.".to_string()),
        actions: vec![Box::new(|tx: &AppEventSender| {
            tx.send(AppEvent::WorktreeDetect { open_picker: true });
        })],
        dismiss_on_select: true,
        ..Default::default()
    });

    items.push(SelectionItem {
        name: "Worktrees settings…".to_string(),
        display_shortcut: Some(key_hint::alt(KeyCode::Char('s'))),
        description: Some("Edit shared dirs and pinned paths.".to_string()),
        actions: vec![Box::new(|tx: &AppEventSender| {
            tx.send(AppEvent::OpenWorktreesSettingsView);
        })],
        dismiss_on_select: true,
        ..Default::default()
    });

    items.push(SelectionItem {
        name: "Create worktree…".to_string(),
        display_shortcut: Some(key_hint::alt(KeyCode::Char('i'))),
        description: Some("Insert `/worktree init` into the composer.".to_string()),
        actions: vec![Box::new(|tx: &AppEventSender| {
            tx.send(AppEvent::OpenToolsCommand {
                command: "/worktree init ".to_string(),
            });
        })],
        dismiss_on_select: true,
        ..Default::default()
    });

    {
        let cwd = chat.session_cwd().to_path_buf();
        let shared_dirs = chat.worktrees_shared_dirs().to_vec();
        items.push(SelectionItem {
            name: "Worktree doctor".to_string(),
            display_shortcut: Some(key_hint::alt(KeyCode::Char('d'))),
            description: Some("Show shared-dir + untracked status for this worktree.".to_string()),
            actions: vec![Box::new(move |tx: &AppEventSender| {
                let cwd = cwd.clone();
                let shared_dirs = shared_dirs.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let lines =
                        codex_core::git_info::worktree_doctor_lines(&cwd, &shared_dirs, 5).await;
                    let lines = lines.into_iter().map(Line::from).collect();
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        PlainHistoryCell::new(lines),
                    )));
                });
            })],
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    let current_root = codex_core::git_info::resolve_git_worktree_head(chat.session_cwd())
        .map(|head| head.worktree_root);
    let workspace_root = current_root
        .as_ref()
        .and_then(|root| codex_core::git_info::resolve_root_git_project_for_trust(root));

    let rows = build_worktree_picker_rows(
        chat.worktree_list(),
        current_root.as_deref(),
        workspace_root.as_deref(),
    );
    for row in rows {
        let dismiss_on_select = !row.is_bare;
        let actions: Vec<SelectionAction> = if row.is_bare {
            Vec::new()
        } else {
            vec![Box::new({
                let path = row.path.clone();
                move |tx: &AppEventSender| {
                    tx.send(AppEvent::WorktreeSwitched(path.clone()));
                    tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                        cwd: Some(path.clone()),
                        approval_policy: None,
                        sandbox_policy: None,
                        model: None,
                        effort: None,
                        summary: None,
                        collaboration_mode: None,
                    }));
                    tx.send(AppEvent::CodexOp(Op::ListSkills {
                        cwds: vec![path.clone()],
                        force_reload: true,
                    }));
                }
            })]
        };

        items.push(SelectionItem {
            name: row.display,
            description: Some(row.description),
            selected_description: Some(row.selected_description),
            is_current: row.is_current,
            actions,
            dismiss_on_select,
            search_value: Some(row.search_value),
            ..Default::default()
        });
    }

    let subtitle = if let Some(err) = chat.worktree_list_error() {
        Some(format!("Failed to detect worktrees: {err}"))
    } else if chat.worktree_list_is_empty() {
        Some("No worktrees detected for this session.".to_string())
    } else {
        None
    };

    chat.show_selection_view(SelectionViewParams {
        title: Some("Select a worktree".to_string()),
        subtitle,
        footer_hint: Some(Line::from(vec![
            key_hint::plain(KeyCode::Up).into(),
            "/".into(),
            key_hint::plain(KeyCode::Down).into(),
            " select  ".dim(),
            key_hint::plain(KeyCode::Enter).into(),
            " open  ".dim(),
            key_hint::alt(KeyCode::Char('r')).into(),
            " refresh  ".dim(),
            key_hint::alt(KeyCode::Char('s')).into(),
            " settings  ".dim(),
            key_hint::alt(KeyCode::Char('i')).into(),
            " create  ".dim(),
            key_hint::alt(KeyCode::Char('d')).into(),
            " doctor  ".dim(),
            "type to search  ".dim(),
            key_hint::plain(KeyCode::Esc).into(),
            " close".dim(),
        ])),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search worktrees".to_string()),
        ..Default::default()
    });
}

#[derive(Clone)]
struct WorktreePickerRow {
    path: PathBuf,
    display: String,
    search_value: String,
    description: String,
    selected_description: String,
    is_current: bool,
    is_workspace_root: bool,
    is_bare: bool,
}
