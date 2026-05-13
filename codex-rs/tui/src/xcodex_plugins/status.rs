use crate::chatwidget::ChatWidget;
use crate::history_cell::HistoryCell;
use crate::status;
use chrono::Local;
use codex_core::protocol::TokenUsage;

pub(crate) fn status_menu_status_cell(
    chat: &ChatWidget,
    rate_limit_snapshots: &[status::RateLimitSnapshotDisplay],
    agents_summary: String,
) -> Box<dyn HistoryCell> {
    let default_usage = TokenUsage::default();
    let token_info = chat.token_info();
    let total_usage = token_info
        .map(|ti| &ti.total_token_usage)
        .unwrap_or(&default_usage);
    let session_stats = (!chat.session_stats().is_empty()).then_some(chat.session_stats());
    let collaboration_mode = chat.collaboration_mode_label();
    let reasoning_effort_override = Some(chat.effective_reasoning_effort());
    status::new_status_menu_summary_card_with_account_display_many(
        chat.config_ref(),
        chat.status_account_display(),
        token_info,
        total_usage,
        &chat.thread_id(),
        chat.thread_name(),
        session_stats,
        chat.forked_from(),
        rate_limit_snapshots,
        chat.plan_type(),
        Local::now(),
        chat.model_display_name(),
        collaboration_mode,
        reasoning_effort_override,
        agents_summary,
    )
}
