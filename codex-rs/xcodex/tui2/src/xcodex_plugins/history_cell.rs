pub(crate) use crate::history_cell::BackgroundActivityEntry;
pub(crate) use crate::history_cell::FinalMessageSeparator as XcodexFinalMessageSeparator;
pub(crate) use crate::history_cell::SessionInfoCell;
pub(crate) use crate::history_cell::new_unified_exec_processes_output;

pub(crate) fn session_first_event_command_lines(
    transcript_style: ratatui::style::Style,
) -> Vec<ratatui::prelude::Line<'static>> {
    let _ = transcript_style;
    crate::history_cell::session_first_event_command_lines()
}

pub(crate) fn new_session_info_with_help_lines(
    config: &codex_core::config::Config,
    requested_model: &str,
    event: codex_core::protocol::SessionConfiguredEvent,
    help_lines: Vec<ratatui::prelude::Line<'static>>,
    is_collaboration: bool,
    collaboration_mode: codex_protocol::config_types::CollaborationMode,
) -> SessionInfoCell {
    let _ = (is_collaboration, collaboration_mode);
    crate::history_cell::new_session_info_with_help_lines(
        config,
        requested_model,
        event,
        help_lines,
    )
}
