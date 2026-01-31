use crate::chatwidget::ChatWidget;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use ratatui::style::Styled as _;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

pub(crate) fn handle_help_command(chat: &mut ChatWidget, rest: &str) {
    let args: Vec<&str> = rest.split_whitespace().collect();
    let topic = match args.as_slice() {
        [] => {
            add_help_topics_output(chat);
            return;
        }
        [topic] => topic.to_ascii_lowercase(),
        _ => {
            chat.add_info_message("Usage: /help <topic>".to_string(), None);
            return;
        }
    };

    match topic.as_str() {
        "xcodex" => {
            add_help_xcodex_output(chat);
        }
        _ => {
            chat.add_info_message(
                format!("Unknown help topic `{topic}`. Try: /help xcodex"),
                None,
            );
        }
    }
}

pub(crate) fn add_help_topics_output(chat: &mut ChatWidget) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/help".magenta()])]);
    let body = PlainHistoryCell::new(vec![
        vec![
            "Topics: ".into(),
            Span::from("xcodex").set_style(crate::theme::accent_style().bold()),
        ]
        .into(),
        vec![
            "Try: ".dim(),
            Span::from("/help xcodex").set_style(crate::theme::accent_style()),
        ]
        .into(),
    ]);
    chat.add_to_history(CompositeHistoryCell::new(vec![
        Box::new(command),
        Box::new(body),
    ]));
}

pub(crate) fn add_help_xcodex_output(chat: &mut ChatWidget) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/help xcodex".magenta()])]);
    let body = PlainHistoryCell::new(vec![
        vec![
            Span::from("xcodex").set_style(crate::theme::accent_style().bold()),
            " additions in this UI".dim(),
        ]
        .into(),
        vec![
            "• ".dim(),
            "/settings".cyan(),
            " — ".dim(),
            "status bar items (git branch/worktree)".into(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Try: ".dim(),
            "/settings status-bar git-branch toggle".cyan(),
        ]
        .into(),
        vec!["  ".into(), "Try: ".dim(), "/settings worktrees".cyan()].into(),
        vec![
            "• ".dim(),
            "xcodex config".cyan(),
            " — ".dim(),
            "edit or diagnose $CODEX_HOME/config.toml".into(),
        ]
        .into(),
        vec!["  ".into(), "Try: ".dim(), "xcodex config edit".cyan()].into(),
        vec!["  ".into(), "Try: ".dim(), "xcodex config doctor".cyan()].into(),
        vec![
            "• ".dim(),
            "/worktree".cyan(),
            " — ".dim(),
            "switch this session between git worktrees".into(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Contract: ".dim(),
            "tool cwd = active worktree root".into(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Shared dirs (opt-in): ".dim(),
            "linked back to workspace root (writes land there)".into(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Pinned paths (opt-in): ".dim(),
            "worktrees.pinned_paths".cyan(),
            " ".dim(),
            "(file tools only)".dim(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Docs: ".dim(),
            "docs/xcodex/worktrees.md".cyan(),
        ]
        .into(),
        vec!["  ".into(), "Try: ".dim(), "/worktree detect".cyan()].into(),
        vec![
            "  ".into(),
            "Try: ".dim(),
            "/worktree doctor".cyan(),
            " ".dim(),
            "(shared dirs / untracked)".dim(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Try: ".dim(),
            "/worktree link-shared".cyan(),
            " ".dim(),
            "(apply shared-dir links)".dim(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Try: ".dim(),
            "/worktree link-shared --migrate".cyan(),
            " ".dim(),
            "(migrate git-untracked files, then link)".dim(),
        ]
        .into(),
        vec![
            "  ".into(),
            "Try: ".dim(),
            "/worktree shared add docs/impl-plans".cyan(),
            " ".dim(),
            "(configure shared dirs)".dim(),
        ]
        .into(),
        vec![
            "• ".dim(),
            "/ps".cyan(),
            " — ".dim(),
            "list background terminals + hooks".into(),
        ]
        .into(),
        vec![
            "• ".dim(),
            "/ps-kill".cyan(),
            " — ".dim(),
            "terminate background terminals".into(),
        ]
        .into(),
        vec![
            "• ".dim(),
            "/thoughts".cyan(),
            " — ".dim(),
            "show/hide agent reasoning".into(),
        ]
        .into(),
    ]);
    chat.add_to_history(CompositeHistoryCell::new(vec![
        Box::new(command),
        Box::new(body),
    ]));
}
