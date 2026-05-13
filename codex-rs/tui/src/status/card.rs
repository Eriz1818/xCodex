use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::with_border_with_inner_width;
use crate::legacy_core::DEFAULT_PROJECT_DOC_FILENAME;
use crate::legacy_core::LOCAL_PROJECT_DOC_FILENAME;
use crate::legacy_core::config::Config;
use crate::version::CODEX_CLI_VERSION;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crate::xtreme;
use chrono::DateTime;
use chrono::Local;
use codex_model_provider_info::WireApi;
use codex_protocol::ThreadId;
use codex_protocol::account::PlanType;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::NetworkAccess;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TokenUsageInfo;
use codex_utils_sandbox_summary::summarize_sandbox_policy;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use unicode_width::UnicodeWidthStr;
use url::Url;

use super::account::StatusAccountDisplay;
use super::format::FieldFormatter;
use super::format::line_display_width;
use super::format::push_label;
use super::format::truncate_line_to_width;
use super::helpers::compose_account_display;
use super::helpers::compose_agents_summary;
use super::helpers::compose_model_display;
use super::helpers::format_directory_display;
use super::helpers::format_tokens_compact;
use super::rate_limits::RateLimitSnapshotDisplay;
use super::rate_limits::StatusRateLimitData;
use super::rate_limits::StatusRateLimitRow;
use super::rate_limits::StatusRateLimitValue;
use super::rate_limits::compose_rate_limit_data;
use super::rate_limits::compose_rate_limit_data_many;
use super::rate_limits::format_status_limit_summary;
use super::rate_limits::render_status_limit_progress_bar;

#[derive(Debug, Clone)]
struct StatusContextWindowData {
    percent_remaining: i64,
    tokens_in_context: i64,
    budget_window: i64,
    full_window: Option<i64>,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusTokenUsageData {
    total: i64,
    input: i64,
    output: i64,
    context_window: Option<StatusContextWindowData>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionStats {
    pub(crate) turns_completed: usize,
    pub(crate) exec_commands: usize,
    pub(crate) mcp_calls: usize,
    pub(crate) patches: usize,
    pub(crate) files_changed: usize,
    pub(crate) approvals_requested: usize,
    pub(crate) tests_run: usize,
}

impl SessionStats {
    pub(crate) fn is_empty(&self) -> bool {
        self.turns_completed == 0
            && self.exec_commands == 0
            && self.mcp_calls == 0
            && self.patches == 0
            && self.files_changed == 0
            && self.approvals_requested == 0
            && self.tests_run == 0
    }

    fn to_summary_parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.turns_completed > 0 {
            parts.push(format!("{} turns", self.turns_completed));
        }
        if self.exec_commands > 0 {
            parts.push(format!("{} cmds", self.exec_commands));
        }
        if self.mcp_calls > 0 {
            parts.push(format!("{} tools", self.mcp_calls));
        }
        if self.patches > 0 {
            parts.push(format!("{} edits", self.patches));
        }
        if self.files_changed > 0 {
            parts.push(format!("{} files", self.files_changed));
        }
        if self.tests_run > 0 {
            parts.push(format!("{} tests", self.tests_run));
        }
        if self.approvals_requested > 0 {
            parts.push(format!("{} approvals", self.approvals_requested));
        }
        parts
    }

    fn spans(&self) -> Vec<Span<'static>> {
        let parts = self.to_summary_parts();
        if parts.is_empty() {
            return Vec::new();
        }

        vec![Span::from(parts.join(" · "))]
    }
}

#[derive(Debug)]
struct StatusRateLimitState {
    rate_limits: StatusRateLimitData,
    refreshing_rate_limits: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusHistoryHandle {
    rate_limit_state: Arc<RwLock<StatusRateLimitState>>,
}

impl StatusHistoryHandle {
    pub(crate) fn finish_rate_limit_refresh(
        &self,
        rate_limits: &[RateLimitSnapshotDisplay],
        now: DateTime<Local>,
    ) {
        let rate_limits = if rate_limits.len() <= 1 {
            compose_rate_limit_data(rate_limits.first(), now)
        } else {
            compose_rate_limit_data_many(rate_limits, now)
        };
        #[expect(clippy::expect_used)]
        let mut state = self
            .rate_limit_state
            .write()
            .expect("status history rate-limit state poisoned");
        state.rate_limits = rate_limits;
        state.refreshing_rate_limits = false;
    }
}

#[derive(Debug)]
struct StatusHistoryCell {
    ui_frontend: String,
    model_name: String,
    model_details: Vec<String>,
    directory: PathBuf,
    codex_home: PathBuf,
    approval_policy: AskForApproval,
    sandbox_policy: SandboxPolicy,
    approval: String,
    sandbox: String,
    agents_summary: String,
    auto_compact_enabled: bool,
    hide_agent_reasoning: bool,
    collaboration_mode: Option<String>,
    model_provider: Option<String>,
    account: Option<StatusAccountDisplay>,
    thread_name: Option<String>,
    session_id: Option<String>,
    session_stats: Option<SessionStats>,
    forked_from: Option<String>,
    token_usage: StatusTokenUsageData,
    rate_limit_state: Arc<RwLock<StatusRateLimitState>>,
    xtreme_ui_enabled: bool,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    _plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> CompositeHistoryCell {
    let snapshots = rate_limits.map(std::slice::from_ref).unwrap_or_default();
    new_status_output_inner(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        /*session_stats*/ None,
        forked_from,
        snapshots,
        _plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        compat_compose_agents_summary(config),
        /*refreshing_rate_limits*/ false,
    )
    .0
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output_with_session_stats(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    session_stats: Option<&SessionStats>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> CompositeHistoryCell {
    let snapshots = rate_limits.map(std::slice::from_ref).unwrap_or_default();
    new_status_output_inner(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        session_stats,
        forked_from,
        snapshots,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        compat_compose_agents_summary(config),
        /*refreshing_rate_limits*/ false,
    )
    .0
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output_with_rate_limits(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: &[RateLimitSnapshotDisplay],
    _plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
    refreshing_rate_limits: bool,
) -> CompositeHistoryCell {
    new_status_output_inner(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        /*session_stats*/ None,
        forked_from,
        rate_limits,
        _plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        compat_compose_agents_summary(config),
        refreshing_rate_limits,
    )
    .0
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output_with_rate_limits_handle(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: &[RateLimitSnapshotDisplay],
    _plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
    agents_summary: String,
    refreshing_rate_limits: bool,
) -> (CompositeHistoryCell, StatusHistoryHandle) {
    new_status_output_inner(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        /*session_stats*/ None,
        forked_from,
        rate_limits,
        _plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        agents_summary,
        refreshing_rate_limits,
    )
}

#[allow(clippy::too_many_arguments)]
fn new_status_output_inner(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    session_stats: Option<&SessionStats>,
    forked_from: Option<ThreadId>,
    rate_limits: &[RateLimitSnapshotDisplay],
    _plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
    agents_summary: String,
    refreshing_rate_limits: bool,
) -> (CompositeHistoryCell, StatusHistoryHandle) {
    let command = PlainHistoryCell::new(vec!["/status".magenta().into()]);
    let (card, handle) = StatusHistoryCell::new(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        session_stats,
        forked_from,
        rate_limits,
        _plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        agents_summary,
        refreshing_rate_limits,
    );

    (
        CompositeHistoryCell::new(vec![Box::new(command), Box::new(card)]),
        handle,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_menu_summary_card(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> Box<dyn HistoryCell> {
    new_status_menu_summary_card_with_session_stats(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        /*session_stats*/ None,
        forked_from,
        rate_limits,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_menu_summary_card_with_session_stats(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    session_stats: Option<&SessionStats>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> Box<dyn HistoryCell> {
    let snapshots = rate_limits.map(std::slice::from_ref).unwrap_or_default();
    let (cell, _) = StatusHistoryCell::new(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        session_stats,
        forked_from,
        snapshots,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        compat_compose_agents_summary(config),
        /*refreshing_rate_limits*/ false,
    );
    Box::new(StatusMenuSummaryCell(cell))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_menu_summary_card_with_account_display(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    session_stats: Option<&SessionStats>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> Box<dyn HistoryCell> {
    let snapshots = rate_limits.map(std::slice::from_ref).unwrap_or_default();
    let (cell, _) = StatusHistoryCell::new(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        session_stats,
        forked_from,
        snapshots,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        compat_compose_agents_summary(config),
        /*refreshing_rate_limits*/ false,
    );
    Box::new(StatusMenuSummaryCell(cell))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_menu_summary_card_with_account_display_many(
    config: &Config,
    account_display: Option<&StatusAccountDisplay>,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    session_stats: Option<&SessionStats>,
    forked_from: Option<ThreadId>,
    rate_limits: &[RateLimitSnapshotDisplay],
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
    agents_summary: String,
) -> Box<dyn HistoryCell> {
    let (cell, _) = StatusHistoryCell::new(
        config,
        account_display,
        token_info,
        total_usage,
        session_id,
        thread_name,
        session_stats,
        forked_from,
        rate_limits,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        agents_summary,
        /*refreshing_rate_limits*/ false,
    );
    Box::new(StatusMenuSummaryCell(cell))
}

pub(crate) fn new_settings_card(
    xtreme_ui_enabled: bool,
    show_model: bool,
    show_git_branch: bool,
    show_worktree: bool,
    transcript_diff_highlight: bool,
    transcript_side_by_side: bool,
    transcript_user_prompt_highlight: bool,
    transcript_syntax_highlight: bool,
) -> Box<dyn HistoryCell> {
    Box::new(SettingsHistoryCell {
        xtreme_ui_enabled,
        show_model,
        show_git_branch,
        show_worktree,
        transcript_diff_highlight,
        transcript_side_by_side,
        transcript_user_prompt_highlight,
        transcript_syntax_highlight,
    })
}

#[derive(Debug)]
struct SettingsHistoryCell {
    xtreme_ui_enabled: bool,
    show_model: bool,
    show_git_branch: bool,
    show_worktree: bool,
    transcript_diff_highlight: bool,
    transcript_side_by_side: bool,
    transcript_user_prompt_highlight: bool,
    transcript_syntax_highlight: bool,
}

impl HistoryCell for SettingsHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let available_inner_width = usize::from(width.saturating_sub(4));
        if available_inner_width == 0 {
            return Vec::new();
        }

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut title_spans = vec![FieldFormatter::INDENT.dim()];
        title_spans.extend(xtreme::title_prefix_spans(self.xtreme_ui_enabled));
        title_spans.extend([
            "xtreme-Codex".bold(),
            " ".dim(),
            format!("(v{CODEX_CLI_VERSION})").dim(),
        ]);
        lines.push(Line::from(title_spans));
        lines.push(Line::from(Vec::<Span<'static>>::new()));

        lines.push("Status bar items".bold().into());
        for (label, enabled) in [
            ("Model name", self.show_model),
            ("Git branch", self.show_git_branch),
            ("Active worktree path", self.show_worktree),
        ] {
            let checkbox = if enabled { "[x]" } else { "[ ]" };
            lines.push(Line::from(format!("  {checkbox} {label}")));
        }

        lines.push(Line::from(Vec::<Span<'static>>::new()));
        lines.push("Transcript".bold().into());
        for (label, enabled) in [
            ("Diff highlight", self.transcript_diff_highlight),
            ("Side-by-side diff", self.transcript_side_by_side),
            (
                "Highlight past prompts",
                self.transcript_user_prompt_highlight,
            ),
            ("Syntax highlight code", self.transcript_syntax_highlight),
        ] {
            let checkbox = if enabled { "[x]" } else { "[ ]" };
            lines.push(Line::from(format!("  {checkbox} {label}")));
        }

        lines.push(Line::from(Vec::<Span<'static>>::new()));
        lines.push(
            vec![
                "Usage: ".dim(),
                "/settings status-bar <git-branch|worktree> [on|off|toggle|status]".cyan(),
            ]
            .into(),
        );
        lines.push(
            vec![
                "Usage: ".dim(),
                "/settings transcript <diff-highlight|side-by-side|highlight-past-prompts|syntax-highlight> [on|off|toggle|status]"
                    .cyan(),
            ]
            .into(),
        );

        let content_width = lines.iter().map(line_display_width).max().unwrap_or(0);
        let inner_width = content_width.min(available_inner_width);
        let truncated_lines: Vec<Line<'static>> = lines
            .into_iter()
            .map(|line| truncate_line_to_width(line, inner_width))
            .collect();

        with_border_with_inner_width(truncated_lines, inner_width)
    }
}

#[derive(Debug)]
struct StatusMenuSummaryCell(StatusHistoryCell);

impl HistoryCell for StatusMenuSummaryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let available_width = usize::from(width);
        if available_width == 0 {
            return Vec::new();
        }

        let status = &self.0;
        #[expect(clippy::expect_used)]
        let rate_limit_state = status
            .rate_limit_state
            .read()
            .expect("status history rate-limit state poisoned");
        let limit_bar_indent = status_menu_limit_bar_indent(&rate_limit_state);

        let mut labels: Vec<&'static str> = vec![
            "UI",
            "Model",
            "Directory",
            "Approval",
            "Sandbox",
            "Agents.md",
            "Auto-compact",
            "Thoughts",
        ];

        if status.session_id.is_some() {
            labels.push("Session");
        }
        if status.session_id.is_some() && status.forked_from.is_some() {
            labels.push("Forked from");
        }
        if !matches!(status.account, Some(StatusAccountDisplay::ChatGpt { .. })) {
            labels.push("Token usage");
        }
        if status.token_usage.context_window.is_some() {
            labels.push("Context");
        }
        labels.push("Limits");

        let formatter = FieldFormatter::from_labels(labels.iter().copied());
        let value_width = formatter.value_width(available_width);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let mut model_spans = vec![Span::from(status.model_name.clone())];
        if !status.model_details.is_empty() {
            model_spans.push(" (".dim());
            model_spans.push(Span::from(status.model_details.join(", ")).dim());
            model_spans.push(")".dim());
        }

        let directory_value = format_directory_display(&status.directory, Some(value_width));

        lines.push(formatter.line("UI", vec![Span::from(status.ui_frontend.clone())]));
        lines.push(formatter.line("Model", model_spans));
        lines.push(formatter.line("Directory", vec![Span::from(directory_value)]));
        lines.push(formatter.line("Approval", vec![Span::from(status.approval.clone())]));
        lines.push(formatter.line("Sandbox", vec![Span::from(status.sandbox.clone())]));
        lines.push(formatter.line("Agents.md", vec![Span::from(status.agents_summary.clone())]));
        lines.push(formatter.line(
            "Auto-compact",
            vec![Span::from(if status.auto_compact_enabled {
                "enabled"
            } else {
                "disabled"
            })],
        ));
        lines.push(formatter.line(
            "Thoughts",
            vec![Span::from(if status.hide_agent_reasoning {
                "hidden"
            } else {
                "shown"
            })],
        ));

        if let Some(session) = status.session_id.as_ref() {
            lines.push(formatter.line("Session", vec![Span::from(session.clone())]));
        }
        if status.session_id.is_some() {
            if let Some(forked_from) = status.forked_from.as_ref() {
                lines.push(formatter.line("Forked from", vec![Span::from(forked_from.clone())]));
            }
        }

        if !matches!(status.account, Some(StatusAccountDisplay::ChatGpt { .. })) {
            lines.push(formatter.line("Token usage", status.token_usage_spans()));
        }
        if let Some(spans) = status.context_window_spans_for_menu(limit_bar_indent) {
            lines.push(formatter.line("Context", spans));
        }

        lines.extend(status_menu_limit_lines(&rate_limit_state, &formatter));

        lines
            .into_iter()
            .map(|line| truncate_line_to_width(line, available_width))
            .collect()
    }
}

fn status_menu_limit_lines(
    state: &StatusRateLimitState,
    formatter: &FieldFormatter,
) -> Vec<Line<'static>> {
    let rows = match &state.rate_limits {
        StatusRateLimitData::Available(rows) | StatusRateLimitData::Stale(rows) => rows,
        StatusRateLimitData::Unavailable => {
            return vec![formatter.line("Limits", vec!["not available for this account".dim()])];
        }
        StatusRateLimitData::Missing => {
            let text = if state.refreshing_rate_limits {
                "refresh requested; run /status again shortly."
            } else {
                "data not available yet"
            };
            return vec![formatter.line("Limits", vec![Span::from(text).dim()])];
        }
    };

    let Some(row) = rows.first() else {
        return vec![formatter.line("Limits", vec!["data not available yet".dim()])];
    };

    let max_label_width = status_menu_limit_label_width(state).max(1);
    let mut lines = Vec::new();

    for (idx, row) in rows.iter().enumerate() {
        let padding = max_label_width.saturating_sub(UnicodeWidthStr::width(row.label.as_str()));
        let label_prefix = format!("{}{} ", row.label, " ".repeat(padding));
        let value_spans = match &row.value {
            StatusRateLimitValue::Window {
                percent_used,
                resets_at,
            } => {
                let percent_remaining = (100.0 - percent_used).clamp(0.0, 100.0);
                let mut spans = vec![
                    Span::from(label_prefix),
                    Span::from(render_status_limit_progress_bar(percent_remaining)),
                    Span::from(" "),
                    Span::from(format_status_limit_summary(percent_remaining)),
                ];
                if let Some(resets_at) = resets_at.as_ref() {
                    spans.push(" ".dim());
                    spans.push(Span::from(format!("(resets {resets_at})")).dim());
                }
                spans
            }
            StatusRateLimitValue::Text(text) => vec![Span::from(format!("{label_prefix}{text}"))],
        };

        if idx == 0 {
            lines.push(formatter.line("Limits", value_spans));
        } else {
            lines.push(formatter.continuation(value_spans));
        }
    }

    if lines.is_empty() {
        lines.push(formatter.line("Limits", vec![Span::from(row.label.clone())]));
    }

    lines
}

impl StatusHistoryCell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: &Config,
        account_display: Option<&StatusAccountDisplay>,
        token_info: Option<&TokenUsageInfo>,
        total_usage: &TokenUsage,
        session_id: &Option<ThreadId>,
        thread_name: Option<String>,
        session_stats: Option<&SessionStats>,
        forked_from: Option<ThreadId>,
        rate_limits: &[RateLimitSnapshotDisplay],
        _plan_type: Option<PlanType>,
        now: DateTime<Local>,
        model_name: &str,
        collaboration_mode: Option<&str>,
        reasoning_effort_override: Option<Option<ReasoningEffort>>,
        agents_summary: String,
        refreshing_rate_limits: bool,
    ) -> (Self, StatusHistoryHandle) {
        let mut config_entries = vec![
            ("workdir", config.cwd.display().to_string()),
            ("model", model_name.to_string()),
            ("provider", config.model_provider_id.clone()),
            (
                "approval",
                config.permissions.approval_policy.value().to_string(),
            ),
            (
                "sandbox",
                summarize_sandbox_policy(config.permissions.sandbox_policy.get()),
            ),
        ];
        if config.model_provider.wire_api == WireApi::Responses {
            let effort_value = reasoning_effort_override
                .unwrap_or(None)
                .map(|effort| effort.to_string())
                .unwrap_or_else(|| "none".to_string());
            config_entries.push(("reasoning effort", effort_value));
            config_entries.push((
                "reasoning summaries",
                config
                    .model_reasoning_summary
                    .map(|summary| summary.to_string())
                    .unwrap_or_else(|| "auto".to_string()),
            ));
        }

        let (model_name, model_details) = compose_model_display(model_name, &config_entries);
        let approval_policy = config.permissions.approval_policy.value();
        let sandbox_policy = config.permissions.sandbox_policy.get().clone();
        let approval = config_entries
            .iter()
            .find(|(key, _)| *key == "approval")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        let sandbox = match &sandbox_policy {
            SandboxPolicy::DangerFullAccess => "danger-full-access".to_string(),
            SandboxPolicy::ReadOnly { .. } => "read-only".to_string(),
            SandboxPolicy::WorkspaceWrite {
                network_access: true,
                ..
            } => "workspace-write with network access".to_string(),
            SandboxPolicy::WorkspaceWrite { .. } => "workspace-write".to_string(),
            SandboxPolicy::ExternalSandbox { network_access } => {
                if matches!(network_access, NetworkAccess::Enabled) {
                    "external-sandbox (network access enabled)".to_string()
                } else {
                    "external-sandbox".to_string()
                }
            }
        };

        let model_provider = format_model_provider(config);
        let account = compose_account_display(account_display);
        let session_id = session_id.as_ref().map(std::string::ToString::to_string);
        let session_stats = session_stats.filter(|stats| !stats.is_empty()).cloned();
        let forked_from = forked_from.map(|id| id.to_string());
        let default_usage = TokenUsage::default();
        let (context_usage, budget_window, full_window) = match token_info {
            Some(info) => (
                &info.last_token_usage,
                info.model_context_window,
                info.full_model_context_window,
            ),
            None => (&default_usage, config.model_context_window, None),
        };
        let context_window = budget_window.or(full_window).map(|budget_window| {
            let full_window = full_window.filter(|full| *full != budget_window);
            StatusContextWindowData {
                percent_remaining: context_usage.percent_of_context_window_remaining(budget_window),
                tokens_in_context: context_usage.tokens_in_context_window(),
                budget_window,
                full_window,
            }
        });

        let token_usage = StatusTokenUsageData {
            total: total_usage.blended_total(),
            input: total_usage.non_cached_input(),
            output: total_usage.output_tokens,
            context_window,
        };
        let rate_limits = if rate_limits.len() <= 1 {
            compose_rate_limit_data(rate_limits.first(), now)
        } else {
            compose_rate_limit_data_many(rate_limits, now)
        };
        let rate_limit_state = Arc::new(RwLock::new(StatusRateLimitState {
            rate_limits,
            refreshing_rate_limits,
        }));

        (
            Self {
                ui_frontend: "tui".to_string(),
                model_name,
                model_details,
                directory: config.cwd.to_path_buf(),
                codex_home: config.codex_home.clone(),
                approval_policy,
                sandbox_policy,
                approval,
                sandbox,
                agents_summary,
                auto_compact_enabled: codex_core::prefs::load_blocking(&config.codex_home)
                    .auto_compact_enabled,
                hide_agent_reasoning: config.hide_agent_reasoning,
                collaboration_mode: collaboration_mode.map(ToString::to_string),
                model_provider,
                account,
                thread_name,
                session_id,
                session_stats,
                forked_from,
                token_usage,
                rate_limit_state: rate_limit_state.clone(),
                xtreme_ui_enabled: xtreme::xtreme_ui_enabled(config),
            },
            StatusHistoryHandle { rate_limit_state },
        )
    }

    fn token_usage_spans(&self) -> Vec<Span<'static>> {
        let total_fmt = format_tokens_compact(self.token_usage.total);
        let input_fmt = format_tokens_compact(self.token_usage.input);
        let output_fmt = format_tokens_compact(self.token_usage.output);

        vec![
            Span::from(total_fmt),
            Span::from(" total "),
            Span::from(" (").dim(),
            Span::from(input_fmt).dim(),
            Span::from(" input").dim(),
            Span::from(" + ").dim(),
            Span::from(output_fmt).dim(),
            Span::from(" output").dim(),
            Span::from(")").dim(),
        ]
    }

    fn context_window_spans(&self) -> Option<Vec<Span<'static>>> {
        let context = self.token_usage.context_window.as_ref()?;
        let used_fmt = format_tokens_compact(context.tokens_in_context);
        let budget_fmt = format_tokens_compact(context.budget_window);
        let display_window = context.full_window.unwrap_or(context.budget_window);
        let display_fmt = format_tokens_compact(display_window);
        let percent_remaining = (context.percent_remaining as f64).clamp(0.0, 100.0);

        let mut spans = vec![
            Span::from(render_status_limit_progress_bar(percent_remaining)),
            Span::from(" "),
            Span::from(format!("{}% left", context.percent_remaining)),
            Span::from(" (").dim(),
            Span::from(used_fmt).dim(),
            Span::from(" used / ").dim(),
            Span::from(display_fmt).dim(),
        ];

        if context.full_window.is_some() {
            spans.extend([Span::from(", budget ").dim(), Span::from(budget_fmt).dim()]);
        }
        spans.push(Span::from(")").dim());

        Some(spans)
    }

    fn context_window_spans_for_menu(&self, limit_bar_indent: usize) -> Option<Vec<Span<'static>>> {
        let mut spans = self.context_window_spans()?;
        if limit_bar_indent > 0 {
            spans.insert(0, Span::from(" ".repeat(limit_bar_indent)));
        }
        Some(spans)
    }

    fn rate_limit_lines(
        &self,
        state: &StatusRateLimitState,
        available_inner_width: usize,
        formatter: &FieldFormatter,
    ) -> Vec<Line<'static>> {
        match &state.rate_limits {
            StatusRateLimitData::Available(rows_data) => {
                if rows_data.is_empty() {
                    return vec![formatter.line(
                        "Limits",
                        vec![Span::from("not available for this account").dim()],
                    )];
                }
                self.rate_limit_row_lines(rows_data, available_inner_width, formatter)
            }
            StatusRateLimitData::Stale(rows_data) => {
                let mut lines =
                    self.rate_limit_row_lines(rows_data, available_inner_width, formatter);
                lines.push(formatter.line(
                    "Warning",
                    vec![Span::from(if state.refreshing_rate_limits {
                        "limits may be stale - run /status again shortly."
                    } else {
                        "limits may be stale - start new turn to refresh."
                    })
                    .dim()],
                ));
                lines
            }
            StatusRateLimitData::Unavailable => vec![formatter.line(
                "Limits",
                vec![Span::from("not available for this account").dim()],
            )],
            StatusRateLimitData::Missing => vec![formatter.line(
                "Limits",
                vec![Span::from(if state.refreshing_rate_limits {
                    "refresh requested; run /status again shortly."
                } else {
                    "data not available yet"
                })
                .dim()],
            )],
        }
    }

    fn rate_limit_row_lines(
        &self,
        rows: &[StatusRateLimitRow],
        available_inner_width: usize,
        formatter: &FieldFormatter,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(rows.len().saturating_mul(2));

        for row in rows {
            match &row.value {
                StatusRateLimitValue::Window {
                    percent_used,
                    resets_at,
                } => {
                    let percent_remaining = (100.0 - percent_used).clamp(0.0, 100.0);
                    let value_spans = vec![
                        Span::from(render_status_limit_progress_bar(percent_remaining)),
                        Span::from(" "),
                        Span::from(format_status_limit_summary(percent_remaining)),
                    ];
                    let base_spans = formatter.full_spans(row.label.as_str(), value_spans);
                    let base_line = Line::from(base_spans.clone());

                    if let Some(resets_at) = resets_at.as_ref() {
                        let resets_span = Span::from(format!("(resets {resets_at})")).dim();
                        let mut inline_spans = base_spans.clone();
                        inline_spans.push(" ".dim());
                        inline_spans.push(resets_span.clone());

                        if line_display_width(&Line::from(inline_spans.clone()))
                            <= available_inner_width
                        {
                            lines.push(Line::from(inline_spans));
                        } else {
                            lines.push(base_line);
                            lines.push(formatter.continuation(vec![resets_span]));
                        }
                    } else {
                        lines.push(base_line);
                    }
                }
                StatusRateLimitValue::Text(text) => {
                    let spans =
                        formatter.full_spans(row.label.as_str(), vec![Span::from(text.clone())]);
                    lines.push(Line::from(spans));
                }
            }
        }

        lines
    }

    fn collect_rate_limit_labels(
        &self,
        state: &StatusRateLimitState,
        seen: &mut BTreeSet<String>,
        labels: &mut Vec<String>,
    ) {
        match &state.rate_limits {
            StatusRateLimitData::Available(rows) => {
                if rows.is_empty() {
                    push_label(labels, seen, "Limits");
                } else {
                    for row in rows {
                        push_label(labels, seen, row.label.as_str());
                    }
                }
            }
            StatusRateLimitData::Stale(rows) => {
                for row in rows {
                    push_label(labels, seen, row.label.as_str());
                }
                push_label(labels, seen, "Warning");
            }
            StatusRateLimitData::Unavailable | StatusRateLimitData::Missing => {
                push_label(labels, seen, "Limits");
            }
        }
    }
}

fn status_menu_limit_label_width(state: &StatusRateLimitState) -> usize {
    match &state.rate_limits {
        StatusRateLimitData::Available(rows) | StatusRateLimitData::Stale(rows) => rows
            .iter()
            .map(|row| UnicodeWidthStr::width(row.label.as_str()))
            .max()
            .unwrap_or(0),
        StatusRateLimitData::Unavailable | StatusRateLimitData::Missing => 0,
    }
}

fn status_menu_limit_bar_indent(state: &StatusRateLimitState) -> usize {
    status_menu_limit_label_width(state).saturating_add(1)
}

impl HistoryCell for StatusHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut title_spans = vec![FieldFormatter::INDENT.dim()];
        title_spans.extend(xtreme::title_prefix_spans(self.xtreme_ui_enabled));
        title_spans.extend([
            "xtreme-Codex".bold(),
            " ".dim(),
            format!("(v{CODEX_CLI_VERSION})").dim(),
        ]);
        lines.push(Line::from(title_spans));
        lines.push(Line::from(Vec::<Span<'static>>::new()));

        let available_inner_width = usize::from(width.saturating_sub(4));
        if available_inner_width == 0 {
            return Vec::new();
        }

        let account_value = self.account.as_ref().map(|account| match account {
            StatusAccountDisplay::ChatGpt { email, plan } => match (email, plan) {
                (Some(email), Some(plan)) => format!("{email} ({plan})"),
                (Some(email), None) => email.clone(),
                (None, Some(plan)) => plan.clone(),
                (None, None) => "ChatGPT".to_string(),
            },
            StatusAccountDisplay::ApiKey => {
                "API key configured (run xcodex login to use ChatGPT)".to_string()
            }
        });

        let power_spans = xtreme::power_meter_value_spans(
            self.xtreme_ui_enabled,
            self.approval_policy,
            &self.sandbox_policy,
        );

        let mut labels: Vec<String> = vec![
            "UI",
            "Model",
            "Directory",
            "CODEX_HOME",
            "Approval",
            "Sandbox",
            "Agents.md",
            "Auto-compact",
            "Thoughts",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let mut seen: BTreeSet<String> = labels.iter().cloned().collect();
        let thread_name = self.thread_name.as_deref().filter(|name| !name.is_empty());
        #[expect(clippy::expect_used)]
        let rate_limit_state = self
            .rate_limit_state
            .read()
            .expect("status history rate-limit state poisoned");

        if self.model_provider.is_some() {
            push_label(&mut labels, &mut seen, "Model provider");
        }
        if account_value.is_some() {
            push_label(&mut labels, &mut seen, "Account");
        }
        if power_spans.is_some() {
            push_label(&mut labels, &mut seen, "Power");
        }
        if thread_name.is_some() {
            push_label(&mut labels, &mut seen, "Thread name");
        }
        if self.session_id.is_some() {
            push_label(&mut labels, &mut seen, "Session");
        }
        if self.session_id.is_some() && self.forked_from.is_some() {
            push_label(&mut labels, &mut seen, "Forked from");
        }
        if self.collaboration_mode.is_some() {
            push_label(&mut labels, &mut seen, "Collaboration mode");
        }
        if self.session_stats.is_some() {
            push_label(&mut labels, &mut seen, "Session stats");
        }
        push_label(&mut labels, &mut seen, "Token usage");
        if self.token_usage.context_window.is_some() {
            push_label(&mut labels, &mut seen, "Context window");
        }

        self.collect_rate_limit_labels(&rate_limit_state, &mut seen, &mut labels);

        let formatter = FieldFormatter::from_labels(labels.iter().map(String::as_str));
        let value_width = formatter.value_width(available_inner_width);

        let note_first_line = Line::from(vec![
            Span::styled("Visit ", crate::theme::accent_style()),
            Span::styled(
                "https://chatgpt.com/codex/settings/usage",
                crate::theme::link_style().add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled(" for up-to-date", crate::theme::accent_style()),
        ]);
        let note_second_line = Line::from(vec![Span::styled(
            "information on rate limits and credits",
            crate::theme::accent_style(),
        )]);
        lines.extend(word_wrap_lines(
            [note_first_line, note_second_line],
            RtOptions::new(available_inner_width),
        ));
        lines.push(Line::from(Vec::<Span<'static>>::new()));

        let mut model_spans = vec![Span::from(self.model_name.clone())];
        if !self.model_details.is_empty() {
            model_spans.push(" (".dim());
            model_spans.push(Span::from(self.model_details.join(", ")).dim());
            model_spans.push(")".dim());
        }

        let directory_value = format_directory_display(&self.directory, Some(value_width));
        let codex_home_value = format_directory_display(&self.codex_home, Some(value_width));

        lines.push(formatter.line("UI", vec![Span::from(self.ui_frontend.clone())]));
        if let Some(spans) = power_spans {
            lines.push(formatter.line("Power", spans));
        }
        lines.push(formatter.line("Model", model_spans));
        if let Some(model_provider) = self.model_provider.as_ref() {
            lines.push(formatter.line("Model provider", vec![Span::from(model_provider.clone())]));
        }
        lines.push(formatter.line("Directory", vec![Span::from(directory_value)]));
        lines.push(formatter.line("CODEX_HOME", vec![Span::from(codex_home_value)]));
        lines.push(formatter.line("Approval", vec![Span::from(self.approval.clone())]));
        lines.push(formatter.line("Sandbox", vec![Span::from(self.sandbox.clone())]));
        lines.push(formatter.line("Agents.md", vec![Span::from(self.agents_summary.clone())]));
        lines.push(formatter.line(
            "Auto-compact",
            vec![if self.auto_compact_enabled {
                Span::styled(
                    "enabled",
                    crate::theme::success_style().add_modifier(Modifier::BOLD),
                )
            } else {
                "disabled".dim()
            }],
        ));
        lines.push(formatter.line(
            "Thoughts",
            vec![Span::from(if self.hide_agent_reasoning {
                "hidden"
            } else {
                "shown"
            })],
        ));

        if let Some(account_value) = account_value {
            lines.push(formatter.line("Account", vec![Span::from(account_value)]));
        }
        if let Some(thread_name) = thread_name {
            lines.push(formatter.line("Thread name", vec![Span::from(thread_name.to_string())]));
        }
        if let Some(collab_mode) = self.collaboration_mode.as_ref() {
            lines.push(formatter.line("Collaboration mode", vec![Span::from(collab_mode.clone())]));
        }
        if let Some(session) = self.session_id.as_ref() {
            lines.push(formatter.line("Session", vec![Span::from(session.clone())]));
        }
        if self.session_id.is_some() {
            if let Some(forked_from) = self.forked_from.as_ref() {
                lines.push(formatter.line("Forked from", vec![Span::from(forked_from.clone())]));
            }
        }
        if let Some(stats) = self.session_stats.as_ref() {
            lines.push(formatter.line("Session stats", stats.spans()));
        }

        lines.push(Line::from(Vec::<Span<'static>>::new()));
        if !matches!(self.account, Some(StatusAccountDisplay::ChatGpt { .. })) {
            lines.push(formatter.line("Token usage", self.token_usage_spans()));
        }
        if let Some(spans) = self.context_window_spans() {
            lines.push(formatter.line("Context window", spans));
        }
        lines.extend(self.rate_limit_lines(&rate_limit_state, available_inner_width, &formatter));

        let content_width = lines.iter().map(line_display_width).max().unwrap_or(0);
        let inner_width = content_width.min(available_inner_width);
        let truncated_lines: Vec<Line<'static>> = lines
            .into_iter()
            .map(|line| truncate_line_to_width(line, inner_width))
            .collect();

        with_border_with_inner_width(truncated_lines, inner_width)
    }
}

fn compat_compose_agents_summary(config: &Config) -> String {
    if config.project_doc_max_bytes == 0 {
        return "<none>".to_string();
    }

    let candidate_filenames = compat_project_doc_candidate_filenames(config);
    let mut paths = Vec::new();

    for candidate in &candidate_filenames {
        let path = config.codex_home.join(candidate);
        if path.is_file() {
            paths.push(path);
            break;
        }
    }

    let search_dirs = compat_project_doc_search_dirs(&config.cwd);
    for dir in search_dirs {
        for candidate in &candidate_filenames {
            let path = dir.join(candidate);
            if path.is_file() {
                if !paths.iter().any(|existing| existing == &path) {
                    paths.push(path);
                }
                break;
            }
        }
    }

    if paths.is_empty() {
        "<none>".to_string()
    } else {
        compose_agents_summary(config, &paths)
    }
}

fn compat_project_doc_candidate_filenames(config: &Config) -> Vec<String> {
    let mut names = vec![
        LOCAL_PROJECT_DOC_FILENAME.to_string(),
        DEFAULT_PROJECT_DOC_FILENAME.to_string(),
    ];
    for candidate in &config.project_doc_fallback_filenames {
        if candidate.is_empty() || names.iter().any(|name| name == candidate) {
            continue;
        }
        names.push(candidate.clone());
    }
    names
}

fn compat_project_doc_search_dirs(cwd: &Path) -> Vec<PathBuf> {
    let Some(project_root) = compat_project_root(cwd) else {
        return vec![cwd.to_path_buf()];
    };

    let mut dirs = Vec::new();
    let mut cursor = cwd.to_path_buf();
    loop {
        dirs.push(cursor.clone());
        if cursor == project_root {
            break;
        }
        let Some(parent) = cursor.parent() else {
            break;
        };
        cursor = parent.to_path_buf();
    }
    dirs.reverse();
    dirs
}

fn compat_project_root(cwd: &Path) -> Option<PathBuf> {
    for ancestor in cwd.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn format_model_provider(config: &Config) -> Option<String> {
    let provider = &config.model_provider;
    let name = provider.name.trim();
    let provider_name = if name.is_empty() {
        config.model_provider_id.as_str()
    } else {
        name
    };
    let base_url = provider.base_url.as_deref().and_then(sanitize_base_url);
    let is_default_openai = provider.is_openai() && base_url.is_none();
    if is_default_openai {
        return None;
    }

    Some(match base_url {
        Some(base_url) => format!("{provider_name} - {base_url}"),
        None => provider_name.to_string(),
    })
}

fn sanitize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let Ok(mut url) = Url::parse(trimmed) else {
        return None;
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_string()).filter(|value| !value.is_empty())
}
