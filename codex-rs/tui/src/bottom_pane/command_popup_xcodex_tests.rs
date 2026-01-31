use super::command_popup::CommandItem;
use super::command_popup::CommandPopup;
use super::command_popup::CommandPopupFlags;
use super::command_popup::DEFAULT_SLASH_POPUP_ROWS;
use super::slash_subcommands::subcommand_list_hint;
use crate::slash_command::SlashCommand;
use codex_protocol::custom_prompts::CustomPrompt;
use pretty_assertions::assert_eq;

fn popup_flags() -> CommandPopupFlags {
    CommandPopupFlags {
        collaboration_modes_enabled: false,
        connectors_enabled: false,
        personality_command_enabled: true,
        windows_degraded_sandbox_active: false,
    }
}

fn popup_with_prompts(prompts: Vec<CustomPrompt>) -> CommandPopup {
    CommandPopup::new(prompts, popup_flags(), DEFAULT_SLASH_POPUP_ROWS)
}

#[test]
fn filter_includes_thoughts_plugin_command() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/tho".to_string());

    let matches = popup.filtered_items();
    let has_thoughts = matches.iter().any(|item| match item {
        CommandItem::BuiltinText { name, .. } => *name == "thoughts",
        CommandItem::Builtin(_) => false,
        CommandItem::ArgValue { .. } => false,
        CommandItem::UserPrompt(_) => false,
    });
    assert!(
        has_thoughts,
        "expected '/thoughts' to appear among filtered commands"
    );
}

#[test]
fn prompt_is_suggested_when_filter_matches_prompt_name() {
    let mut popup = popup_with_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: "hello from prompt".to_string(),
        description: None,
        argument_hint: None,
    }]);
    popup.on_composer_text_change("/my".to_string());
    let items = popup.filtered_items();
    let has_prompt = items
        .into_iter()
        .any(|item| matches!(item, CommandItem::UserPrompt(_)));
    assert!(has_prompt, "expected /my to suggest the custom prompt");
}

#[test]
fn worktree_subcommands_are_suggested_under_worktree() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree ".to_string());

    let items = popup.filtered_items();
    assert!(
        !items
            .iter()
            .any(|item| matches!(item, CommandItem::Builtin(SlashCommand::Worktree))),
        "expected /worktree root command to be hidden in subcommand context"
    );
    assert!(
        items
            .iter()
            .any(|item| matches!(item, CommandItem::BuiltinText { .. })),
        "expected at least one /worktree subcommand suggestion under /worktree"
    );
}

#[test]
fn worktree_subcommand_hint_uses_plugin_order() {
    let hint = subcommand_list_hint("worktree").expect("worktree hint");
    assert_eq!(
        hint,
        "Type space for subcommands: detect, doctor, init, shared, link-shared"
    );
}

#[test]
fn settings_subcommands_are_suggested_under_settings() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/settings ".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"settings status-bar") && subcommands.contains(&"settings worktrees"),
        "expected /settings to suggest subcommands, got {subcommands:?}"
    );
}

#[test]
fn settings_nested_subcommands_are_suggested_under_status_bar() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/settings status-bar ".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"settings status-bar git-branch")
            && subcommands.contains(&"settings status-bar worktree"),
        "expected /settings status-bar to suggest nested subcommands, got {subcommands:?}"
    );
}

#[test]
fn mcp_subcommands_are_suggested_under_mcp() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/mcp ".to_string());

    let items = popup.filtered_items();
    assert!(
        !items
            .iter()
            .any(|item| matches!(item, CommandItem::Builtin(SlashCommand::Mcp))),
        "expected /mcp root command to be hidden in subcommand context"
    );

    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"mcp retry") && subcommands.contains(&"mcp timeout"),
        "expected /mcp to suggest subcommands, got {subcommands:?}"
    );
}

#[test]
fn mcp_retry_subcommands_are_suggested_under_retry() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/mcp retry ".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"mcp retry failed"),
        "expected /mcp retry to suggest failed, got {subcommands:?}"
    );
}

#[test]
fn worktree_subcommands_are_hidden_until_space() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree".to_string());

    let items = popup.filtered_items();
    assert!(
        !items.iter().any(|item| {
            matches!(item, CommandItem::BuiltinText { name, .. } if name.starts_with("worktree "))
        }),
        "expected no /worktree subcommand suggestions without a trailing space"
    );
}

#[test]
fn arrow_key_selection_is_not_reset_by_popup_sync() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree ".to_string());

    let first = popup.selected_item();
    popup.move_down();
    let moved = popup.selected_item();
    assert_ne!(first, moved, "expected move_down to change selection");

    // Simulate redundant sync calls (e.g. after an Up/Down key event).
    popup.on_composer_text_change("/worktree ".to_string());
    assert_eq!(
        popup.selected_item(),
        moved,
        "expected selection to persist across redundant sync"
    );
}

#[test]
fn worktree_subcommands_filter_by_prefix() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree d".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"worktree detect") && subcommands.contains(&"worktree doctor"),
        "expected /worktree d to suggest detect/doctor, got {subcommands:?}"
    );
}

#[test]
fn subcommand_context_hides_other_root_suggestions() {
    let prompts = vec![CustomPrompt {
        name: "worktree-helper".to_string(),
        path: "/tmp/worktree-helper.md".to_string().into(),
        content: "hello".to_string(),
        description: None,
        argument_hint: None,
    }];

    let mut popup = popup_with_prompts(prompts);
    popup.on_composer_text_change("/worktree d".to_string());

    let items = popup.filtered_items();
    assert!(
        items.iter().all(|item| {
            matches!(
                item,
                CommandItem::BuiltinText { .. } | CommandItem::ArgValue { .. }
            )
        }),
        "expected subcommand context to hide other root suggestions, got {items:?}"
    );
}

#[test]
fn selection_does_not_reset_when_refreshing_popup() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree ".to_string());

    popup.move_down();
    let moved = popup.selected_item();
    assert!(moved.is_some(), "expected selection after moving down");

    popup.on_composer_text_change("/worktree ".to_string());
    assert_eq!(
        popup.selected_item(),
        moved,
        "expected selection to persist across refresh"
    );
}

#[test]
fn worktree_nested_subcommands_are_suggested_under_shared() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree shared ".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"worktree shared add")
            && subcommands.contains(&"worktree shared rm")
            && subcommands.contains(&"worktree shared list"),
        "expected /worktree shared to suggest nested subcommands, got {subcommands:?}"
    );
    assert!(
        !subcommands.contains(&"worktree detect"),
        "expected /worktree shared suggestions to be scoped (no detect), got {subcommands:?}"
    );
}

#[test]
fn worktree_leaf_subcommand_stays_visible_while_typing_args() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree shared add docs/impl-plans".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"worktree shared add"),
        "expected leaf subcommand to stay visible while typing args, got {subcommands:?}"
    );
}

#[test]
fn worktree_leaf_subcommand_stays_visible_after_trailing_space_and_args() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree init foo ".to_string());

    let items = popup.filtered_items();
    let subcommands: Vec<&str> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::BuiltinText { name, .. } => Some(name),
            _ => None,
        })
        .collect();

    assert!(
        subcommands.contains(&"worktree init"),
        "expected leaf subcommand to stay visible after a trailing space and args, got {subcommands:?}"
    );
}

#[test]
fn worktree_init_description_includes_next_arg_hint() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree init foo ".to_string());

    let rows = popup.rows_from_matches(popup.filtered());
    let init = rows
        .iter()
        .find(|row| row.name == "/worktree init")
        .and_then(|row| row.description.as_deref())
        .unwrap_or_default();

    assert!(
        init.contains("Next: <branch>"),
        "expected /worktree init row to include next-arg hint, got {init:?}"
    );
}

#[test]
fn worktree_init_branch_arg_suggests_branches() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.set_slash_completion_branches(vec!["main".to_string(), "feature".to_string()]);
    popup.set_current_git_branch(Some("feature".to_string()));
    popup.on_composer_text_change("/worktree init foo ".to_string());

    let items = popup.filtered_items();
    let values: Vec<String> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::ArgValue { display, .. } => Some(display),
            _ => None,
        })
        .collect();

    assert!(
        values.contains(&"feature".to_string()) && values.contains(&"main".to_string()),
        "expected branch suggestions to include current and default branches, got {values:?}"
    );
}

#[test]
fn worktree_init_path_arg_suggests_default_path() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree init feat/x main ".to_string());

    let items = popup.filtered_items();
    let values: Vec<String> = items
        .into_iter()
        .filter_map(|item| match item {
            CommandItem::ArgValue { display, .. } => Some(display),
            _ => None,
        })
        .collect();

    assert!(
        values.contains(&".worktrees/feat-x".to_string()),
        "expected path suggestions to include default .worktrees slug, got {values:?}"
    );
}

#[test]
fn default_selection_prefers_subcommands_in_worktree_context() {
    let mut popup = popup_with_prompts(Vec::new());
    popup.on_composer_text_change("/worktree shar".to_string());
    assert!(
        matches!(popup.selected_item(), Some(CommandItem::BuiltinText { .. })),
        "expected subcommand to be selected by default for /worktree context"
    );
}
