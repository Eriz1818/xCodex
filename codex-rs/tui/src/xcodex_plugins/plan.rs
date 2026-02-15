use crate::app_event::AppEvent;
use crate::app_event::PlanFixAndStartAction;
use crate::app_event::PlanSettingsCycleTarget;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::ChatComposerConfig;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::TabState;
use crate::chatwidget::ChatWidget;
use crate::render::renderable::Renderable;
use crate::slash_command::SlashCommand;
use crate::style::user_message_style;
use codex_core::git_info::get_git_repo_root;
use codex_core::plan_file;
use codex_file_search::FileMatch;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::openai_models::ModelPreset;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

use super::PluginSubcommandHintOrder;
use super::PluginSubcommandNode;
use super::PluginSubcommandRoot;

const PLAN_LIST_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "open",
        full_name: "plan list open",
        description: "show open plans (Draft/Active/Paused)",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "closed",
        full_name: "plan list closed",
        description: "show closed plans (Done)",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "all",
        full_name: "plan list all",
        description: "show non-archived plans",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "archived",
        full_name: "plan list archived",
        description: "show archived plans only",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_BRAINSTORM_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "auto",
        full_name: "plan settings brainstorm-first auto",
        description: "brainstorm prompt follows model hints",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "always",
        full_name: "plan settings brainstorm-first always",
        description: "always ask brainstorm-first question",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "never",
        full_name: "plan settings brainstorm-first never",
        description: "skip brainstorm-first question",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_FLOWCHART_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "auto",
        full_name: "plan settings flowchart auto",
        description: "flowchart output follows model hints",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "prefer-text",
        full_name: "plan settings flowchart prefer-text",
        description: "prefer text over diagrams",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "prefer-diagrams",
        full_name: "plan settings flowchart prefer-diagrams",
        description: "prefer diagrams over text",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_MISMATCH_ACTION_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "warn",
        full_name: "plan settings mismatch-action warn",
        description: "show alert but allow continue",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "block",
        full_name: "plan settings mismatch-action block",
        description: "block implementation actions until resolved",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_NAMING_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "funny",
        full_name: "plan settings naming funny",
        description: "use verb-adjective-animal naming",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "date-title",
        full_name: "plan settings naming date-title",
        description: "use yyyy-mm-dd-title naming",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_MODE_CUSTOM_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "default",
        full_name: "plan settings mode custom default",
        description: "initialize custom template from default and select custom mode",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "adr-lite",
        full_name: "plan settings mode custom adr-lite",
        description: "initialize custom template from adr-lite and select custom mode",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_MODE_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "default",
        full_name: "plan settings mode default",
        description: "use upstream-aligned plan workflow defaults",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "adr-lite",
        full_name: "plan settings mode adr-lite",
        description: "use adr-lite workflow defaults",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "custom",
        full_name: "plan settings mode custom",
        description: "use a user-managed custom workflow template (optional seed)",
        run_on_enter: true,
        insert_trailing_space: true,
        children: PLAN_MODE_CUSTOM_CHILDREN,
    },
];

const PLAN_MODEL_CHILDREN: &[PluginSubcommandNode] = &[PluginSubcommandNode {
    token: "inherit",
    full_name: "plan settings model inherit",
    description: "inherit the default/global model in plan mode",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const PLAN_CUSTOM_TEMPLATE_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "view",
        full_name: "plan settings custom-template view",
        description: "show custom template path",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "open",
        full_name: "plan settings custom-template open",
        description: "open custom template in external editor",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "init",
        full_name: "plan settings custom-template init",
        description: "initialize custom template from a built-in mode",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[
            PluginSubcommandNode {
                token: "default",
                full_name: "plan settings custom-template init default",
                description: "seed custom template from default mode template",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
            PluginSubcommandNode {
                token: "adr-lite",
                full_name: "plan settings custom-template init adr-lite",
                description: "seed custom template from adr-lite mode template",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
        ],
    },
];

const PLAN_CURRENT_PATH_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "view",
        full_name: "plan settings current-path view",
        description: "show active plan file path",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "copy",
        full_name: "plan settings current-path copy",
        description: "copy active plan file path",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "open",
        full_name: "plan settings current-path open",
        description: "open active plan file in external editor",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
];

const PLAN_SETTINGS_CHILDREN: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "base-dir",
        full_name: "plan settings base-dir",
        description: "show or set default plan base directory",
        run_on_enter: true,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "current-path",
        full_name: "plan settings current-path",
        description: "show/copy/open current active plan path",
        run_on_enter: true,
        insert_trailing_space: false,
        children: PLAN_CURRENT_PATH_CHILDREN,
    },
    PluginSubcommandNode {
        token: "brainstorm-first",
        full_name: "plan settings brainstorm-first",
        description: "set brainstorm-first policy",
        run_on_enter: false,
        insert_trailing_space: true,
        children: PLAN_BRAINSTORM_CHILDREN,
    },
    PluginSubcommandNode {
        token: "flowchart",
        full_name: "plan settings flowchart",
        description: "set flowchart preference",
        run_on_enter: false,
        insert_trailing_space: true,
        children: PLAN_FLOWCHART_CHILDREN,
    },
    PluginSubcommandNode {
        token: "track-worktree",
        full_name: "plan settings track-worktree",
        description: "enable/disable worktree tracking",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[
            PluginSubcommandNode {
                token: "on",
                full_name: "plan settings track-worktree on",
                description: "track plan worktree context",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
            PluginSubcommandNode {
                token: "off",
                full_name: "plan settings track-worktree off",
                description: "do not track plan worktree context",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
        ],
    },
    PluginSubcommandNode {
        token: "track-branch",
        full_name: "plan settings track-branch",
        description: "enable/disable branch tracking",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[
            PluginSubcommandNode {
                token: "on",
                full_name: "plan settings track-branch on",
                description: "track plan branch context",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
            PluginSubcommandNode {
                token: "off",
                full_name: "plan settings track-branch off",
                description: "do not track plan branch context",
                run_on_enter: true,
                insert_trailing_space: false,
                children: &[],
            },
        ],
    },
    PluginSubcommandNode {
        token: "mismatch-action",
        full_name: "plan settings mismatch-action",
        description: "set behavior when tracked context mismatches",
        run_on_enter: false,
        insert_trailing_space: true,
        children: PLAN_MISMATCH_ACTION_CHILDREN,
    },
    PluginSubcommandNode {
        token: "naming",
        full_name: "plan settings naming",
        description: "set autogenerated plan filename strategy",
        run_on_enter: false,
        insert_trailing_space: true,
        children: PLAN_NAMING_CHILDREN,
    },
    PluginSubcommandNode {
        token: "mode",
        full_name: "plan settings mode",
        description: "set plan workflow mode",
        run_on_enter: false,
        insert_trailing_space: true,
        children: PLAN_MODE_CHILDREN,
    },
    PluginSubcommandNode {
        token: "model",
        full_name: "plan settings model",
        description: "set model override for plan mode",
        run_on_enter: true,
        insert_trailing_space: true,
        children: PLAN_MODEL_CHILDREN,
    },
    PluginSubcommandNode {
        token: "custom-template",
        full_name: "plan settings custom-template",
        description: "manage custom plan workflow template",
        run_on_enter: true,
        insert_trailing_space: true,
        children: PLAN_CUSTOM_TEMPLATE_CHILDREN,
    },
];

const PLAN_SUBCOMMANDS: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "list",
        full_name: "plan list",
        description: "open plan list popup",
        run_on_enter: true,
        insert_trailing_space: true,
        children: PLAN_LIST_CHILDREN,
    },
    PluginSubcommandNode {
        token: "open",
        full_name: "plan open",
        description: "open or create the active plan file",
        run_on_enter: true,
        insert_trailing_space: true,
        children: &[],
    },
    PluginSubcommandNode {
        token: "status",
        full_name: "plan status",
        description: "show current plan status and settings",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "done",
        full_name: "plan done",
        description: "mark the active plan file as done",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "archive",
        full_name: "plan archive",
        description: "mark the active plan file as archived",
        run_on_enter: true,
        insert_trailing_space: false,
        children: &[],
    },
    PluginSubcommandNode {
        token: "settings",
        full_name: "plan settings",
        description: "open plan mode settings",
        run_on_enter: true,
        insert_trailing_space: true,
        children: PLAN_SETTINGS_CHILDREN,
    },
];

const PLAN_HINT_ORDER: &[PluginSubcommandHintOrder] = &[
    PluginSubcommandHintOrder {
        token: "list",
        order: 0,
    },
    PluginSubcommandHintOrder {
        token: "open",
        order: 1,
    },
    PluginSubcommandHintOrder {
        token: "status",
        order: 2,
    },
    PluginSubcommandHintOrder {
        token: "settings",
        order: 3,
    },
    PluginSubcommandHintOrder {
        token: "done",
        order: 4,
    },
    PluginSubcommandHintOrder {
        token: "archive",
        order: 5,
    },
];

pub(crate) const PLAN_SUBCOMMAND_ROOT: PluginSubcommandRoot = PluginSubcommandRoot {
    root: "plan",
    anchor: SlashCommand::Plan,
    children: PLAN_SUBCOMMANDS,
    list_hint_order: Some(PLAN_HINT_ORDER),
};

const PLAN_BASE_DIR_FILE: &str = ".base-dir";
const PLAN_GITIGNORE_PROMPTED_FILE: &str = ".gitignore-prompted-base-dirs.json";
const BRAINSTORM_PREF_FILE: &str = ".brainstorm-first-pref";
const FLOWCHART_PREF_FILE: &str = ".flowchart-pref";
const PLAN_MODE_FILE: &str = ".mode";
const PLAN_CUSTOM_TEMPLATE_FILE: &str = ".custom-template";
const PLAN_CUSTOM_SEED_MODE_FILE: &str = ".custom-seed-mode";
const PLAN_TRACK_WORKTREE_FILE: &str = ".track-worktree";
const PLAN_TRACK_BRANCH_FILE: &str = ".track-branch";
const PLAN_MISMATCH_ACTION_FILE: &str = ".mismatch-action";
const PLAN_NAMING_STRATEGY_FILE: &str = ".naming-strategy";
const PLAN_MODEL_FILE: &str = ".model";
const ACTIVE_PLAN_FILE: &str = ".active-plan";
const ACTIVE_PLAN_BY_THREAD_FILE: &str = ".active-plan-by-thread.json";

const DEFAULT_BRAINSTORM_PREF: &str = "auto";
const DEFAULT_FLOWCHART_PREF: &str = "auto";
const DEFAULT_PLAN_MODE: &str = "default";
const PLAN_MODE_DEFAULT: &str = "default";
const PLAN_MODE_ADR_LITE: &str = "adr-lite";
const PLAN_MODE_CUSTOM: &str = "custom";
const PLAN_FILE_MODE_FIELD: &str = "Mode:";
const PLAN_FILE_SYNC_MODE_FIELD: &str = "Sync mode:";
const PLAN_MISMATCH_ACTION_WARN: &str = "warn";
const PLAN_MISMATCH_ACTION_BLOCK: &str = "block";
const PLAN_NAMING_FUNNY: &str = "funny";
const PLAN_NAMING_DATE_TITLE: &str = "date-title";

const OPEN_STATUSES: &[&str] = &["Draft", "Active", "Paused"];
const CLOSED_STATUSES: &[&str] = &["Done"];
const ARCHIVED_STATUSES: &[&str] = &["Archived"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanUiUpdate {
    pub(crate) path: PathBuf,
    pub(crate) todos_remaining: usize,
    pub(crate) is_done: bool,
}

pub(crate) fn handle_plan_command(chat: &mut ChatWidget, rest: &str) {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        open_plan_menu(chat);
        return;
    }
    if try_handle_subcommand(chat, trimmed) {
        return;
    }
    chat.add_info_message(
        "Usage: /plan [list|open|status|done|archive|settings]".to_string(),
        Some("Try `/plan` to open the plan list.".to_string()),
    );
}

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &str) -> bool {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        open_plan_menu(chat);
        return true;
    }
    if trimmed == "list" {
        show_plan_list(chat, PlanListScope::Open);
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("list ") {
        let Some(scope) = PlanListScope::parse(rest.trim()) else {
            chat.add_error_message("Usage: /plan list [open|closed|all|archived]".to_string());
            return true;
        };
        show_plan_list(chat, scope);
        return true;
    }
    if trimmed == "open" {
        open_plan_file(chat, None);
        return true;
    }
    if let Some(path_arg) = trimmed.strip_prefix("open ") {
        open_plan_file(chat, Some(path_arg));
        return true;
    }
    if trimmed == "status" {
        show_plan_status(chat);
        return true;
    }
    if trimmed == "done" {
        open_plan_done_confirmation(chat);
        return true;
    }
    if trimmed == "archive" {
        open_plan_archive_confirmation(chat);
        return true;
    }
    if trimmed == "settings" {
        open_plan_settings(chat);
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("settings ") {
        handle_settings_command(chat, rest);
        return true;
    }
    false
}

pub(crate) fn open_plan_menu(chat: &mut ChatWidget) {
    show_plan_list(chat, PlanListScope::Open);
}

pub(crate) fn open_plan_list_scope(chat: &mut ChatWidget, scope: &str) {
    let scope = PlanListScope::parse(scope).unwrap_or(PlanListScope::Open);
    show_plan_list(chat, scope);
}

pub(crate) fn open_plan_settings_menu(chat: &mut ChatWidget) {
    open_plan_settings(chat);
}

pub(crate) fn open_plan_file_path(
    chat: &mut ChatWidget,
    path: Option<PathBuf>,
) -> Option<PlanUiUpdate> {
    if let Some(path) = path {
        let value = path.display().to_string();
        open_plan_file(chat, Some(&value));
        return active_plan_ui_update(chat);
    }
    open_plan_file(chat, None);
    active_plan_ui_update(chat)
}

pub(crate) fn open_plan_load_confirmation(chat: &mut ChatWidget, path: PathBuf, scope: &str) {
    let scope = PlanListScope::parse(scope).unwrap_or(PlanListScope::Open);
    let path_display = path.display().to_string();
    let load_path = path;
    let cancel_scope = scope.token().to_string();
    chat.show_selection_view(SelectionViewParams {
        title: Some("Load selected plan?".to_string()),
        subtitle: Some(path_display.clone()),
        footer_note: Some(Line::from(
            "Choose `Load` to set this as the active plan file.",
        )),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": confirm"),
            ("Esc", ": cancel"),
        ])),
        items: vec![
            SelectionItem {
                name: "Load".to_string(),
                description: Some(format!("Set active plan to {path_display}")),
                actions: vec![Box::new(move |sender| {
                    sender.send(AppEvent::OpenPlanFile {
                        path: Some(load_path.clone()),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Return to plan list".to_string()),
                actions: vec![Box::new(move |sender| {
                    sender.send(AppEvent::OpenPlanListView {
                        scope: cancel_scope.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

pub(crate) fn mark_active_plan_done_action(chat: &mut ChatWidget) -> Option<PlanUiUpdate> {
    mark_active_plan_done(chat);
    active_plan_ui_update(chat)
}

pub(crate) fn mark_active_plan_archived_action(chat: &mut ChatWidget) -> Option<PlanUiUpdate> {
    mark_active_plan_archived(chat);
    active_plan_ui_update(chat)
}

pub(crate) fn pause_active_plan_run_action(chat: &mut ChatWidget) -> Option<PlanUiUpdate> {
    pause_active_plan_run(chat);
    active_plan_ui_update(chat)
}

pub(crate) fn sync_active_plan_turn_end(
    chat: &mut ChatWidget,
    last_agent_message: Option<&str>,
    turn_proposed_plan_text: Option<&str>,
) -> Option<PlanUiUpdate> {
    let Some(path) = read_active_plan_path(chat) else {
        return None;
    };
    if !path.exists() {
        return None;
    }

    let today = chrono::Local::now().date_naive().to_string();
    let note = checkpoint_note(last_agent_message);
    if let Err(err) = ensure_plan_mode_lock(chat, &path) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            "failed to ensure plan mode lock metadata before turn-end sync"
        );
    }
    let sync_result = if uses_adr_lite_sync(chat, Some(&path)) {
        let worktree = chat.session_cwd().display().to_string();
        let branch = current_branch_name(chat);
        plan_file::sync_turn_end_adr_lite_with_content(
            &path,
            &today,
            &note,
            &worktree,
            &branch,
            last_agent_message,
            turn_proposed_plan_text,
        )
    } else {
        plan_file::sync_turn_end_with_content(
            &path,
            &today,
            &note,
            last_agent_message,
            turn_proposed_plan_text,
        )
    };
    if let Err(err) = sync_result {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            "failed to sync active plan file on turn end"
        );
    }
    active_plan_ui_update(chat)
}

pub(crate) fn sync_active_plan_session_state(chat: &mut ChatWidget) -> Option<PlanUiUpdate> {
    bind_active_plan_to_thread(chat);
    let active_path = read_active_plan_path(chat);
    if let Some(path) = active_path
        && path.exists()
    {
        if let Err(err) = ensure_plan_mode_lock(chat, &path) {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "failed to ensure plan mode lock metadata during session recovery"
            );
        }
        if uses_adr_lite_sync(chat, Some(&path)) {
            let today = chrono::Local::now().date_naive().to_string();
            let worktree = chat.session_cwd().display().to_string();
            let branch = current_branch_name(chat);
            if let Err(err) = plan_file::sync_adr_lite_open_or_resume(
                &path,
                &today,
                &worktree,
                &branch,
                "session recovery sync",
            ) {
                tracing::warn!(
                    error = %err,
                    path = %path.display(),
                    "failed adr-lite open/resume sync for active plan file"
                );
            }
        }
        maybe_notify_plan_context_mismatch(chat, &path, "session recovery");
    }
    active_plan_ui_update(chat)
}

pub(crate) fn open_post_plan_prompt(
    chat: &mut ChatWidget,
    default_mode_mask: Option<CollaborationModeMask>,
) {
    let context_block_reason = plan_start_implementation_block_reason(chat);
    let (mut start_actions, mut start_disabled_reason) = match default_mode_mask.clone() {
        Some(mask) => {
            let user_text = "Implement the plan.".to_string();
            let actions: Vec<SelectionAction> = vec![Box::new(move |sender| {
                sender.send(AppEvent::SubmitUserMessageWithMode {
                    text: user_text.clone(),
                    collaboration_mode: mask.clone(),
                });
            })];
            (actions, None)
        }
        None => (Vec::new(), Some("Default mode unavailable".to_string())),
    };
    if start_disabled_reason.is_none() {
        start_disabled_reason = context_block_reason.clone();
    }
    if start_disabled_reason.is_some() {
        start_actions.clear();
    }

    let mut items: Vec<SelectionItem> = Vec::new();
    if context_block_reason.is_some() {
        let fix_default_mask = default_mode_mask.clone();
        let (fix_actions, fix_disabled_reason) = if fix_default_mask.is_some() {
            let actions: Vec<SelectionAction> = vec![Box::new(move |sender| {
                sender.send(AppEvent::OpenPlanFixAndStartPrompt {
                    default_mode_mask: fix_default_mask.clone(),
                });
            })];
            (actions, None)
        } else {
            (Vec::new(), Some("Default mode unavailable".to_string()))
        };
        items.push(SelectionItem {
            name: "Fix and Start...".to_string(),
            description: Some(
                "Compare plan vs current worktree/branch, choose resolution, then start."
                    .to_string(),
            ),
            actions: fix_actions,
            disabled_reason: fix_disabled_reason,
            dismiss_on_select: true,
            ..Default::default()
        });
    }
    items.push(SelectionItem {
        name: "Start Implementation".to_string(),
        description: Some("Switch to Default mode and start coding.".to_string()),
        actions: start_actions,
        disabled_reason: start_disabled_reason,
        dismiss_on_select: true,
        ..Default::default()
    });
    items.push(SelectionItem {
        name: "Discuss Further".to_string(),
        description: Some("Stay in Plan mode and continue refining.".to_string()),
        actions: vec![Box::new(|sender| {
            sender.send(AppEvent::ReopenPlanNextStepPromptAfterTurn)
        })],
        dismiss_on_select: true,
        ..Default::default()
    });
    items.push(SelectionItem {
        name: "Cancel".to_string(),
        description: Some("Pause this plan run and keep the plan file.".to_string()),
        actions: vec![Box::new(|sender| sender.send(AppEvent::PauseActivePlanRun))],
        dismiss_on_select: true,
        ..Default::default()
    });
    items.push(SelectionItem {
        name: "Do something else...".to_string(),
        description: Some("Enter a one-line prompt and send it immediately.".to_string()),
        actions: vec![Box::new(|sender| {
            sender.send(AppEvent::OpenPlanDoSomethingElsePrompt)
        })],
        dismiss_on_select: true,
        ..Default::default()
    });

    chat.show_selection_view(SelectionViewParams {
        title: Some("Plan ready: choose next step".to_string()),
        subtitle: Some("Select how to proceed from Plan mode.".to_string()),
        footer_note: Some(Line::from(
            "Cancel keeps the plan file and sets status to `Paused`.",
        )),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": confirm"),
            ("Esc", ": close"),
        ])),
        undim_footer_hint: true,
        items,
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

pub(crate) fn open_plan_fix_and_start_prompt(
    chat: &mut ChatWidget,
    default_mode_mask: Option<CollaborationModeMask>,
) {
    let Some(mask) = default_mode_mask else {
        chat.add_error_message("Default mode unavailable for Start Implementation.".to_string());
        return;
    };
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
        return;
    };
    if !path.exists() {
        chat.add_error_message(format!(
            "Active plan file does not exist: `{}`",
            path.display(),
        ));
        return;
    }

    let context = plan_context_snapshot(chat, &path);
    let plan_worktree = context.plan_worktree.as_deref().unwrap_or("(missing)");
    let plan_branch = context.plan_branch.as_deref().unwrap_or("(missing)");
    let current_worktree = context.current_worktree.as_str();
    let current_branch = context.current_branch.as_str();
    let context_summary = format!(
        "plan wt=`{plan_worktree}` / br=`{plan_branch}`; current wt=`{current_worktree}` / br=`{current_branch}`"
    );
    let update_mask = mask.clone();
    let keep_mask = mask.clone();
    chat.show_selection_view(SelectionViewParams {
        title: Some("Fix context mismatch and start".to_string()),
        subtitle: Some(context_summary),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": confirm"),
            ("Esc", ": close"),
        ])),
        undim_footer_hint: true,
        items: vec![
            SelectionItem {
                name: "Use Current Context and Start".to_string(),
                description: Some(
                    "Update plan `Worktree:`/`Branch:` to current session values, then start."
                        .to_string(),
                ),
                actions: vec![Box::new(move |sender| {
                    sender.send(AppEvent::ResolvePlanContextMismatchAndStart {
                        action: PlanFixAndStartAction::UpdatePlanContextAndStart,
                        collaboration_mode: update_mask.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep Plan Context, Start Anyway".to_string(),
                description: Some(
                    "Do not modify plan metadata; start implementation in current context once."
                        .to_string(),
                ),
                actions: vec![Box::new(move |sender| {
                    sender.send(AppEvent::ResolvePlanContextMismatchAndStart {
                        action: PlanFixAndStartAction::StartWithoutContextChange,
                        collaboration_mode: keep_mask.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Return without starting implementation.".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

pub(crate) fn resolve_plan_context_mismatch_and_start_action(
    chat: &mut ChatWidget,
    action: PlanFixAndStartAction,
    collaboration_mode: CollaborationModeMask,
) -> Option<PlanUiUpdate> {
    let mut ui_update = None;
    if action == PlanFixAndStartAction::UpdatePlanContextAndStart {
        let Some(path) = read_active_plan_path(chat) else {
            chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
            return None;
        };
        if !path.exists() {
            chat.add_error_message(format!(
                "Active plan file does not exist: `{}`",
                path.display(),
            ));
            return None;
        }
        let current_worktree = chat.session_cwd().display().to_string();
        let current_branch = current_branch_name(chat);
        if let Err(err) = upsert_plan_metadata_value(&path, "Worktree:", &current_worktree) {
            chat.add_error_message(format!("Failed to update plan worktree metadata: {err}"));
            return None;
        }
        if let Err(err) = upsert_plan_metadata_value(&path, "Branch:", &current_branch) {
            chat.add_error_message(format!("Failed to update plan branch metadata: {err}"));
            return None;
        }
        chat.add_info_message(
            format!(
                "Updated plan context metadata to current worktree/branch: `{}`.",
                path.display()
            ),
            None,
        );
        ui_update = active_plan_ui_update(chat);
    }

    chat.submit_user_message_with_mode("Implement the plan.".to_string(), collaboration_mode);
    ui_update
}

fn handle_settings_command(chat: &mut ChatWidget, rest: &str) {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        open_plan_settings(chat);
        return;
    }

    if trimmed == "base-dir" {
        open_plan_settings_with_selected(chat, Some(0), false);
        open_plan_base_dir_editor(chat);
        return;
    }
    if let Some(value) = trimmed.strip_prefix("base-dir ") {
        set_plan_base_dir(chat, value);
        return;
    }

    if trimmed == "current-path" || trimmed == "current-path view" {
        show_current_plan_path(chat);
        return;
    }
    if trimmed == "current-path copy" {
        copy_current_plan_path(chat);
        return;
    }
    if trimmed == "current-path open" {
        open_current_plan_path(chat);
        return;
    }

    if trimmed == "custom-template" || trimmed == "custom-template view" {
        show_custom_template_path(chat);
        return;
    }
    if trimmed == "custom-template open" {
        open_custom_template_path(chat);
        return;
    }
    if let Some(value) = trimmed.strip_prefix("custom-template init ") {
        init_custom_template(chat, value.trim());
        return;
    }

    if trimmed == "brainstorm-first" {
        let brainstorm = read_brainstorm_pref(chat);
        chat.add_info_message(
            format!(
                "Brainstorm-first preference: `{brainstorm}`\nUsage: /plan settings brainstorm-first <auto|always|never>",
            ),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("brainstorm-first ") {
        set_brainstorm_pref(chat, value.trim(), true);
        return;
    }

    if trimmed == "flowchart" {
        let flowchart = read_flowchart_pref(chat);
        chat.add_info_message(
            format!(
                "Flowchart preference: `{flowchart}`\nUsage: /plan settings flowchart <auto|prefer-text|prefer-diagrams>",
            ),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("flowchart ") {
        set_flowchart_pref(chat, value.trim(), true);
        return;
    }

    if trimmed == "track-worktree" {
        let current = bool_setting_label(read_plan_track_worktree(chat));
        chat.add_info_message(
            format!("Track worktree: `{current}`\nUsage: /plan settings track-worktree <on|off>",),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("track-worktree ") {
        set_plan_track_worktree(chat, value.trim(), true);
        return;
    }

    if trimmed == "track-branch" {
        let current = bool_setting_label(read_plan_track_branch(chat));
        chat.add_info_message(
            format!("Track branch: `{current}`\nUsage: /plan settings track-branch <on|off>",),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("track-branch ") {
        set_plan_track_branch(chat, value.trim(), true);
        return;
    }

    if trimmed == "mismatch-action" {
        let action = read_plan_mismatch_action(chat);
        chat.add_info_message(
            format!(
                "Mismatch action: `{action}`\nUsage: /plan settings mismatch-action <warn|block>",
            ),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("mismatch-action ") {
        set_plan_mismatch_action(chat, value.trim(), true);
        return;
    }

    if trimmed == "naming" {
        let naming = read_plan_naming_strategy(chat);
        chat.add_info_message(
            format!(
                "Plan naming strategy: `{naming}`\nUsage: /plan settings naming <funny|date-title>",
            ),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("naming ") {
        set_plan_naming_strategy(chat, value.trim(), true);
        return;
    }

    if trimmed == "mode" {
        let mode = read_plan_mode(chat);
        chat.add_info_message(
            format!(
                "Plan mode: `{mode}`\nUsage: /plan settings mode <default|adr-lite|custom [default|adr-lite]>",
            ),
            None,
        );
        return;
    }
    if trimmed == "mode custom-setup" {
        open_plan_custom_mode_setup(chat);
        return;
    }
    if let Some(value) = trimmed.strip_prefix("mode ") {
        set_plan_mode(chat, value.trim(), true);
        return;
    }

    if trimmed == "model" {
        let model = plan_mode_model_override(chat).unwrap_or_else(|| "inherit".to_string());
        chat.add_info_message(
            format!(
                "Plan mode model: `{model}`\nUsage: /plan settings model <inherit|model_slug>",
            ),
            None,
        );
        return;
    }
    if let Some(value) = trimmed.strip_prefix("model ") {
        set_plan_model(chat, value.trim(), true);
        return;
    }

    chat.add_error_message(
        "Usage: /plan settings [base-dir|current-path|custom-template|brainstorm-first|flowchart|track-worktree|track-branch|mismatch-action|naming|mode|model]".to_string(),
    );
}

fn open_plan_settings(chat: &mut ChatWidget) {
    open_plan_settings_with_selected(chat, None, false);
}

fn open_plan_settings_with_selected(
    chat: &mut ChatWidget,
    selected_idx: Option<usize>,
    replace_existing: bool,
) {
    let base_dir = plan_base_dir(chat);
    let active_plan = read_active_plan_path(chat);
    let brainstorm = read_brainstorm_pref(chat);
    let flowchart = read_flowchart_pref(chat);
    let track_worktree = read_plan_track_worktree(chat);
    let track_branch = read_plan_track_branch(chat);
    let mismatch_action = read_plan_mismatch_action(chat);
    let naming_strategy = read_plan_naming_strategy(chat);
    let mode = read_plan_mode(chat);
    let plan_model = plan_mode_model_override(chat);
    let plan_model_label = plan_mode_model_display_label(chat, plan_model.as_deref());
    let mode_display = format_mode_for_display(&mode);
    let mode_is_custom = mode == PLAN_MODE_CUSTOM;
    let mode_hint = if mode_is_custom {
        "Tab: cycle. Press Enter for custom mode setup."
    } else {
        "Tab: cycle. Press Enter to open mode picker."
    };
    let mode_enter_actions = if mode_is_custom {
        vec![apply_plan_settings_action(
            "mode custom-setup".to_string(),
            false,
        )]
    } else {
        vec![open_plan_mode_picker_action()]
    };
    let mode_dismiss_on_enter = !mode_is_custom;
    let mut items = vec![
        SelectionItem {
            name: "Default plan base directory".to_string(),
            selected_description: Some(base_dir.display().to_string()),
            actions: vec![open_plan_base_dir_editor_action()],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Brainstorm-first: {brainstorm}"),
            selected_description: Some("Tab/Enter: cycle auto | always | never".to_string()),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::BrainstormFirst,
                1,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::BrainstormFirst,
                1,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Flowchart preference: {flowchart}"),
            selected_description: Some(
                "Tab/Enter: cycle auto | prefer-text | prefer-diagrams".to_string(),
            ),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::Flowchart,
                2,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::Flowchart,
                2,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Track worktree: {}", bool_setting_label(track_worktree)),
            selected_description: Some("Tab/Enter: toggle on | off".to_string()),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::TrackWorktree,
                3,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::TrackWorktree,
                3,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Track branch: {}", bool_setting_label(track_branch)),
            selected_description: Some("Tab/Enter: toggle on | off".to_string()),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::TrackBranch,
                4,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::TrackBranch,
                4,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Context mismatch action: {mismatch_action}"),
            selected_description: Some("Tab/Enter: cycle block | warn".to_string()),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::MismatchAction,
                5,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::MismatchAction,
                5,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Plan naming strategy: {naming_strategy}"),
            selected_description: Some("Tab/Enter: cycle funny | date-title".to_string()),
            actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::Naming,
                6,
            )],
            tab_actions: vec![cycle_plan_settings_action(
                PlanSettingsCycleTarget::Naming,
                6,
            )],
            dismiss_on_select: false,
            ..Default::default()
        },
        SelectionItem {
            name: format!("Plan mode: {mode_display}"),
            selected_description: Some(mode_hint.to_string()),
            actions: mode_enter_actions,
            tab_actions: vec![cycle_plan_settings_action(PlanSettingsCycleTarget::Mode, 7)],
            dismiss_on_select: mode_dismiss_on_enter,
            ..Default::default()
        },
    ];
    if mode_is_custom {
        items.push(SelectionItem {
            name: "Custom workflow template".to_string(),
            selected_description: Some(custom_template_path(chat).display().to_string()),
            actions: vec![apply_plan_settings_action(
                "custom-template view".to_string(),
                true,
            )],
            dismiss_on_select: true,
            ..Default::default()
        });
        items.push(SelectionItem {
            name: "Edit custom workflow template".to_string(),
            selected_description: Some("Open template in external editor".to_string()),
            actions: vec![apply_plan_settings_action(
                "custom-template open".to_string(),
                true,
            )],
            dismiss_on_select: true,
            ..Default::default()
        });
    }

    items.push(SelectionItem {
        name: format!("Plan mode model: {plan_model_label}"),
        selected_description: Some("Press Enter to choose model override".to_string()),
        actions: vec![open_plan_model_picker_action()],
        dismiss_on_select: false,
        ..Default::default()
    });

    if let Some(path) = active_plan {
        let path_display = path.display().to_string();
        items.push(SelectionItem {
            name: "Current plan file path".to_string(),
            selected_description: Some(path_display.clone()),
            actions: vec![apply_plan_settings_action(
                "current-path view".to_string(),
                true,
            )],
            dismiss_on_select: true,
            ..Default::default()
        });
        items.push(SelectionItem {
            name: "Copy current plan path".to_string(),
            selected_description: Some(path_display.clone()),
            actions: vec![apply_plan_settings_action(
                "current-path copy".to_string(),
                true,
            )],
            dismiss_on_select: true,
            ..Default::default()
        });
        items.push(SelectionItem {
            name: "Edit current plan file".to_string(),
            selected_description: Some(path_display),
            actions: vec![apply_plan_settings_action(
                "current-path open".to_string(),
                true,
            )],
            dismiss_on_select: true,
            ..Default::default()
        });
    } else {
        items.push(disabled_item(
            "Current plan file path".to_string(),
            Some("(none)".to_string()),
            "No active plan file",
        ));
        items.push(disabled_item(
            "Copy current plan path".to_string(),
            Some("Run `/plan open` to create/select a plan".to_string()),
            "No active plan file",
        ));
        items.push(disabled_item(
            "Edit current plan file".to_string(),
            Some("Run `/plan open` to create/select a plan".to_string()),
            "No active plan file",
        ));
    }

    let params = SelectionViewParams {
        title: Some("Plan settings".to_string()),
        subtitle: Some("Manage plan mode defaults".to_string()),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": apply"),
            ("Tab", ": cycle selected"),
            ("type", ": search"),
            ("Esc", ": close"),
        ])),
        undim_footer_hint: true,
        selected_item_footer_note: false,
        items,
        is_searchable: true,
        search_placeholder: Some("Search plan settings".to_string()),
        initial_selected_idx: selected_idx,
        ..Default::default()
    };
    if replace_existing {
        chat.show_or_replace_selection_view(params);
    } else {
        chat.show_selection_view(params);
    }
}

pub(crate) fn open_plan_base_dir_editor(chat: &mut ChatWidget) {
    let initial_path = plan_base_dir(chat).display().to_string();
    let view = PlanBaseDirEditorView::new(chat.app_event_tx(), false, initial_path);
    chat.show_view(Box::new(view));
}

pub(crate) fn open_plan_mode_picker(chat: &mut ChatWidget) {
    let mode = read_plan_mode(chat);
    chat.show_selection_view(SelectionViewParams {
        title: Some("Plan mode".to_string()),
        subtitle: Some("Choose the default workflow mode".to_string()),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": apply"),
            ("Esc", ": back"),
        ])),
        undim_footer_hint: true,
        selected_item_footer_note: false,
        items: vec![
            SelectionItem {
                name: "Default".to_string(),
                selected_description: Some("Use standard planning workflow".to_string()),
                is_current: mode == PLAN_MODE_DEFAULT,
                actions: vec![apply_plan_settings_action("mode default".to_string(), true)],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "ADR-lite".to_string(),
                selected_description: Some("Use repo-local ADR-lite workflow defaults".to_string()),
                is_current: mode == PLAN_MODE_ADR_LITE,
                actions: vec![apply_plan_settings_action(
                    "mode adr-lite".to_string(),
                    true,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Custom".to_string(),
                selected_description: Some("Choose a seed and manage custom template".to_string()),
                is_current: mode == PLAN_MODE_CUSTOM,
                actions: vec![apply_plan_settings_action(
                    "mode custom-setup".to_string(),
                    false,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(match mode.as_str() {
            PLAN_MODE_DEFAULT => 0,
            PLAN_MODE_ADR_LITE => 1,
            PLAN_MODE_CUSTOM => 2,
            _ => 0,
        }),
        ..Default::default()
    });
}

pub(crate) fn open_plan_model_picker(chat: &mut ChatWidget) {
    let Some(mut presets) = chat.selectable_model_presets() else {
        chat.add_info_message(
            "Models are being updated; please try plan model selection again in a moment."
                .to_string(),
            None,
        );
        return;
    };
    presets.sort_by(|left, right| left.display_name.cmp(&right.display_name));

    let current = plan_mode_model_override(chat);
    let mut items = vec![SelectionItem {
        name: "Inherit default model".to_string(),
        selected_description: Some(
            "Use your current global/default model in Plan mode".to_string(),
        ),
        is_current: current.is_none(),
        actions: vec![apply_plan_settings_action(
            "model inherit".to_string(),
            true,
        )],
        dismiss_on_select: true,
        ..Default::default()
    }];

    items.extend(presets.into_iter().map(|preset: ModelPreset| {
        let model = preset.model.to_string();
        SelectionItem {
            name: preset.display_name.to_string(),
            description: (!preset.description.is_empty()).then_some(preset.description.to_string()),
            selected_description: Some(model.clone()),
            is_current: current.as_deref() == Some(model.as_str()),
            is_default: preset.is_default,
            actions: vec![apply_plan_settings_action(format!("model {model}"), true)],
            dismiss_on_select: true,
            ..Default::default()
        }
    }));

    chat.show_selection_view(SelectionViewParams {
        title: Some("Plan mode model".to_string()),
        subtitle: Some("Choose a model override for Plan mode".to_string()),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": apply"),
            ("type", ": search"),
            ("Esc", ": back"),
        ])),
        undim_footer_hint: true,
        selected_item_footer_note: false,
        items,
        is_searchable: true,
        search_placeholder: Some("Search models".to_string()),
        ..Default::default()
    });
}

fn open_plan_custom_mode_setup(chat: &mut ChatWidget) {
    let custom_seed = read_custom_seed_mode(chat);
    let template_path = custom_template_path(chat);
    chat.show_selection_view(SelectionViewParams {
        title: Some("Custom mode setup".to_string()),
        subtitle: Some("Choose seed and manage custom template".to_string()),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": apply"),
            ("Esc", ": back"),
        ])),
        undim_footer_hint: true,
        selected_item_footer_note: false,
        items: vec![
            SelectionItem {
                name: "Seed from default".to_string(),
                selected_description: Some(
                    "Create/reseed template from default workflow".to_string(),
                ),
                is_current: custom_seed == PLAN_MODE_DEFAULT,
                actions: vec![apply_plan_settings_action(
                    "mode custom default".to_string(),
                    true,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Seed from adr-lite".to_string(),
                selected_description: Some(
                    "Create/reseed template from adr-lite workflow".to_string(),
                ),
                is_current: custom_seed == PLAN_MODE_ADR_LITE,
                actions: vec![apply_plan_settings_action(
                    "mode custom adr-lite".to_string(),
                    true,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "View custom template path".to_string(),
                selected_description: Some(template_path.display().to_string()),
                actions: vec![apply_plan_settings_action(
                    "custom-template view".to_string(),
                    true,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Edit custom template".to_string(),
                selected_description: Some("Open template in external editor".to_string()),
                actions: vec![apply_plan_settings_action(
                    "custom-template open".to_string(),
                    true,
                )],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(if custom_seed == PLAN_MODE_DEFAULT {
            0
        } else {
            1
        }),
        ..Default::default()
    });
}

pub(crate) fn apply_plan_settings_command(
    chat: &mut ChatWidget,
    args: &str,
    reopen_settings: bool,
) {
    handle_settings_command(chat, args);
    if reopen_settings {
        open_plan_settings(chat);
    }
}

pub(crate) fn cycle_plan_settings_value(chat: &mut ChatWidget, target: &str, selected_idx: usize) {
    match target {
        "brainstorm-first" => {
            let next = next_brainstorm_pref(&read_brainstorm_pref(chat));
            set_brainstorm_pref(chat, next, false);
        }
        "flowchart" => {
            let next = next_flowchart_pref(&read_flowchart_pref(chat));
            set_flowchart_pref(chat, next, false);
        }
        "mode" => {
            let mode = read_plan_mode(chat);
            let custom_seed = read_custom_seed_mode(chat);
            let next = next_mode_cycle_command(&mode, &custom_seed);
            if let Some(value) = next.strip_prefix("mode ") {
                set_plan_mode(chat, value, false);
            }
        }
        "track-worktree" => {
            let next = !read_plan_track_worktree(chat);
            set_plan_track_worktree(chat, bool_setting_label(next), false);
        }
        "track-branch" => {
            let next = !read_plan_track_branch(chat);
            set_plan_track_branch(chat, bool_setting_label(next), false);
        }
        "mismatch-action" => {
            let next = next_mismatch_action(&read_plan_mismatch_action(chat));
            set_plan_mismatch_action(chat, next, false);
        }
        "naming" => {
            let next = next_naming_strategy(&read_plan_naming_strategy(chat));
            set_plan_naming_strategy(chat, next, false);
        }
        _ => {}
    }
    open_plan_settings_with_selected(chat, Some(selected_idx), true);
}

fn set_plan_base_dir(chat: &mut ChatWidget, raw: &str) {
    let path_arg = strip_wrapping_quotes(raw.trim());
    if path_arg.is_empty() {
        chat.add_error_message("Usage: /plan settings base-dir <path>".to_string());
        return;
    }

    let resolved = if PathBuf::from(path_arg).is_absolute() {
        PathBuf::from(path_arg)
    } else {
        chat.session_cwd().join(path_arg)
    };
    if let Err(err) = std::fs::create_dir_all(&resolved) {
        chat.add_error_message(format!(
            "Failed to create base directory `{}`: {err}",
            resolved.display(),
        ));
        return;
    }
    let normalized = std::fs::canonicalize(&resolved).unwrap_or(resolved);

    if let Err(err) = write_state_file(chat, PLAN_BASE_DIR_FILE, &normalized.display().to_string())
    {
        chat.add_error_message(format!("Failed to save plan base directory: {err}"));
        return;
    }

    let mut notes = vec![
        "xcodex writes plan files there directly. Ensure this path is in your trusted writable list."
            .to_string(),
    ];
    if let Some(gitignore_note) = maybe_gitignore_note(chat, &normalized) {
        notes.push(gitignore_note);
    }

    chat.add_info_message(
        format!("Plan base directory set to `{}`.", normalized.display()),
        Some(notes.join("\n")),
    );
}

fn show_current_plan_path(chat: &mut ChatWidget) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_info_message(
            "No active plan file.\nRun `/plan open` to create or select one.".to_string(),
            None,
        );
        return;
    };
    chat.add_info_message(
        format!(
            "Current active plan file: `{}`\nUse `/plan settings current-path copy` to copy it.",
            path.display(),
        ),
        None,
    );
}

fn copy_current_plan_path(chat: &mut ChatWidget) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file to copy.".to_string());
        return;
    };
    let text = path.display().to_string();
    match crate::clipboard_copy::copy_text(text.clone()) {
        Ok(()) => chat.add_info_message(format!("Copied active plan path: `{text}`"), None),
        Err(err) => chat.add_error_message(format!("Failed to copy active plan path: {err}")),
    }
}

fn open_current_plan_path(chat: &mut ChatWidget) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
        return;
    };
    if !path.exists() {
        chat.add_error_message(format!(
            "Active plan file does not exist: `{}`",
            path.display(),
        ));
        return;
    }
    chat.app_event_tx()
        .send(AppEvent::OpenPlanInExternalEditor { path });
}

fn set_brainstorm_pref(chat: &mut ChatWidget, pref: &str, announce: bool) {
    if !matches!(pref, "auto" | "always" | "never") {
        chat.add_error_message(
            "Invalid brainstorm-first value. Use auto|always|never.".to_string(),
        );
        return;
    }
    if let Err(err) = write_state_file(chat, BRAINSTORM_PREF_FILE, pref) {
        chat.add_error_message(format!("Failed to save brainstorm-first preference: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(
            format!("Brainstorm-first preference set to `{pref}`."),
            None,
        );
    }
}

fn set_flowchart_pref(chat: &mut ChatWidget, pref: &str, announce: bool) {
    if !matches!(pref, "auto" | "prefer-text" | "prefer-diagrams") {
        chat.add_error_message(
            "Invalid flowchart preference. Use auto|prefer-text|prefer-diagrams.".to_string(),
        );
        return;
    }
    if let Err(err) = write_state_file(chat, FLOWCHART_PREF_FILE, pref) {
        chat.add_error_message(format!("Failed to save flowchart preference: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(format!("Flowchart preference set to `{pref}`."), None);
    }
}

fn set_plan_track_worktree(chat: &mut ChatWidget, raw: &str, announce: bool) {
    let Some(value) = normalize_bool_setting(raw) else {
        chat.add_error_message("Invalid track-worktree value. Use on|off.".to_string());
        return;
    };
    if let Err(err) = write_state_file(chat, PLAN_TRACK_WORKTREE_FILE, bool_setting_label(value)) {
        chat.add_error_message(format!("Failed to save track-worktree setting: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(
            format!("Track worktree set to `{}`.", bool_setting_label(value)),
            None,
        );
    }
}

fn set_plan_track_branch(chat: &mut ChatWidget, raw: &str, announce: bool) {
    let Some(value) = normalize_bool_setting(raw) else {
        chat.add_error_message("Invalid track-branch value. Use on|off.".to_string());
        return;
    };
    if let Err(err) = write_state_file(chat, PLAN_TRACK_BRANCH_FILE, bool_setting_label(value)) {
        chat.add_error_message(format!("Failed to save track-branch setting: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(
            format!("Track branch set to `{}`.", bool_setting_label(value)),
            None,
        );
    }
}

fn set_plan_mismatch_action(chat: &mut ChatWidget, raw: &str, announce: bool) {
    let Some(action) = normalize_mismatch_action(raw) else {
        chat.add_error_message("Invalid mismatch-action value. Use warn|block.".to_string());
        return;
    };
    if let Err(err) = write_state_file(chat, PLAN_MISMATCH_ACTION_FILE, action) {
        chat.add_error_message(format!("Failed to save mismatch-action setting: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(format!("Mismatch action set to `{action}`."), None);
    }
}

fn set_plan_naming_strategy(chat: &mut ChatWidget, raw: &str, announce: bool) {
    let Some(strategy) = normalize_naming_strategy(raw) else {
        chat.add_error_message("Invalid naming strategy. Use funny|date-title.".to_string());
        return;
    };
    if let Err(err) = write_state_file(chat, PLAN_NAMING_STRATEGY_FILE, strategy) {
        chat.add_error_message(format!("Failed to save naming strategy: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(format!("Plan naming strategy set to `{strategy}`."), None);
    }
}

fn set_plan_model(chat: &mut ChatWidget, raw: &str, announce: bool) {
    let value = strip_wrapping_quotes(raw.trim());
    if value.is_empty() {
        chat.add_error_message("Invalid model value. Use inherit|<model_slug>.".to_string());
        return;
    }
    if value.eq_ignore_ascii_case("inherit") {
        if let Err(err) = remove_state_file(chat, PLAN_MODEL_FILE) {
            chat.add_error_message(format!("Failed to save plan model setting: {err}"));
            return;
        }
        if announce {
            chat.add_info_message("Plan mode model set to `inherit`.".to_string(), None);
        }
        return;
    }
    if let Err(err) = write_state_file(chat, PLAN_MODEL_FILE, value) {
        chat.add_error_message(format!("Failed to save plan model setting: {err}"));
        return;
    }
    if announce {
        chat.add_info_message(format!("Plan mode model set to `{value}`."), None);
    }
}

fn set_plan_mode(chat: &mut ChatWidget, raw_mode: &str, announce: bool) {
    let mut tokens = raw_mode.split_whitespace();
    let Some(mode_token) = tokens.next() else {
        chat.add_error_message(
            "Invalid plan mode. Use default|adr-lite|custom [default|adr-lite].".to_string(),
        );
        return;
    };
    let Some(mode) = normalize_plan_mode(mode_token) else {
        chat.add_error_message(
            "Invalid plan mode. Use default|adr-lite|custom [default|adr-lite].".to_string(),
        );
        return;
    };
    let custom_seed_mode = if mode == PLAN_MODE_CUSTOM {
        let seed_token = tokens.next();
        if tokens.next().is_some() {
            chat.add_error_message(
                "Invalid plan mode. Use /plan settings mode custom <default|adr-lite>.".to_string(),
            );
            return;
        }
        match seed_token.and_then(normalize_custom_seed_mode) {
            Some(seed_mode) => Some(seed_mode.to_string()),
            None => {
                if seed_token.is_some() {
                    chat.add_error_message(
                        "Invalid custom seed mode. Use `default` or `adr-lite`.".to_string(),
                    );
                    return;
                }
                None
            }
        }
    } else {
        if tokens.next().is_some() {
            chat.add_error_message(
                "Invalid plan mode. Use default|adr-lite|custom [default|adr-lite].".to_string(),
            );
            return;
        }
        None
    };

    let mut notes = vec![format!(
        "Mode default base directory: `{}`.",
        mode_default_plan_base_dir(chat, mode).display(),
    )];

    if mode == PLAN_MODE_CUSTOM {
        let seed_mode = custom_seed_mode.unwrap_or_else(|| read_custom_seed_mode(chat));
        let template_path = custom_template_path(chat);
        let created = match ensure_custom_template_exists(chat, &template_path, &seed_mode) {
            Ok(created) => created,
            Err(err) => {
                chat.add_error_message(format!(
                    "Failed to initialize custom template `{}`: {err}",
                    template_path.display(),
                ));
                return;
            }
        };
        notes.push(format!(
            "Custom template path: `{}`.",
            template_path.display()
        ));
        notes.push(format!("Custom seed mode: `{seed_mode}`."));
        if created {
            notes.push("Created custom template from selected seed.".to_string());
        } else {
            notes
                .push("Custom template already existed; kept file contents unchanged.".to_string());
        }
        notes.push(
            "Use `/plan settings custom-template init <default|adr-lite>` to reseed intentionally."
                .to_string(),
        );
    }

    if let Err(err) = write_state_file(chat, PLAN_MODE_FILE, mode) {
        chat.add_error_message(format!("Failed to save plan mode: {err}"));
        return;
    }
    notes.push("Explicit `/plan settings base-dir` still takes precedence.".to_string());
    if announce {
        chat.add_info_message(
            format!("Plan mode set to `{mode}`."),
            Some(notes.join("\n")),
        );
    }
}

fn ensure_custom_template_exists(
    chat: &ChatWidget,
    template_path: &Path,
    seed_mode: &str,
) -> std::io::Result<bool> {
    if let Some(parent) = template_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut created = false;
    if !template_path.exists() {
        let template = default_plan_template_for_mode(chat, template_path, seed_mode);
        std::fs::write(template_path, template)?;
        created = true;
    }

    write_state_file(
        chat,
        PLAN_CUSTOM_TEMPLATE_FILE,
        &template_path.display().to_string(),
    )?;
    write_state_file(chat, PLAN_CUSTOM_SEED_MODE_FILE, seed_mode)?;
    Ok(created)
}

fn show_custom_template_path(chat: &mut ChatWidget) {
    let template_path = custom_template_path(chat);
    let mode = read_plan_mode(chat);
    let seed = read_custom_seed_mode(chat);
    chat.add_info_message(
        format!(
            "Custom template path: `{}`\nPlan mode: `{mode}`\nSeed mode: `{seed}`",
            template_path.display()
        ),
        Some(
            "Use `/plan settings custom-template init <default|adr-lite>` to (re)seed this file."
                .to_string(),
        ),
    );
}

fn open_custom_template_path(chat: &mut ChatWidget) {
    let template_path = custom_template_path(chat);
    if !template_path.exists() {
        chat.add_error_message(format!(
            "Custom template file does not exist: `{}`\nRun `/plan settings custom-template init <default|adr-lite>` first.",
            template_path.display(),
        ));
        return;
    }
    chat.app_event_tx()
        .send(AppEvent::OpenPlanInExternalEditor {
            path: template_path,
        });
}

fn init_custom_template(chat: &mut ChatWidget, raw_seed: &str) {
    let Some(seed_mode) = normalize_custom_seed_mode(raw_seed) else {
        chat.add_error_message(
            "Invalid custom seed mode. Use `default` or `adr-lite`.".to_string(),
        );
        return;
    };

    let template_path = custom_template_path(chat);
    if let Some(parent) = template_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        chat.add_error_message(format!(
            "Failed to create custom template directory `{}`: {err}",
            parent.display(),
        ));
        return;
    }

    let template = default_plan_template_for_mode(chat, &template_path, seed_mode);
    if let Err(err) = std::fs::write(&template_path, template) {
        chat.add_error_message(format!(
            "Failed to write custom template `{}`: {err}",
            template_path.display(),
        ));
        return;
    }
    if let Err(err) = write_state_file(
        chat,
        PLAN_CUSTOM_TEMPLATE_FILE,
        &template_path.display().to_string(),
    ) {
        chat.add_error_message(format!("Failed to persist custom template path: {err}"));
        return;
    }
    if let Err(err) = write_state_file(chat, PLAN_CUSTOM_SEED_MODE_FILE, seed_mode) {
        chat.add_error_message(format!("Failed to persist custom seed mode: {err}"));
        return;
    }

    chat.add_info_message(
        format!(
            "Custom template initialized from `{seed_mode}` at `{}`.",
            template_path.display(),
        ),
        Some("Set `/plan settings mode custom` to use this template for new plans.".to_string()),
    );
}

fn show_plan_status(chat: &mut ChatWidget) {
    let base_dir = plan_base_dir(chat);
    let active_plan = read_active_plan_path(chat);
    let brainstorm = read_brainstorm_pref(chat);
    let flowchart = read_flowchart_pref(chat);
    let track_worktree = read_plan_track_worktree(chat);
    let track_branch = read_plan_track_branch(chat);
    let mismatch_action = read_plan_mismatch_action(chat);
    let naming_strategy = read_plan_naming_strategy(chat);
    let mode = read_plan_mode(chat);
    let plan_model = plan_mode_model_override(chat);

    let mut lines = vec![
        "Plan status".to_string(),
        format!("- Mode: {mode}"),
        format!(
            "- Plan mode model: {}",
            plan_model.as_deref().unwrap_or("inherit")
        ),
        format!("- Base directory: {}", base_dir.display()),
        format!("- Active plan file: {}", format_path(active_plan.clone())),
        format!("- Brainstorm-first: {brainstorm}"),
        format!("- Flowchart preference: {flowchart}"),
        format!("- Track worktree: {}", bool_setting_label(track_worktree)),
        format!("- Track branch: {}", bool_setting_label(track_branch)),
        format!("- Context mismatch action: {mismatch_action}"),
        format!("- Naming strategy: {naming_strategy}"),
    ];
    if mode == PLAN_MODE_CUSTOM {
        lines.push(format!(
            "- Custom template path: {}",
            custom_template_path(chat).display()
        ));
        lines.push(format!(
            "- Custom seed mode: {}",
            read_custom_seed_mode(chat)
        ));
    }

    if let Some(path) = active_plan
        && path.exists()
    {
        if let Some(active_mode) = read_plan_mode_from_file(&path) {
            lines.push(format!("- Active plan workflow mode: {active_mode}"));
        }
        if let Some(active_sync_mode) = read_plan_sync_mode_from_file(&path) {
            lines.push(format!("- Active plan sync mode: {active_sync_mode}"));
        }
        let status = read_plan_status(&path).unwrap_or_else(|| "Unknown".to_string());
        let todos = count_unchecked_todos(&path);
        lines.push(format!("- Active plan status: {status}"));
        lines.push(format!("- TODOs remaining: {todos}"));
        if let Some(summary) = plan_context_mismatch_summary(chat, &path) {
            lines.push(format!("- Context check: {summary}"));
        }
    }

    chat.add_info_message(lines.join("\n"), None);
}

fn show_plan_list(chat: &mut ChatWidget, scope: PlanListScope) {
    let base_dir = plan_base_dir(chat);
    let plans = discover_plans(chat);
    let active_plan = read_active_plan_path(chat);
    let filtered: Vec<PlanEntry> = plans
        .into_iter()
        .filter(|entry| scope.matches_status(entry.status.as_str()))
        .collect();

    let mut items = vec![
        SelectionItem {
            name: "Create new plan".to_string(),
            selected_description: Some("Create/open active plan file".to_string()),
            actions: vec![open_plan_file_action(None)],
            dismiss_on_select: true,
            ..Default::default()
        },
        SelectionItem {
            name: "Plan settings".to_string(),
            selected_description: Some("Open plan mode settings".to_string()),
            actions: vec![open_plan_settings_action()],
            dismiss_on_select: true,
            ..Default::default()
        },
    ];

    let mut selected_idx: Option<usize> = None;
    for entry in &filtered {
        let is_current = active_plan.as_ref().is_some_and(|path| path == &entry.path);
        let description = format!("[{}] {}", entry.status, entry.path.display());
        let mut selected_description = description.clone();
        if is_current {
            selected_description.push_str(" (active)");
        }

        let item = SelectionItem {
            name: entry.title.clone(),
            selected_description: Some(selected_description),
            is_current,
            actions: vec![open_plan_load_confirmation_action(
                entry.path.clone(),
                scope.token().to_string(),
            )],
            dismiss_on_select: true,
            search_value: Some(format!(
                "{} {} {}",
                entry.title,
                entry.status,
                entry.path.display(),
            )),
            ..Default::default()
        };
        if is_current {
            selected_idx = Some(items.len());
        }
        items.push(item);
    }

    if filtered.is_empty() {
        items.push(disabled_item(
            "No plans in this tab".to_string(),
            Some("Use `/plan open` to create one.".to_string()),
            "No matches",
        ));
    }

    let subtitle = format!(
        "{} plans · filter: {} · base: {}",
        filtered.len(),
        scope.label(),
        base_dir.display(),
    );

    chat.show_or_replace_selection_view(SelectionViewParams {
        title: Some("Plan list".to_string()),
        subtitle: Some(subtitle),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": choose"),
            ("Tab", ": next filter"),
            ("type", ": search"),
            ("Esc", ": close"),
        ])),
        undim_footer_hint: true,
        selected_item_footer_note: false,
        items,
        is_searchable: true,
        search_placeholder: Some("Search plans by title, status, or path".to_string()),
        initial_selected_idx: selected_idx,
        tab_state: Some(TabState {
            current: scope.index(),
            count: PlanListScope::COUNT,
            on_tab: Box::new(|idx, sender| {
                let scope = PlanListScope::from_index(idx);
                sender.send(AppEvent::OpenPlanListView {
                    scope: scope.token().to_string(),
                });
            }),
        }),
        ..Default::default()
    });
}

fn open_plan_file(chat: &mut ChatWidget, path_arg: Option<&str>) {
    let resolved_path = path_arg
        .map(|arg| resolve_plan_path(chat, arg))
        .unwrap_or_else(|| default_new_plan_path(chat));
    let mut created_new_file = false;

    if let Some(parent) = resolved_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        chat.add_error_message(format!(
            "Failed to create plan directory `{}`: {err}",
            parent.display(),
        ));
        return;
    }

    if !resolved_path.exists() {
        let template = default_plan_template(chat, &resolved_path);
        if let Err(err) = std::fs::write(&resolved_path, template) {
            chat.add_error_message(format!(
                "Failed to create plan file `{}`: {err}",
                resolved_path.display(),
            ));
            return;
        }
        created_new_file = true;
    }

    if let Err(err) = write_active_plan_path(chat, &resolved_path) {
        chat.add_error_message(format!("Failed to set active plan file: {err}"));
        return;
    }

    let selected_mode = read_plan_mode(chat);
    if created_new_file {
        let sync_mode = effective_sync_mode_for_workflow_mode(chat, &selected_mode);
        if let Err(err) =
            upsert_plan_metadata_value(&resolved_path, PLAN_FILE_MODE_FIELD, &selected_mode)
        {
            tracing::warn!(
                error = %err,
                path = %resolved_path.display(),
                "failed to seed plan mode metadata while creating plan file"
            );
        }
        if let Err(err) =
            upsert_plan_metadata_value(&resolved_path, PLAN_FILE_SYNC_MODE_FIELD, sync_mode)
        {
            tracing::warn!(
                error = %err,
                path = %resolved_path.display(),
                "failed to seed plan sync-mode metadata while creating plan file"
            );
        }
    }
    let (locked_mode, lock_seeded) = match ensure_plan_mode_lock(chat, &resolved_path) {
        Ok((mode, seeded_metadata)) => (mode, seeded_metadata),
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %resolved_path.display(),
                "failed to ensure plan mode lock metadata while opening plan file"
            );
            (selected_mode.clone(), false)
        }
    };

    if uses_adr_lite_sync(chat, Some(&resolved_path)) {
        let today = chrono::Local::now().date_naive().to_string();
        let worktree = chat.session_cwd().display().to_string();
        let branch = current_branch_name(chat);
        if let Err(err) = plan_file::sync_adr_lite_open_or_resume(
            &resolved_path,
            &today,
            &worktree,
            &branch,
            "open sync",
        ) {
            tracing::warn!(
                error = %err,
                path = %resolved_path.display(),
                "failed adr-lite open sync for plan file"
            );
        }
    }
    maybe_notify_plan_context_mismatch(chat, &resolved_path, "opened plan file");

    let mut notes: Vec<String> = Vec::new();
    if locked_mode != selected_mode {
        notes.push(format!(
            "This plan is locked to workflow mode `{locked_mode}` (current default setting is `{selected_mode}`)."
        ));
    }
    if lock_seeded {
        notes.push("Plan mode lock metadata is active for this plan file.".to_string());
    }

    chat.add_info_message(
        format!("Active plan file set to `{}`.", resolved_path.display()),
        (!notes.is_empty()).then_some(notes.join("\n")),
    );
}

fn mark_active_plan_done(chat: &mut ChatWidget) {
    set_active_plan_status(chat, "Done", "Marked plan as done");
}

fn mark_active_plan_archived(chat: &mut ChatWidget) {
    set_active_plan_status(chat, "Archived", "Archived plan");
}

fn pause_active_plan_run(chat: &mut ChatWidget) {
    set_active_plan_status(chat, "Paused", "Paused plan run");
}

fn checkpoint_note(last_agent_message: Option<&str>) -> String {
    let Some(first_line) = last_agent_message
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .and_then(|message| message.lines().find(|line| !line.trim().is_empty()))
        .map(str::trim)
    else {
        return "Completed assistant turn.".to_string();
    };

    let compact = first_line
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let max_chars = 140usize;
    if compact.chars().count() <= max_chars {
        return compact;
    }

    let truncated: String = compact.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn set_active_plan_status(chat: &mut ChatWidget, status: &str, success_prefix: &str) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
        return;
    };
    if !path.exists() {
        chat.add_error_message(format!(
            "Active plan file does not exist: `{}`",
            path.display(),
        ));
        return;
    }

    let today = chrono::Local::now().date_naive().to_string();
    if let Err(err) = plan_file::set_status(&path, status, &today) {
        chat.add_error_message(format!("Failed to update plan status: {err}"));
        return;
    }

    chat.add_info_message(format!("{success_prefix}: `{}`", path.display()), None);
}

fn open_plan_done_confirmation(chat: &mut ChatWidget) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
        return;
    };
    if !path.exists() {
        chat.add_error_message(format!(
            "Active plan file does not exist: `{}`",
            path.display()
        ));
        return;
    }

    let path_display = path.display().to_string();
    chat.show_selection_view(SelectionViewParams {
        title: Some("Mark active plan as done?".to_string()),
        subtitle: Some(path_display.clone()),
        footer_note: Some(Line::from(
            "This updates `Status: Done` in the active plan file.",
        )),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": confirm"),
            ("Esc", ": cancel"),
        ])),
        items: vec![
            SelectionItem {
                name: "Mark as done".to_string(),
                description: Some(format!("Set `Status: Done` for {path_display}")),
                actions: vec![Box::new(|sender| sender.send(AppEvent::MarkActivePlanDone))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Keep current plan status".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

fn open_plan_archive_confirmation(chat: &mut ChatWidget) {
    let Some(path) = read_active_plan_path(chat) else {
        chat.add_error_message("No active plan file. Run `/plan open` first.".to_string());
        return;
    };
    if !path.exists() {
        chat.add_error_message(format!(
            "Active plan file does not exist: `{}`",
            path.display()
        ));
        return;
    }

    let path_display = path.display().to_string();
    chat.show_selection_view(SelectionViewParams {
        title: Some("Archive active plan?".to_string()),
        subtitle: Some(path_display.clone()),
        footer_note: Some(Line::from(
            "This updates `Status: Archived` in the active plan file.",
        )),
        footer_hint: Some(plan_footer_hint_line(&[
            ("↑/↓", ": select"),
            ("Enter", ": confirm"),
            ("Esc", ": cancel"),
        ])),
        items: vec![
            SelectionItem {
                name: "Archive".to_string(),
                description: Some(format!("Set `Status: Archived` for {path_display}")),
                actions: vec![Box::new(|sender| {
                    sender.send(AppEvent::MarkActivePlanArchived)
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Keep current plan status".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        initial_selected_idx: Some(0),
        ..Default::default()
    });
}

fn disabled_item(name: String, description: Option<String>, reason: &str) -> SelectionItem {
    SelectionItem {
        name,
        selected_description: description.or_else(|| Some(reason.to_string())),
        is_disabled: true,
        is_dimmed: true,
        ..Default::default()
    }
}

fn plan_footer_hint_line(items: &[(&str, &str)]) -> Line<'static> {
    let key_style = crate::theme::accent_style().add_modifier(Modifier::BOLD);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (idx, (key, hint)) in items.iter().enumerate() {
        spans.push(Span::styled((*key).to_string(), key_style));
        spans.push(Span::raw((*hint).to_string()));
        if idx + 1 < items.len() {
            spans.push("  ".into());
        }
    }
    Line::from(spans)
}

fn next_brainstorm_pref(current: &str) -> &'static str {
    match current {
        "auto" => "always",
        "always" => "never",
        _ => "auto",
    }
}

fn next_flowchart_pref(current: &str) -> &'static str {
    match current {
        "auto" => "prefer-text",
        "prefer-text" => "prefer-diagrams",
        _ => "auto",
    }
}

fn next_mode_cycle_command(current_mode: &str, custom_seed: &str) -> String {
    match (current_mode, custom_seed) {
        (PLAN_MODE_DEFAULT, _) => "mode adr-lite".to_string(),
        (PLAN_MODE_ADR_LITE, _) => "mode custom".to_string(),
        (PLAN_MODE_CUSTOM, _) => "mode default".to_string(),
        _ => "mode default".to_string(),
    }
}

fn format_mode_for_display(mode: &str) -> String {
    mode.to_string()
}

fn bool_setting_label(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn normalize_bool_setting(raw: &str) -> Option<bool> {
    match raw.trim() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn normalize_mismatch_action(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        PLAN_MISMATCH_ACTION_WARN => Some(PLAN_MISMATCH_ACTION_WARN),
        PLAN_MISMATCH_ACTION_BLOCK => Some(PLAN_MISMATCH_ACTION_BLOCK),
        _ => None,
    }
}

fn next_mismatch_action(current: &str) -> &'static str {
    match normalize_mismatch_action(current).unwrap_or(PLAN_MISMATCH_ACTION_BLOCK) {
        PLAN_MISMATCH_ACTION_WARN => PLAN_MISMATCH_ACTION_BLOCK,
        _ => PLAN_MISMATCH_ACTION_WARN,
    }
}

fn normalize_naming_strategy(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        PLAN_NAMING_FUNNY => Some(PLAN_NAMING_FUNNY),
        PLAN_NAMING_DATE_TITLE => Some(PLAN_NAMING_DATE_TITLE),
        _ => None,
    }
}

fn next_naming_strategy(current: &str) -> &'static str {
    match normalize_naming_strategy(current).unwrap_or(PLAN_NAMING_FUNNY) {
        PLAN_NAMING_DATE_TITLE => PLAN_NAMING_FUNNY,
        _ => PLAN_NAMING_DATE_TITLE,
    }
}

fn apply_plan_settings_action(args: String, reopen_settings: bool) -> SelectionAction {
    Box::new(move |sender| {
        sender.send(AppEvent::ApplyPlanSettingsCommand {
            args: args.clone(),
            reopen_settings,
        });
    })
}

fn cycle_plan_settings_action(
    target: PlanSettingsCycleTarget,
    selected_idx: usize,
) -> SelectionAction {
    Box::new(move |sender| {
        sender.send(AppEvent::CyclePlanSettingsValue {
            target,
            selected_idx,
        });
    })
}

fn open_plan_settings_action() -> SelectionAction {
    Box::new(|sender| sender.send(AppEvent::OpenPlanSettingsView))
}

fn open_plan_base_dir_editor_action() -> SelectionAction {
    Box::new(|sender| sender.send(AppEvent::OpenPlanBaseDirEditorView))
}

fn open_plan_mode_picker_action() -> SelectionAction {
    Box::new(|sender| sender.send(AppEvent::OpenPlanModePickerView))
}

fn open_plan_model_picker_action() -> SelectionAction {
    Box::new(|sender| sender.send(AppEvent::OpenPlanModelPickerView))
}

fn open_plan_file_action(path: Option<PathBuf>) -> SelectionAction {
    Box::new(move |sender| {
        sender.send(AppEvent::OpenPlanFile { path: path.clone() });
    })
}

fn open_plan_load_confirmation_action(path: PathBuf, scope: String) -> SelectionAction {
    Box::new(move |sender| {
        sender.send(AppEvent::OpenPlanLoadConfirmation {
            path: path.clone(),
            scope: scope.clone(),
        });
    })
}

struct PlanBaseDirEditorView {
    composer: ChatComposer,
    app_event_tx: AppEventSender,
    complete: bool,
}

impl PlanBaseDirEditorView {
    fn new(
        app_event_tx: AppEventSender,
        enhanced_keys_supported: bool,
        initial_value: String,
    ) -> Self {
        let mut composer = ChatComposer::new_with_config(
            true,
            app_event_tx.clone(),
            enhanced_keys_supported,
            "Type a path, or use @ to search".to_string(),
            false,
            ChatComposerConfig {
                popups_enabled: true,
                slash_commands_enabled: false,
                image_paste_enabled: false,
            },
        );
        composer.set_steer_enabled(true);
        composer.set_footer_hint_override(Some(Vec::new()));
        composer.set_show_context_right(false);
        composer.set_text_content(initial_value, Vec::new(), Vec::new());
        Self {
            composer,
            app_event_tx,
            complete: false,
        }
    }
}

impl BottomPaneView for PlanBaseDirEditorView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if matches!(key_event.code, KeyCode::Esc) && key_event.kind == KeyEventKind::Press {
            self.on_ctrl_c();
            return;
        }
        let (result, _needs_redraw) = self.composer.handle_key_event(key_event);
        match result {
            InputResult::Submitted { text, .. } | InputResult::Queued { text, .. } => {
                let value = text.lines().next().unwrap_or_default().trim().to_string();
                if value.is_empty() {
                    return;
                }
                self.app_event_tx.send(AppEvent::ApplyPlanSettingsCommand {
                    args: format!("base-dir {value}"),
                    reopen_settings: true,
                });
                self.complete = true;
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        self.composer.handle_paste(pasted)
    }

    fn flush_paste_burst_if_due(&mut self) -> bool {
        self.composer.flush_paste_burst_if_due()
    }

    fn is_in_paste_burst(&self) -> bool {
        self.composer.is_in_paste_burst()
    }

    fn on_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.composer.on_file_search_result(query, matches);
    }
}

impl Renderable for PlanBaseDirEditorView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let [content_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        let base_style = user_message_style().patch(crate::theme::composer_style());
        Block::default().style(base_style).render(content_area, buf);
        Block::default().style(base_style).render(footer_area, buf);

        let surface = Rect {
            x: content_area.x.saturating_add(2),
            y: content_area.y.saturating_add(1),
            width: content_area.width.saturating_sub(4),
            height: content_area.height.saturating_sub(2),
        };
        let title_area = Rect {
            x: surface.x,
            y: surface.y,
            width: surface.width,
            height: 1,
        };
        ratatui::widgets::Paragraph::new(Line::from("Plan settings: default base directory"))
            .render(title_area, buf);
        let subtitle_y = surface.y.saturating_add(1);
        if subtitle_y < surface.bottom() {
            ratatui::widgets::Paragraph::new(Line::from("Type path directly or use @ file search"))
                .render(
                    Rect {
                        x: surface.x,
                        y: subtitle_y,
                        width: surface.width,
                        height: 1,
                    },
                    buf,
                );
        }
        let composer_area = Rect {
            x: surface.x,
            y: surface.y.saturating_add(2),
            width: surface.width,
            height: surface.height.saturating_sub(2),
        };
        self.composer.render(composer_area, buf);

        let hint_area = Rect {
            x: footer_area.x.saturating_add(2),
            y: footer_area.y,
            width: footer_area.width.saturating_sub(2),
            height: 1,
        };
        Paragraph::new(plan_footer_hint_line(&[
            ("↑/↓", " browse @results"),
            ("Tab", " choose @result"),
            ("Enter", " apply"),
            ("Esc", " cancel"),
        ]))
        .style(base_style)
        .render(hint_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.composer.desired_height(width).saturating_add(5)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let surface = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(2),
        };
        self.composer.cursor_pos(Rect {
            x: surface.x,
            y: surface.y.saturating_add(2),
            width: surface.width,
            height: surface.height.saturating_sub(2),
        })
    }
}

fn plan_state_dir(chat: &ChatWidget) -> PathBuf {
    chat.codex_home().join("plans")
}

fn plan_base_dir(chat: &ChatWidget) -> PathBuf {
    read_state_file(chat, PLAN_BASE_DIR_FILE)
        .map(PathBuf::from)
        .or_else(|| chat.configured_plan_base_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| mode_default_plan_base_dir(chat, &read_plan_mode(chat)))
}

fn active_plan_pointer_path(chat: &ChatWidget) -> PathBuf {
    plan_state_dir(chat).join(ACTIVE_PLAN_FILE)
}

fn active_plan_by_thread_path(chat: &ChatWidget) -> PathBuf {
    plan_state_dir(chat).join(ACTIVE_PLAN_BY_THREAD_FILE)
}

fn active_plan_thread_key(chat: &ChatWidget) -> Option<String> {
    chat.thread_id().map(|thread_id| thread_id.to_string())
}

fn read_active_plan_path_global(chat: &ChatWidget) -> Option<PathBuf> {
    let content = std::fs::read_to_string(active_plan_pointer_path(chat)).ok()?;
    let value = content.trim();
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

fn read_active_plan_path(chat: &ChatWidget) -> Option<PathBuf> {
    if let Some(thread_key) = active_plan_thread_key(chat)
        && let Some(path) = read_active_plan_by_thread(chat).get(&thread_key)
    {
        return Some(PathBuf::from(path));
    }
    read_active_plan_path_global(chat)
}

fn write_active_plan_path(chat: &ChatWidget, path: &Path) -> std::io::Result<()> {
    let pointer = active_plan_pointer_path(chat);
    if let Some(parent) = pointer.parent() {
        std::fs::create_dir_all(parent)?;
    }
    plan_file::write_atomic(&pointer, &path.display().to_string())?;
    if let Some(thread_key) = active_plan_thread_key(chat)
        && let Err(err) = write_active_plan_by_thread(chat, &thread_key, path)
    {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            thread_id = thread_key,
            "failed to persist thread-scoped active plan pointer"
        );
    }
    Ok(())
}

fn read_active_plan_by_thread(chat: &ChatWidget) -> BTreeMap<String, String> {
    let path = active_plan_by_thread_path(chat);
    let Ok(content) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<BTreeMap<String, String>>(&content).unwrap_or_default()
}

fn write_active_plan_by_thread(
    chat: &ChatWidget,
    thread_key: &str,
    path: &Path,
) -> std::io::Result<()> {
    let mut state = read_active_plan_by_thread(chat);
    state.insert(thread_key.to_string(), path.display().to_string());
    let state_path = active_plan_by_thread_path(chat);
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&state).map_err(|err| {
        std::io::Error::other(format!(
            "failed to serialize thread-scoped active plan map: {err}"
        ))
    })?;
    plan_file::write_atomic(&state_path, &serialized)
}

fn bind_active_plan_to_thread(chat: &ChatWidget) {
    let Some(thread_key) = active_plan_thread_key(chat) else {
        return;
    };
    let state = read_active_plan_by_thread(chat);
    if state.contains_key(&thread_key) {
        return;
    }
    let Some(path) = read_active_plan_path_global(chat) else {
        return;
    };
    if let Err(err) = write_active_plan_by_thread(chat, &thread_key, &path) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            thread_id = thread_key,
            "failed to bind global active plan pointer to thread-scoped state"
        );
    }
}

fn read_state_file(chat: &ChatWidget, file: &str) -> Option<String> {
    std::fs::read_to_string(plan_state_dir(chat).join(file))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn write_state_file(chat: &ChatWidget, file: &str, value: &str) -> std::io::Result<()> {
    let path = plan_state_dir(chat).join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    plan_file::write_atomic(&path, value)
}

fn remove_state_file(chat: &ChatWidget, file: &str) -> std::io::Result<()> {
    let path = plan_state_dir(chat).join(file);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn maybe_gitignore_note(chat: &ChatWidget, base_dir: &Path) -> Option<String> {
    let repo_root = get_git_repo_root(base_dir)?;
    if !base_dir.starts_with(&repo_root) {
        return None;
    }

    let is_ignored = is_gitignored_in_repo(&repo_root, base_dir)?;
    if is_ignored {
        return None;
    }

    let base_dir_key = base_dir.display().to_string();
    let mut prompted = read_gitignore_prompted_base_dirs(chat);
    if prompted.contains(&base_dir_key) {
        return None;
    }
    prompted.insert(base_dir_key);
    if let Err(err) = write_gitignore_prompted_base_dirs(chat, &prompted) {
        tracing::warn!(
            error = %err,
            path = %base_dir.display(),
            "failed to persist gitignore prompt state for plan base dir"
        );
    }

    let suggestion = base_dir
        .strip_prefix(&repo_root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| format!("`{}/`", relative.display()))
        .unwrap_or_else(|| "`plans/`".to_string());

    Some(format!(
        "This directory is inside git worktree `{}` and does not appear ignored. Consider adding {} to `.gitignore`.",
        repo_root.display(),
        suggestion
    ))
}

fn is_gitignored_in_repo(repo_root: &Path, base_dir: &Path) -> Option<bool> {
    let probe = base_dir.join(".xcodex-plan-ignore-probe");
    let relative_probe = probe.strip_prefix(repo_root).ok()?;
    if relative_probe.as_os_str().is_empty() {
        return Some(false);
    }

    let status = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(["check-ignore", "-q", "--no-index"])
        .arg(relative_probe)
        .status()
        .ok()?;

    if status.success() {
        return Some(true);
    }
    status.code().and_then(|code| (code == 1).then_some(false))
}

fn read_gitignore_prompted_base_dirs(chat: &ChatWidget) -> BTreeSet<String> {
    let path = plan_state_dir(chat).join(PLAN_GITIGNORE_PROMPTED_FILE);
    let Ok(content) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    serde_json::from_str::<Vec<String>>(&content)
        .map(|items| items.into_iter().collect())
        .unwrap_or_default()
}

fn write_gitignore_prompted_base_dirs(
    chat: &ChatWidget,
    paths: &BTreeSet<String>,
) -> std::io::Result<()> {
    let path = plan_state_dir(chat).join(PLAN_GITIGNORE_PROMPTED_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let as_vec: Vec<String> = paths.iter().cloned().collect();
    let serialized = serde_json::to_string_pretty(&as_vec).map_err(|err| {
        std::io::Error::other(format!(
            "failed to serialize gitignore-prompted plan base dirs: {err}"
        ))
    })?;
    plan_file::write_atomic(&path, &serialized)
}

fn read_brainstorm_pref(chat: &ChatWidget) -> String {
    read_state_file(chat, BRAINSTORM_PREF_FILE)
        .unwrap_or_else(|| DEFAULT_BRAINSTORM_PREF.to_string())
}

fn read_flowchart_pref(chat: &ChatWidget) -> String {
    read_state_file(chat, FLOWCHART_PREF_FILE).unwrap_or_else(|| DEFAULT_FLOWCHART_PREF.to_string())
}

fn read_plan_mode(chat: &ChatWidget) -> String {
    read_state_file(chat, PLAN_MODE_FILE)
        .or_else(|| chat.configured_plan_mode().map(ToString::to_string))
        .and_then(|mode| normalize_plan_mode(&mode).map(ToString::to_string))
        .unwrap_or_else(|| DEFAULT_PLAN_MODE.to_string())
}

pub(crate) fn plan_mode_model_override(chat: &ChatWidget) -> Option<String> {
    read_state_file(chat, PLAN_MODEL_FILE).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("inherit") {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn plan_mode_model_display_label(chat: &ChatWidget, model: Option<&str>) -> String {
    let Some(model) = model else {
        return "inherit".to_string();
    };
    let Some(presets) = chat.selectable_model_presets() else {
        return model.to_string();
    };
    presets
        .into_iter()
        .find(|preset| preset.model == model)
        .map(|preset| preset.display_name)
        .unwrap_or_else(|| model.to_string())
}

fn normalize_plan_mode(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        PLAN_MODE_DEFAULT | "xcodex" => Some(PLAN_MODE_DEFAULT),
        PLAN_MODE_ADR_LITE => Some(PLAN_MODE_ADR_LITE),
        PLAN_MODE_CUSTOM => Some(PLAN_MODE_CUSTOM),
        _ => None,
    }
}

fn normalize_plan_sync_mode(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        PLAN_MODE_DEFAULT => Some(PLAN_MODE_DEFAULT),
        PLAN_MODE_ADR_LITE => Some(PLAN_MODE_ADR_LITE),
        _ => None,
    }
}

fn read_custom_seed_mode(chat: &ChatWidget) -> String {
    read_state_file(chat, PLAN_CUSTOM_SEED_MODE_FILE)
        .or_else(|| {
            chat.configured_plan_custom_seed_mode()
                .map(ToString::to_string)
        })
        .and_then(|mode| normalize_custom_seed_mode(&mode).map(ToString::to_string))
        .unwrap_or_else(|| PLAN_MODE_ADR_LITE.to_string())
}

fn normalize_custom_seed_mode(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        PLAN_MODE_DEFAULT => Some(PLAN_MODE_DEFAULT),
        PLAN_MODE_ADR_LITE => Some(PLAN_MODE_ADR_LITE),
        _ => None,
    }
}

fn default_naming_strategy_for_mode(mode: &str) -> &'static str {
    match mode {
        PLAN_MODE_DEFAULT => PLAN_NAMING_FUNNY,
        PLAN_MODE_ADR_LITE | PLAN_MODE_CUSTOM => PLAN_NAMING_DATE_TITLE,
        _ => PLAN_NAMING_FUNNY,
    }
}

fn read_plan_track_worktree(chat: &ChatWidget) -> bool {
    read_state_file(chat, PLAN_TRACK_WORKTREE_FILE)
        .and_then(|value| normalize_bool_setting(&value))
        .or_else(|| chat.configured_plan_track_worktree())
        .unwrap_or(true)
}

fn read_plan_track_branch(chat: &ChatWidget) -> bool {
    read_state_file(chat, PLAN_TRACK_BRANCH_FILE)
        .and_then(|value| normalize_bool_setting(&value))
        .or_else(|| chat.configured_plan_track_branch())
        .unwrap_or(true)
}

fn read_plan_mismatch_action(chat: &ChatWidget) -> String {
    read_state_file(chat, PLAN_MISMATCH_ACTION_FILE)
        .or_else(|| {
            chat.configured_plan_mismatch_action()
                .map(ToString::to_string)
        })
        .and_then(|value| normalize_mismatch_action(&value).map(ToString::to_string))
        .unwrap_or_else(|| PLAN_MISMATCH_ACTION_BLOCK.to_string())
}

fn read_plan_naming_strategy(chat: &ChatWidget) -> String {
    read_state_file(chat, PLAN_NAMING_STRATEGY_FILE)
        .or_else(|| {
            chat.configured_plan_naming_strategy()
                .map(ToString::to_string)
        })
        .and_then(|value| normalize_naming_strategy(&value).map(ToString::to_string))
        .unwrap_or_else(|| default_naming_strategy_for_mode(&read_plan_mode(chat)).to_string())
}

fn mode_default_plan_base_dir(chat: &ChatWidget, mode: &str) -> PathBuf {
    match mode {
        PLAN_MODE_ADR_LITE => plan_state_dir(chat),
        _ => plan_state_dir(chat),
    }
}

fn discover_plans(chat: &ChatWidget) -> Vec<PlanEntry> {
    let base_dir = plan_base_dir(chat);
    let mut plans: Vec<PlanEntry> = Vec::new();
    let mut stack = vec![base_dir];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension() != Some(OsStr::new("md")) {
                continue;
            }
            let status = read_plan_status(&path).unwrap_or_else(|| "Draft".to_string());
            let title = read_plan_title(&path).unwrap_or_else(|| "Untitled".to_string());
            plans.push(PlanEntry {
                path,
                status,
                title,
            });
        }
    }

    plans.sort_by(|left, right| left.path.cmp(&right.path));
    plans
}

fn read_plan_status(path: &Path) -> Option<String> {
    plan_file::read_status(path).ok().flatten()
}

fn read_plan_title(path: &Path) -> Option<String> {
    plan_file::read_title(path).ok().flatten()
}

fn count_unchecked_todos(path: &Path) -> usize {
    plan_file::count_unchecked_todos(path).unwrap_or(0)
}

fn read_plan_metadata_value(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) {
            return Some(trimmed.trim_start_matches(key).trim().to_string());
        }
        None
    })
}

fn read_plan_mode_from_file(path: &Path) -> Option<String> {
    read_plan_metadata_value(path, PLAN_FILE_MODE_FIELD)
        .and_then(|value| normalize_plan_mode(&value).map(ToString::to_string))
}

fn read_plan_sync_mode_from_file(path: &Path) -> Option<String> {
    read_plan_metadata_value(path, PLAN_FILE_SYNC_MODE_FIELD)
        .and_then(|value| normalize_plan_sync_mode(&value).map(ToString::to_string))
}

fn ensure_plan_mode_lock(chat: &ChatWidget, path: &Path) -> std::io::Result<(String, bool)> {
    let fallback_mode = read_plan_mode(chat);
    let existing_mode = read_plan_mode_from_file(path);
    let existing_sync_mode = read_plan_sync_mode_from_file(path);
    let locked_mode = existing_mode
        .clone()
        .unwrap_or_else(|| fallback_mode.clone());
    let locked_sync_mode = existing_sync_mode
        .clone()
        .unwrap_or_else(|| effective_sync_mode_for_workflow_mode(chat, &locked_mode).to_string());

    let mut seeded_metadata = false;
    if existing_mode.is_none() {
        upsert_plan_metadata_value(path, PLAN_FILE_MODE_FIELD, &locked_mode)?;
        seeded_metadata = true;
    }
    if existing_sync_mode.is_none() {
        upsert_plan_metadata_value(path, PLAN_FILE_SYNC_MODE_FIELD, &locked_sync_mode)?;
        seeded_metadata = true;
    }

    Ok((locked_mode, seeded_metadata))
}

fn upsert_plan_metadata_value(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if !replaced && line.trim_start().starts_with(key) {
            lines.push(format!("{key} {value}"));
            replaced = true;
            continue;
        }
        lines.push(line.to_string());
    }

    if !replaced {
        let insert_at = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Last updated:"))
            .map(|idx| idx + 1)
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| line.trim_start().starts_with("Started:"))
                    .map(|idx| idx + 1)
            })
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| line.starts_with("# "))
                    .map(|idx| idx + 1)
            })
            .unwrap_or(0);
        lines.insert(insert_at, format!("{key} {value}"));
    }

    plan_file::write_atomic(path, &ensure_trailing_newline(lines.join("\n")))
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

#[derive(Clone, Debug)]
struct PlanContextSnapshot {
    plan_worktree: Option<String>,
    current_worktree: String,
    plan_branch: Option<String>,
    current_branch: String,
}

fn plan_context_snapshot(chat: &ChatWidget, path: &Path) -> PlanContextSnapshot {
    let plan_worktree = read_plan_metadata_value(path, "Worktree:");
    let current_worktree = chat.session_cwd().display().to_string();
    let plan_branch = read_plan_metadata_value(path, "Branch:");
    let current_branch = current_branch_name(chat);
    PlanContextSnapshot {
        plan_worktree,
        current_worktree,
        plan_branch,
        current_branch,
    }
}

fn normalize_worktree_for_compare(raw: &str) -> String {
    std::fs::canonicalize(raw)
        .unwrap_or_else(|_| PathBuf::from(raw))
        .to_string_lossy()
        .to_string()
}

fn plan_context_mismatch_details(chat: &ChatWidget, path: &Path) -> Vec<String> {
    let snapshot = plan_context_snapshot(chat, path);
    let mut mismatches = Vec::new();
    let current_worktree = normalize_worktree_for_compare(&snapshot.current_worktree);

    if read_plan_track_worktree(chat) {
        match snapshot.plan_worktree.as_deref() {
            None => {
                mismatches.push(format!(
                    "worktree metadata missing (`Worktree:`; current=`{}`)",
                    snapshot.current_worktree
                ));
            }
            Some(value) if normalize_worktree_for_compare(value) != current_worktree => {
                mismatches.push(format!(
                    "worktree differs (plan=`{value}`, current=`{}`)",
                    snapshot.current_worktree
                ));
            }
            Some(_) => {}
        }
    }
    if read_plan_track_branch(chat) {
        match snapshot.plan_branch.as_deref() {
            None => {
                mismatches.push(format!(
                    "branch metadata missing (`Branch:`; current=`{}`)",
                    snapshot.current_branch
                ));
            }
            Some(value) if value != snapshot.current_branch => {
                mismatches.push(format!(
                    "branch differs (plan=`{value}`, current=`{}`)",
                    snapshot.current_branch
                ));
            }
            Some(_) => {}
        }
    }

    mismatches
}

fn plan_context_mismatch_summary(chat: &ChatWidget, path: &Path) -> Option<String> {
    if !read_plan_track_worktree(chat) && !read_plan_track_branch(chat) {
        return Some("tracking disabled".to_string());
    }
    let details = plan_context_mismatch_details(chat, path);
    if details.is_empty() {
        return Some("matched".to_string());
    }
    Some(format!(
        "mismatch ({})",
        details
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

fn plan_start_implementation_block_reason(chat: &ChatWidget) -> Option<String> {
    if read_plan_mismatch_action(chat) != PLAN_MISMATCH_ACTION_BLOCK {
        return None;
    }
    let path = read_active_plan_path(chat)?;
    if !path.exists() {
        return None;
    }
    let details = plan_context_mismatch_details(chat, &path);
    if details.is_empty() {
        return None;
    }
    Some(format!(
        "Blocked by plan context mismatch: {}",
        details
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

fn maybe_notify_plan_context_mismatch(chat: &mut ChatWidget, path: &Path, source: &str) {
    let details = plan_context_mismatch_details(chat, path);
    if details.is_empty() {
        return;
    }
    let action = read_plan_mismatch_action(chat);
    let message = format!(
        "Plan context mismatch detected while {source}: {}",
        details
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ")
    );
    if action == PLAN_MISMATCH_ACTION_BLOCK {
        chat.add_error_message(format!(
            "{message}. Start Implementation is blocked until context is resolved."
        ));
    } else {
        chat.add_info_message(
            format!("{message}. Continuing because mismatch-action is `warn`."),
            None,
        );
    }
}

fn active_plan_ui_update(chat: &ChatWidget) -> Option<PlanUiUpdate> {
    let path = read_active_plan_path(chat)?;
    if !path.exists() {
        return None;
    }
    let status = read_plan_status(&path).unwrap_or_else(|| "Unknown".to_string());
    let todos_remaining = count_unchecked_todos(&path);
    Some(PlanUiUpdate {
        path,
        todos_remaining,
        is_done: status.eq_ignore_ascii_case("done"),
    })
}

fn resolve_plan_path(chat: &ChatWidget, raw: &str) -> PathBuf {
    let unquoted = strip_wrapping_quotes(raw.trim());
    let path = PathBuf::from(unquoted);
    if path.is_absolute() {
        path
    } else {
        plan_base_dir(chat).join(path)
    }
}

fn strip_wrapping_quotes(raw: &str) -> &str {
    let has_double = raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2;
    let has_single = raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2;
    if has_double || has_single {
        &raw[1..raw.len() - 1]
    } else {
        raw
    }
}

fn default_new_plan_path(chat: &ChatWidget) -> PathBuf {
    if let Some(active_path) = read_active_plan_path(chat)
        && active_path.exists()
    {
        return active_path;
    }

    let base_dir = plan_base_dir(chat).join(derive_project_name(chat));
    let naming = read_plan_naming_strategy(chat);
    let stem = if naming == PLAN_NAMING_DATE_TITLE {
        date_title_slug(chat)
    } else {
        let mut rng = rand::rng();
        funny_slug(&mut rng)
    };
    let initial = base_dir.join(format!("{stem}.md"));
    if !initial.exists() {
        return initial;
    }
    for suffix in 2..1000 {
        let candidate = base_dir.join(format!("{stem}-{suffix}.md"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base_dir.join(format!("{stem}-{}.md", rand::random::<u16>()))
}

fn derive_project_name(chat: &ChatWidget) -> String {
    if let Some(name) = chat
        .session_cwd()
        .file_name()
        .and_then(OsStr::to_str)
        .map(sanitize_component)
        .filter(|value| !value.is_empty())
    {
        return name;
    }

    std::env::args()
        .next()
        .and_then(|arg| {
            PathBuf::from(arg)
                .file_name()
                .and_then(OsStr::to_str)
                .map(sanitize_component)
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "project".to_string())
}

fn sanitize_component(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn funny_slug(rng: &mut impl rand::Rng) -> String {
    const VERBS: &[&str] = &[
        "shaping",
        "debugging",
        "building",
        "tuning",
        "sketching",
        "mapping",
        "stitching",
        "planning",
    ];
    const ADJECTIVES: &[&str] = &[
        "swift", "curious", "steady", "silver", "bright", "quiet", "nimble", "bold",
    ];
    const ANIMALS: &[&str] = &[
        "otter", "panda", "falcon", "lynx", "dolphin", "fox", "owl", "whale",
    ];

    let verb = VERBS[rng.random_range(0..VERBS.len())];
    let adjective = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let animal = ANIMALS[rng.random_range(0..ANIMALS.len())];
    format!("{verb}-{adjective}-{animal}")
}

fn date_title_slug(chat: &ChatWidget) -> String {
    let today = chrono::Local::now().date_naive().to_string();
    let title = relevant_plan_title_slug(chat);
    format!("{today}-{title}")
}

fn relevant_plan_title_slug(chat: &ChatWidget) -> String {
    let branch = current_branch_name(chat);
    let branch_slug = sanitize_component(&branch);
    let title = if branch_slug.is_empty()
        || matches!(branch_slug.as_str(), "main" | "master" | "unknown")
    {
        derive_project_name(chat)
    } else {
        branch_slug
    };

    title.chars().take(64).collect::<String>()
}

fn default_plan_template(chat: &ChatWidget, path: &Path) -> String {
    let mode = read_plan_mode(chat);
    if mode == PLAN_MODE_CUSTOM {
        if let Ok(template) = std::fs::read_to_string(custom_template_path(chat)) {
            return template;
        }
        let seed = read_custom_seed_mode(chat);
        return default_plan_template_for_mode(chat, path, &seed);
    }
    default_plan_template_for_mode(chat, path, &mode)
}

fn default_plan_template_for_mode(chat: &ChatWidget, path: &Path, mode: &str) -> String {
    if mode == PLAN_MODE_ADR_LITE {
        return default_adr_lite_template(chat, path);
    }
    default_default_template(chat, path)
}

fn custom_template_path(chat: &ChatWidget) -> PathBuf {
    read_state_file(chat, PLAN_CUSTOM_TEMPLATE_FILE)
        .map(PathBuf::from)
        .or_else(|| {
            chat.configured_plan_custom_template()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| plan_state_dir(chat).join("custom").join("template.md"))
}

fn effective_sync_mode_for_workflow_mode(chat: &ChatWidget, mode: &str) -> &'static str {
    if mode == PLAN_MODE_ADR_LITE {
        return PLAN_MODE_ADR_LITE;
    }
    if mode == PLAN_MODE_CUSTOM {
        return if read_custom_seed_mode(chat) == PLAN_MODE_ADR_LITE {
            PLAN_MODE_ADR_LITE
        } else {
            PLAN_MODE_DEFAULT
        };
    }
    PLAN_MODE_DEFAULT
}

fn effective_plan_sync_mode(chat: &ChatWidget, path: Option<&Path>) -> &'static str {
    if let Some(path) = path {
        if let Some(sync_mode) = read_plan_sync_mode_from_file(path)
            && let Some(normalized) = normalize_plan_sync_mode(&sync_mode)
        {
            return normalized;
        }
        if let Some(mode) = read_plan_mode_from_file(path) {
            return effective_sync_mode_for_workflow_mode(chat, &mode);
        }
    }
    let mode = read_plan_mode(chat);
    effective_sync_mode_for_workflow_mode(chat, &mode)
}

fn uses_adr_lite_sync(chat: &ChatWidget, path: Option<&Path>) -> bool {
    effective_plan_sync_mode(chat, path) == PLAN_MODE_ADR_LITE
}

fn default_default_template(chat: &ChatWidget, path: &Path) -> String {
    let today = chrono::Local::now().date_naive();
    format!(
        "# /plan task\n\nStatus: Draft\nTODOs remaining: 3\nStarted: {today}\nLast updated: {today}\nWorktree: {}\nBranch: {}\n\n## Goal\n\n- TODO\n\n## Plan (checklist)\n\n- [ ] Phase 0 — setup foundations\n- [ ] Phase 1 — build on Phase 0\n- [ ] Phase 2 — iterate and validate\n\n## Progress log\n\n- {today}: Created plan file at `{}`.\n",
        chat.session_cwd().display(),
        current_branch_name(chat),
        path.display(),
    )
}

fn default_adr_lite_template(chat: &ChatWidget, path: &Path) -> String {
    let today = chrono::Local::now().date_naive();
    format!(
        "# /plan task — Implementation plan\n\nStatus: Draft\nTODOs remaining: 4\nAllowed statuses: Draft|Active|Paused|Done|Archived\nOwner: {}\nStarted: {today}\nLast updated: {today}\nWorktree: {}\nBranch: {}\n\n## Goal\n\nWhat we are trying to achieve, in 1–3 sentences.\n\n## Scope\n\n- In scope:\n- Out of scope:\n\n## Definitions / contracts\n\n- <term>: <definition>\n\n## Current state / evidence (facts only)\n\n- <fact> (evidence: <path or link>)\n\n## Hypotheses (unconfirmed)\n\n- <hypothesis> (confidence: low|med|high)\n\n## Root causes (confirmed)\n\n- <root cause> (evidence: <path or link>)\n\n## Decisions\n\n- {today}: [Active] Decision: Do X, not Y.\n  - Expected behavior:\n  - Evidence:\n  - Rationale:\n\n## Open questions\n\n- <question> (options: A/B, recommendation)\n\n## Implementation approach (recommended)\n\n- <approach>\n\n## Plan (checklist)\n\n- [ ] Phase 1 — first small shippable milestone\n- [ ] Phase 2 — next milestone\n- [ ] Phase 3 — final milestone\n\n## Acceptance criteria / verification\n\n- [ ] <criterion>\n\n## Progress log\n\n- {today}: Created plan file at `{}`.\n\n## Learnings\n\n- None yet.\n\n## Memories\n\n- None yet.\n",
        plan_owner(),
        chat.session_cwd().display(),
        current_branch_name(chat),
        path.display(),
    )
}

fn plan_owner() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn current_branch_name(chat: &ChatWidget) -> String {
    std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(chat.session_cwd())
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                (!branch.is_empty()).then_some(branch)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_path(path: Option<PathBuf>) -> String {
    path.map(|value| value.display().to_string())
        .unwrap_or_else(|| "(none)".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanListScope {
    Open,
    Closed,
    All,
    Archived,
}

impl PlanListScope {
    const COUNT: usize = 4;

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            "all" => Some(Self::All),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Open,
            1 => Self::Closed,
            2 => Self::All,
            3 => Self::Archived,
            _ => Self::Open,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Open => 0,
            Self::Closed => 1,
            Self::All => 2,
            Self::Archived => 3,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
            Self::Archived => "archived",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::All => "All",
            Self::Archived => "Archived",
        }
    }

    fn matches_status(self, status: &str) -> bool {
        match self {
            Self::Open => OPEN_STATUSES.contains(&status),
            Self::Closed => CLOSED_STATUSES.contains(&status),
            Self::All => !ARCHIVED_STATUSES.contains(&status),
            Self::Archived => ARCHIVED_STATUSES.contains(&status),
        }
    }
}

#[derive(Clone, Debug)]
struct PlanEntry {
    path: PathBuf,
    status: String,
    title: String,
}
