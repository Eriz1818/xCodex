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
) {
    let command = PlainHistoryCell::new(vec![Line::from(vec!["/settings".magenta()])]);
    let card = crate::status::new_settings_card(
        chat.xtreme_ui_enabled(),
        show_git_branch,
        show_worktree,
        transcript_diff_highlight,
        transcript_user_prompt_highlight,
    );
    chat.add_to_history(CompositeHistoryCell::new(vec![Box::new(command), card]));
}
