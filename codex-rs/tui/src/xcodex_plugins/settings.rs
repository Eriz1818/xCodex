use crate::chatwidget::ChatWidget;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) fn add_settings_output_with_values(
    chat: &mut ChatWidget,
    show_git_branch: bool,
    show_worktree: bool,
    transcript_diff_highlight: bool,
    transcript_user_prompt_highlight: bool,
    transcript_syntax_highlight: bool,
) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/settings".magenta()])]);
    let card = crate::status::new_settings_card(
        chat.xtreme_ui_enabled(),
        show_git_branch,
        show_worktree,
        transcript_diff_highlight,
        transcript_user_prompt_highlight,
        transcript_syntax_highlight,
    );
    chat.add_to_history(CompositeHistoryCell::new(vec![Box::new(command), card]));
}

pub(crate) fn handle_settings_command(chat: &mut ChatWidget, rest: &str) -> bool {
    let args: Vec<&str> = rest.split_whitespace().collect();
    let current_git_branch = chat.config_ref().tui_status_bar_show_git_branch;
    let current_worktree = chat.config_ref().tui_status_bar_show_worktree;
    let current_diff_highlight = chat.config_ref().tui_transcript_diff_highlight;
    let current_user_prompt_highlight = chat.config_ref().tui_transcript_user_prompt_highlight;
    let current_syntax_highlight = chat.config_ref().tui_transcript_syntax_highlight;

    let (section, item, action) = match args.as_slice() {
        [] | ["status-bar"] | ["transcript"] => {
            add_settings_output_with_values(
                chat,
                current_git_branch,
                current_worktree,
                current_diff_highlight,
                current_user_prompt_highlight,
                current_syntax_highlight,
            );
            return true;
        }
        ["worktrees"] => {
            crate::xcodex_plugins::worktree::open_worktrees_settings_view(chat);
            return true;
        }
        ["status-bar", item] => ("status-bar", *item, None),
        ["status-bar", item, action] => ("status-bar", *item, Some(*action)),
        ["transcript", item] => ("transcript", *item, None),
        ["transcript", item, action] => ("transcript", *item, Some(*action)),
        _ => {
            chat.add_info_message(
                "Usage: /settings [status-bar|transcript|worktrees]".to_string(),
                None,
            );
            return true;
        }
    };

    let mut next_git_branch = current_git_branch;
    let mut next_worktree = current_worktree;
    let mut next_diff_highlight = current_diff_highlight;
    let mut next_user_prompt_highlight = current_user_prompt_highlight;
    let mut next_syntax_highlight = current_syntax_highlight;

    let item = item.to_ascii_lowercase();
    let action = action.map(str::to_ascii_lowercase);

    enum SettingsAction {
        Toggle,
        Set(bool),
        Status,
    }

    let action = match action.as_deref() {
        None | Some("toggle") => SettingsAction::Toggle,
        Some("on") | Some("enable") | Some("true") => SettingsAction::Set(true),
        Some("off") | Some("disable") | Some("false") => SettingsAction::Set(false),
        Some("status") | Some("show") => SettingsAction::Status,
        Some(_) => {
            match section {
                "status-bar" => chat.add_info_message(
                    "Usage: /settings status-bar <git-branch|worktree> [on|off|toggle|status]"
                        .to_string(),
                    None,
                ),
                "transcript" => chat.add_info_message(
                    "Usage: /settings transcript <diff-highlight|highlight-past-prompts|syntax-highlight> [on|off|toggle|status]"
                        .to_string(),
                    None,
                ),
                _ => {}
            }
            return true;
        }
    };

    match section {
        "status-bar" => {
            let selected = match item.as_str() {
                "git-branch" | "branch" => Some((&mut next_git_branch, current_git_branch)),
                "worktree" | "worktree-path" => Some((&mut next_worktree, current_worktree)),
                _ => None,
            };
            let Some((selected, current)) = selected else {
                chat.add_info_message(
                    "Unknown setting. Use: git-branch | worktree".to_string(),
                    None,
                );
                return true;
            };

            let next = match action {
                SettingsAction::Toggle => Some(!current),
                SettingsAction::Set(value) => Some(value),
                SettingsAction::Status => None,
            };
            if let Some(value) = next {
                *selected = value;
                chat.app_event_tx()
                    .send(crate::app_event::AppEvent::UpdateStatusBarGitOptions {
                        show_git_branch: next_git_branch,
                        show_worktree: next_worktree,
                    });
                chat.app_event_tx()
                    .send(crate::app_event::AppEvent::PersistStatusBarGitOptions {
                        show_git_branch: next_git_branch,
                        show_worktree: next_worktree,
                    });
            }
        }
        "transcript" => {
            if item.as_str() != "diff-highlight"
                && item.as_str() != "highlight-past-prompts"
                && item.as_str() != "syntax-highlight"
            {
                chat.add_info_message(
                    "Unknown setting. Use: diff-highlight | highlight-past-prompts | syntax-highlight"
                        .to_string(),
                    None,
                );
                return true;
            }
            if item.as_str() == "diff-highlight" {
                let next = match action {
                    SettingsAction::Toggle => Some(!current_diff_highlight),
                    SettingsAction::Set(value) => Some(value),
                    SettingsAction::Status => None,
                };
                if let Some(value) = next {
                    next_diff_highlight = value;
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::UpdateTranscriptDiffHighlight(
                            next_diff_highlight,
                        ),
                    );
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::PersistTranscriptDiffHighlight(
                            next_diff_highlight,
                        ),
                    );
                }
            } else if item.as_str() == "highlight-past-prompts" {
                let next = match action {
                    SettingsAction::Toggle => Some(!current_user_prompt_highlight),
                    SettingsAction::Set(value) => Some(value),
                    SettingsAction::Status => None,
                };
                if let Some(value) = next {
                    next_user_prompt_highlight = value;
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::UpdateTranscriptUserPromptHighlight(
                            next_user_prompt_highlight,
                        ),
                    );
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::PersistTranscriptUserPromptHighlight(
                            next_user_prompt_highlight,
                        ),
                    );
                }
            } else {
                let next = match action {
                    SettingsAction::Toggle => Some(!current_syntax_highlight),
                    SettingsAction::Set(value) => Some(value),
                    SettingsAction::Status => None,
                };
                if let Some(value) = next {
                    next_syntax_highlight = value;
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::UpdateTranscriptSyntaxHighlight(
                            next_syntax_highlight,
                        ),
                    );
                    chat.app_event_tx().send(
                        crate::app_event::AppEvent::PersistTranscriptSyntaxHighlight(
                            next_syntax_highlight,
                        ),
                    );
                }
            }
        }
        _ => {}
    }

    add_settings_output_with_values(
        chat,
        next_git_branch,
        next_worktree,
        next_diff_highlight,
        next_user_prompt_highlight,
        next_syntax_highlight,
    );
    true
}
