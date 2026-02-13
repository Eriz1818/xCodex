use crate::chatwidget::transcript_spacer_line;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::SESSION_HEADER_MAX_INNER_WIDTH;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::card_inner_width;
use crate::history_cell::padded_emoji;
use crate::history_cell::with_border;
use crate::history_cell::with_border_with_inner_width;
use crate::key_hint;
use crate::live_wrap::take_prefix_by_width;
use crate::markdown::append_markdown;
use crate::render::line_utils::prefix_lines;
use crate::style::user_message_style;
use crate::tooltips;
use crate::update_action::UpdateAction;
use crate::version::CODEX_CLI_VERSION;
use crate::xtreme;
use codex_common::format_env_display::format_env_display;
use codex_core::config;
use codex_core::config::Config;
use codex_core::config::types::McpServerTransportConfig;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::McpAuthStatus;
use codex_core::protocol::McpServerSnapshotState;
use codex_core::protocol::McpStartupStatus;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol::SessionConfiguredEvent;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::mcp::Resource;
use codex_protocol::mcp::ResourceTemplate;
use codex_protocol::mcp::Tool;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub(crate) struct BackgroundActivityEntry {
    pub(crate) id: String,
    pub(crate) command_display: String,
}

impl BackgroundActivityEntry {
    pub(crate) fn new(id: String, command_display: String) -> Self {
        Self {
            id,
            command_display,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct McpStartupRenderInfo<'a> {
    pub(crate) statuses: Option<&'a HashMap<String, McpStartupStatus>>,
    pub(crate) durations: Option<&'a HashMap<String, Duration>>,
    pub(crate) ready_duration: Option<Duration>,
    pub(crate) server_states: Option<&'a HashMap<String, McpServerSnapshotState>>,
}

pub(crate) fn user_prompt_style(highlight: bool) -> Style {
    let mut style = user_message_style().patch(crate::theme::composer_style());
    if highlight {
        style = style.patch(crate::theme::user_prompt_highlight_style());
    }
    style
}

#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug)]
struct UpdateAvailableHistoryCell {
    latest_version: String,
    update_action: Option<UpdateAction>,
}

#[cfg_attr(debug_assertions, allow(dead_code))]
impl UpdateAvailableHistoryCell {
    fn new(latest_version: String, update_action: Option<UpdateAction>) -> Self {
        Self {
            latest_version,
            update_action,
        }
    }
}

impl HistoryCell for UpdateAvailableHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        use ratatui_macros::line;
        use ratatui_macros::text;
        let update_instruction = if let Some(update_action) = self.update_action {
            line!["Run ", update_action.command_str().cyan(), " to update."]
        } else {
            line![
                "See ",
                "https://github.com/Eriz1818/xcodex".cyan().underlined(),
                " for installation options."
            ]
        };

        let content = text![
            line![
                padded_emoji("✨").bold().cyan(),
                "Update available!".bold().cyan(),
                " ",
                format!("{CODEX_CLI_VERSION} -> {}", self.latest_version).bold(),
            ],
            update_instruction,
            "",
            "See full release notes:",
            "https://github.com/Eriz1818/xcodex/releases/latest"
                .cyan()
                .underlined(),
        ];

        let inner_width = content
            .width()
            .min(usize::from(width.saturating_sub(4)))
            .max(1);
        with_border_with_inner_width(content.lines, inner_width)
    }
}

#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug)]
struct WhatsNewHistoryCell {
    version: String,
    bullets: Vec<String>,
}

#[cfg_attr(debug_assertions, allow(dead_code))]
impl WhatsNewHistoryCell {
    fn new(version: String, bullets: Vec<String>) -> Self {
        Self { version, bullets }
    }
}

impl HistoryCell for WhatsNewHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        use ratatui_macros::line;

        let mut lines = Vec::new();
        lines.push(line![
            padded_emoji("⚡").bold().cyan(),
            format!("What's new in ⚡xtreme-Codex v{}", self.version).bold(),
        ]);
        lines.push(line![""]);

        for bullet in &self.bullets {
            lines.push(line!["• ".dim(), bullet.clone()]);
        }

        lines.push(line![""]);
        lines.push(line![
            "Read more: ".dim(),
            "https://github.com/Eriz1818/xcodex/releases/latest"
                .cyan()
                .underlined(),
        ]);

        let content: Text<'static> = lines.into();
        let inner_width = content
            .width()
            .min(usize::from(width.saturating_sub(4)))
            .max(1);
        with_border_with_inner_width(content.lines, inner_width)
    }
}

#[derive(Debug)]
struct XcodexTooltipsHistoryCell {
    xcodex_tip: Option<String>,
    codex_tip: Option<String>,
}

impl XcodexTooltipsHistoryCell {
    fn new(xcodex_tip: Option<String>, codex_tip: Option<String>) -> Option<Self> {
        if xcodex_tip.is_some() || codex_tip.is_some() {
            Some(Self {
                xcodex_tip,
                codex_tip,
            })
        } else {
            None
        }
    }
}

impl HistoryCell for XcodexTooltipsHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let indent = "  ";
        let indent_width = UnicodeWidthStr::width(indent);
        let wrap_width = usize::from(width.max(1))
            .saturating_sub(indent_width)
            .max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();

        if let Some(tip) = self.xcodex_tip.as_deref() {
            append_markdown(&format!("**⚡Tips:** {tip}"), Some(wrap_width), &mut lines);
        }
        if let Some(tip) = self.codex_tip.as_deref() {
            append_markdown(&format!("**Tips:** {tip}"), Some(wrap_width), &mut lines);
        }

        prefix_lines(lines, indent.into(), indent.into())
    }
}

#[derive(Debug)]
struct FallbackTooltipHistoryCell {
    tip: String,
}

impl FallbackTooltipHistoryCell {
    fn new(tip: String) -> Self {
        Self { tip }
    }
}

impl HistoryCell for FallbackTooltipHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let indent = "  ";
        let indent_width = UnicodeWidthStr::width(indent);
        let wrap_width = usize::from(width.max(1))
            .saturating_sub(indent_width)
            .max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_markdown(
            &format!("**Tips:** {}", self.tip),
            Some(wrap_width),
            &mut lines,
        );

        prefix_lines(lines, indent.into(), indent.into())
    }
}

#[cfg_attr(debug_assertions, allow(dead_code))]
pub(crate) fn new_update_available_history_cell(
    latest_version: String,
    update_action: Option<UpdateAction>,
) -> Box<dyn HistoryCell> {
    Box::new(UpdateAvailableHistoryCell::new(
        latest_version,
        update_action,
    ))
}

#[cfg_attr(debug_assertions, allow(dead_code))]
pub(crate) fn new_whats_new_history_cell(
    version: String,
    bullets: Vec<String>,
) -> Box<dyn HistoryCell> {
    Box::new(WhatsNewHistoryCell::new(version, bullets))
}

pub(crate) fn maybe_xcodex_tooltips_cell(show_tooltips: bool) -> Option<Box<dyn HistoryCell>> {
    if !show_tooltips || !config::is_xcodex_invocation() {
        return None;
    }

    XcodexTooltipsHistoryCell::new(
        tooltips::random_xcodex_tooltip(),
        tooltips::random_tooltip(),
    )
    .map(|cell| Box::new(cell) as Box<dyn HistoryCell>)
}

pub(crate) fn hooks_section_lines(
    hooks: &[BackgroundActivityEntry],
    wrap_width: usize,
    max_entries: usize,
    prefix: &'static str,
    truncation_suffix: &'static str,
    command_style: Style,
    transcript_style: Style,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from("").style(crate::theme::transcript_style()));
    out.push(vec![format!("Hooks ({})", hooks.len()).bold()].into());
    out.push(Line::from("").style(crate::theme::transcript_style()));

    if hooks.is_empty() {
        out.push("  • No hooks running.".italic().into());
        return out;
    }

    let prefix_width = UnicodeWidthStr::width(prefix);
    let truncation_suffix_width = UnicodeWidthStr::width(truncation_suffix);
    let mut shown = 0usize;
    for entry in hooks {
        if shown >= max_entries {
            break;
        }
        let id_display = entry.id.as_str();
        let id_width = UnicodeWidthStr::width(id_display);
        if wrap_width <= prefix_width {
            out.push(Line::from(prefix.dim()));
            shown += 1;
            continue;
        }

        let (snippet, snippet_truncated) = {
            let command_display = entry.command_display.as_str();
            let (first_line, has_more_lines) = match command_display.split_once('\n') {
                Some((first, _)) => (first, true),
                None => (command_display, false),
            };
            let max_graphemes = 80;
            let mut graphemes = first_line.grapheme_indices(true);
            if let Some((byte_index, _)) = graphemes.nth(max_graphemes) {
                (first_line[..byte_index].to_string(), true)
            } else {
                (first_line.to_string(), has_more_lines)
            }
        };
        let budget = wrap_width.saturating_sub(prefix_width);
        if budget <= id_width.saturating_add(1) {
            let (truncated, _, _) = take_prefix_by_width(id_display, budget);
            out.push(vec![prefix.dim(), Span::from(truncated).set_style(command_style)].into());
            shown += 1;
            continue;
        }

        let mut needs_suffix = snippet_truncated;
        let snippet_budget = budget.saturating_sub(id_width.saturating_add(1));
        if !needs_suffix {
            let (_, remainder, _) = take_prefix_by_width(&snippet, snippet_budget);
            if !remainder.is_empty() {
                needs_suffix = true;
            }
        }
        if needs_suffix && snippet_budget > truncation_suffix_width {
            let available = snippet_budget.saturating_sub(truncation_suffix_width);
            let (truncated, _, _) = take_prefix_by_width(&snippet, available);
            out.push(
                vec![
                    prefix.dim(),
                    Span::from(id_display.to_string()).set_style(command_style),
                    " ".dim(),
                    Span::from(truncated).set_style(transcript_style),
                    truncation_suffix.dim(),
                ]
                .into(),
            );
        } else {
            let (truncated, _, _) = take_prefix_by_width(&snippet, snippet_budget);
            out.push(
                vec![
                    prefix.dim(),
                    Span::from(id_display.to_string()).set_style(command_style),
                    " ".dim(),
                    Span::from(truncated).set_style(transcript_style),
                ]
                .into(),
            );
        }
        shown += 1;
    }

    let remaining = hooks.len().saturating_sub(shown);
    if remaining > 0 {
        let more_text = format!("... and {remaining} more running");
        if wrap_width <= prefix_width {
            out.push(Line::from(prefix.dim()));
        } else {
            let budget = wrap_width.saturating_sub(prefix_width);
            let (truncated, _, _) = take_prefix_by_width(&more_text, budget);
            out.push(vec![prefix.dim(), truncated.dim()].into());
        }
    }

    out
}

#[derive(Debug)]
struct XcodexUnifiedExecProcessesCell {
    processes: Vec<BackgroundActivityEntry>,
    hooks: Vec<BackgroundActivityEntry>,
}

impl XcodexUnifiedExecProcessesCell {
    fn new(processes: Vec<BackgroundActivityEntry>, hooks: Vec<BackgroundActivityEntry>) -> Self {
        Self { processes, hooks }
    }
}

impl HistoryCell for XcodexUnifiedExecProcessesCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }

        let wrap_width = width as usize;
        let max_entries = 16usize;
        let mut out: Vec<Line<'static>> = Vec::new();
        let command = crate::theme::command_style();
        let transcript = crate::theme::transcript_style();
        out.push(vec![format!("Background terminals ({})", self.processes.len()).bold()].into());
        out.push(transcript_spacer_line());

        if self.processes.is_empty() {
            out.push("  • No background terminals running.".italic().into());
        }

        let prefix = "  • ";
        let prefix_width = UnicodeWidthStr::width(prefix);
        let truncation_suffix = " [...]";
        let truncation_suffix_width = UnicodeWidthStr::width(truncation_suffix);
        let mut shown = 0usize;
        for entry in &self.processes {
            if shown >= max_entries {
                break;
            }
            let id_display = entry.id.as_str();
            let id_width = UnicodeWidthStr::width(id_display);
            if wrap_width <= prefix_width {
                out.push(Line::from(prefix.dim()));
                shown += 1;
                continue;
            }

            let (snippet, snippet_truncated) = {
                let command_display = entry.command_display.as_str();
                let (first_line, has_more_lines) = match command_display.split_once('\n') {
                    Some((first, _)) => (first, true),
                    None => (command_display, false),
                };
                let max_graphemes = 80;
                let mut graphemes = first_line.grapheme_indices(true);
                if let Some((byte_index, _)) = graphemes.nth(max_graphemes) {
                    (first_line[..byte_index].to_string(), true)
                } else {
                    (first_line.to_string(), has_more_lines)
                }
            };
            let budget = wrap_width.saturating_sub(prefix_width);
            if budget <= id_width.saturating_add(1) {
                let (truncated, _, _) = take_prefix_by_width(id_display, budget);
                out.push(vec![prefix.dim(), Span::from(truncated).set_style(command)].into());
                shown += 1;
                continue;
            }

            let mut needs_suffix = snippet_truncated;
            let snippet_budget = budget.saturating_sub(id_width.saturating_add(1));
            if !needs_suffix {
                let (_, remainder, _) = take_prefix_by_width(&snippet, snippet_budget);
                if !remainder.is_empty() {
                    needs_suffix = true;
                }
            }
            if needs_suffix && snippet_budget > truncation_suffix_width {
                let available = snippet_budget.saturating_sub(truncation_suffix_width);
                let (truncated, _, _) = take_prefix_by_width(&snippet, available);
                out.push(
                    vec![
                        prefix.dim(),
                        Span::from(id_display.to_string()).set_style(command),
                        " ".dim(),
                        Span::from(truncated).set_style(transcript),
                        truncation_suffix.dim(),
                    ]
                    .into(),
                );
            } else {
                let (truncated, _, _) = take_prefix_by_width(&snippet, snippet_budget);
                out.push(
                    vec![
                        prefix.dim(),
                        Span::from(id_display.to_string()).set_style(command),
                        " ".dim(),
                        Span::from(truncated).set_style(transcript),
                    ]
                    .into(),
                );
            }
            shown += 1;
        }

        let remaining = self.processes.len().saturating_sub(shown);
        if remaining > 0 {
            let more_text = format!("... and {remaining} more running");
            if wrap_width <= prefix_width {
                out.push(Line::from(prefix.dim()));
            } else {
                let budget = wrap_width.saturating_sub(prefix_width);
                let (truncated, _, _) = take_prefix_by_width(&more_text, budget);
                out.push(vec![prefix.dim(), truncated.dim()].into());
            }
        }

        out.extend(hooks_section_lines(
            &self.hooks,
            wrap_width,
            max_entries,
            prefix,
            truncation_suffix,
            command,
            transcript,
        ));
        out
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.display_lines(width).len() as u16
    }
}

pub(crate) fn new_unified_exec_processes_output(
    processes: Vec<BackgroundActivityEntry>,
    hooks: Vec<BackgroundActivityEntry>,
) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec![
        Span::from("/ps")
            .set_style(crate::theme::command_style())
            .into(),
    ]);
    let summary = XcodexUnifiedExecProcessesCell::new(processes, hooks);
    CompositeHistoryCell::new(vec![Box::new(command), Box::new(summary)])
}

/// Render MCP tools grouped by connection using the fully-qualified tool names.
pub(crate) fn new_mcp_tools_output(
    config: &Config,
    tools: HashMap<String, Tool>,
    resources: HashMap<String, Vec<Resource>>,
    resource_templates: HashMap<String, Vec<ResourceTemplate>>,
    auth_statuses: &HashMap<String, McpAuthStatus>,
    startup: McpStartupRenderInfo<'_>,
) -> PlainHistoryCell {
    fn format_duration(duration: Duration) -> String {
        let ms = duration.as_millis();
        if ms < 1_000 {
            format!("{ms}ms")
        } else {
            let secs = duration.as_secs_f64();
            format!("{secs:.1}s")
        }
    }

    let mut lines: Vec<Line<'static>> = vec![
        "/mcp".magenta().into(),
        transcript_spacer_line(),
        vec!["🔌  ".into(), "MCP Tools".bold()].into(),
        transcript_spacer_line(),
    ];

    if let Some(duration) = startup.ready_duration
        && startup
            .statuses
            .is_some_and(|statuses| !statuses.is_empty())
    {
        let duration_display = format_duration(duration);
        lines.push(
            vec![
                "  • MCP ready: ".into(),
                format!("({duration_display})").dim(),
            ]
            .into(),
        );
        lines.push(transcript_spacer_line());
    }

    if tools.is_empty() {
        let still_starting = match startup.statuses {
            Some(statuses) => statuses
                .values()
                .any(|status| matches!(status, McpStartupStatus::Starting)),
            None => false,
        };
        if still_starting {
            lines.push(
                "  • No MCP tools available yet (servers still starting)."
                    .italic()
                    .into(),
            );
        } else {
            lines.push("  • No MCP tools available.".italic().into());
        }
        lines.push(transcript_spacer_line());
    }

    let mut servers: Vec<_> = config.mcp_servers.iter().collect();
    servers.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut retryable_servers: Vec<String> = Vec::new();

    for (server, cfg) in servers {
        let prefix = format!("mcp__{server}__");
        let mut names: Vec<String> = tools
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| k[prefix.len()..].to_string())
            .collect();
        names.sort();

        let auth_status = auth_statuses
            .get(server.as_str())
            .copied()
            .unwrap_or(McpAuthStatus::Unsupported);
        let mut header: Vec<Span<'static>> = vec!["  • ".into(), server.clone().into()];
        if !cfg.enabled {
            header.push(" ".into());
            header.push("(disabled)".red());
            lines.push(header.into());
            if let Some(reason) = cfg.disabled_reason.as_ref().map(ToString::to_string) {
                lines.push(vec!["    • Reason: ".into(), reason.dim()].into());
            }
            lines.push(transcript_spacer_line());
            continue;
        }
        lines.push(header.into());
        lines.push(vec!["    • Status: ".into(), "enabled".green()].into());

        let startup_status = startup
            .statuses
            .and_then(|statuses| statuses.get(server.as_str()))
            .cloned();
        let server_state = startup
            .server_states
            .and_then(|states| states.get(server.as_str()))
            .copied();
        let mut startup_spans: Vec<Span<'static>> = vec!["    • Startup: ".into()];
        let startup_duration = startup
            .durations
            .and_then(|durations| durations.get(server.as_str()))
            .copied();
        let mut startup_error: Option<String> = None;
        let mut retryable = false;
        match startup_status {
            Some(McpStartupStatus::Starting) => {
                startup_spans.push("Starting".cyan());
            }
            Some(McpStartupStatus::Ready) => {
                startup_spans.push("Ready".green());
            }
            Some(McpStartupStatus::Failed { error }) => {
                startup_spans.push("Failed".red());
                startup_error = Some(error);
                retryable = true;
            }
            Some(McpStartupStatus::Cancelled) => {
                startup_spans.push("Cancelled".dim());
                retryable = true;
            }
            None => match server_state {
                Some(McpServerSnapshotState::Ready) => {
                    startup_spans.push("Ready".green());
                }
                Some(McpServerSnapshotState::Cached) => {
                    startup_spans.push("Cached".yellow());
                }
                None => {
                    startup_spans.push("Unknown".dim());
                }
            },
        }
        if let Some(duration) = startup_duration {
            let duration_display = format_duration(duration);
            startup_spans.push(" ".into());
            startup_spans.push(format!("({duration_display})").dim());
        }
        lines.push(startup_spans.into());
        lines.push(vec!["    • Auth: ".into(), auth_status.to_string().into()].into());
        if let Some(error) = startup_error.as_ref() {
            lines.push(vec!["    • Error: ".into(), error.clone().red()].into());
        }
        if retryable {
            retryable_servers.push(server.clone());
            lines.push(
                vec![
                    "    • Retry: ".into(),
                    format!("/mcp retry {server}").magenta(),
                ]
                .into(),
            );
        }

        match &cfg.transport {
            McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            } => {
                let args_suffix = if args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args.join(" "))
                };
                let cmd_display = format!("{command}{args_suffix}");
                lines.push(vec!["    • Command: ".into(), cmd_display.into()].into());

                if let Some(cwd) = cwd.as_ref() {
                    lines.push(vec!["    • Cwd: ".into(), cwd.display().to_string().into()].into());
                }

                let env_display = format_env_display(env.as_ref(), env_vars);
                if env_display != "-" {
                    lines.push(vec!["    • Env: ".into(), env_display.into()].into());
                }
            }
            McpServerTransportConfig::StreamableHttp {
                url,
                http_headers,
                env_http_headers,
                ..
            } => {
                lines.push(vec!["    • URL: ".into(), url.clone().into()].into());
                if let Some(headers) = http_headers.as_ref()
                    && !headers.is_empty()
                {
                    let mut pairs: Vec<_> = headers.iter().collect();
                    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                    let display = pairs
                        .into_iter()
                        .map(|(name, _)| format!("{name}=*****"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push(vec!["    • HTTP headers: ".into(), display.into()].into());
                }
                if let Some(headers) = env_http_headers.as_ref()
                    && !headers.is_empty()
                {
                    let mut pairs: Vec<_> = headers.iter().collect();
                    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                    let display = pairs
                        .into_iter()
                        .map(|(name, var)| format!("{name}={var}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push(vec!["    • Env HTTP headers: ".into(), display.into()].into());
                }
            }
        }

        if names.is_empty() {
            lines.push("    • Tools: (none)".into());
        } else {
            lines.push(vec!["    • Tools: ".into(), names.join(", ").into()].into());
        }

        let server_resources: Vec<Resource> =
            resources.get(server.as_str()).cloned().unwrap_or_default();
        if server_resources.is_empty() {
            lines.push("    • Resources: (none)".into());
        } else {
            let mut spans: Vec<Span<'static>> = vec!["    • Resources: ".into()];

            for (idx, resource) in server_resources.iter().enumerate() {
                if idx > 0 {
                    spans.push(", ".into());
                }

                let label = resource.title.as_ref().unwrap_or(&resource.name);
                spans.push(label.clone().into());
                spans.push(" ".into());
                spans.push(format!("({})", resource.uri).dim());
            }

            lines.push(spans.into());
        }

        let server_templates: Vec<ResourceTemplate> = resource_templates
            .get(server.as_str())
            .cloned()
            .unwrap_or_default();
        if server_templates.is_empty() {
            lines.push("    • Resource templates: (none)".into());
        } else {
            let mut spans: Vec<Span<'static>> = vec!["    • Resource templates: ".into()];

            for (idx, template) in server_templates.iter().enumerate() {
                if idx > 0 {
                    spans.push(", ".into());
                }

                let label = template.title.as_ref().unwrap_or(&template.name);
                spans.push(label.clone().into());
                spans.push(" ".into());
                spans.push(format!("({})", template.uri_template).dim());
            }

            lines.push(spans.into());
        }

        lines.push(transcript_spacer_line());
    }

    if !retryable_servers.is_empty() {
        retryable_servers.sort();
        retryable_servers.dedup();
        lines.push(
            vec![
                "  • Retry failed: ".into(),
                "/mcp retry failed".magenta(),
                " (or a specific server above)".dim(),
            ]
            .into(),
        );
        lines.push(transcript_spacer_line());
    }

    PlainHistoryCell::new(lines)
}

pub(crate) fn session_header_title_spans(
    version: &str,
    xtreme_ui_enabled: bool,
) -> Vec<Span<'static>> {
    let mut title_spans: Vec<Span<'static>> = xtreme::title_prefix_spans(xtreme_ui_enabled);
    title_spans.push(Span::from("xtreme-Codex").bold());
    if codex_core::build_info::pyo3_hooks_enabled() {
        title_spans.push(Span::from(" ").dim());
        title_spans.push(Span::from("(with PyO3)").magenta());
    }
    title_spans.push(Span::from(" ").dim());
    title_spans.push(Span::from(format!("(v{version})")).dim());
    title_spans
}

pub(crate) fn session_header_power_spans(
    xtreme_ui_enabled: bool,
    approval: AskForApproval,
    sandbox: &SandboxPolicy,
    label_width: usize,
) -> Option<Vec<Span<'static>>> {
    xtreme::power_meter_spans(xtreme_ui_enabled, approval, sandbox, label_width)
}

pub(crate) fn session_first_event_command_lines(transcript_style: Style) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "  ".into(),
            Span::from("/init").set_style(transcript_style),
            " - create an AGENTS.md file with instructions for xcodex".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/status").set_style(transcript_style),
            " - show current session configuration".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/approvals").set_style(transcript_style),
            " - choose what xcodex can do without approval".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/permissions").set_style(transcript_style),
            " - choose what Codex is allowed to do".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/model").set_style(crate::theme::accent_style()),
            " - choose what model and reasoning effort to use".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/review").set_style(transcript_style),
            " - review any changes and find issues".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/hooks").set_style(transcript_style),
            " - automate xcodex with hooks".dim(),
        ]),
        Line::from(vec![
            "  ".into(),
            Span::from("/resume").set_style(transcript_style),
            " - resume a saved chat".dim(),
        ]),
    ]
}

fn session_first_event_help_lines() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![
        "  To get started, describe a task or try one of these commands:"
            .dim()
            .into(),
        Line::from("").style(crate::theme::transcript_style()),
    ];
    let transcript_style = crate::theme::transcript_style();
    lines.extend(session_first_event_command_lines(transcript_style));
    lines
}

#[derive(Debug)]
pub(crate) struct XcodexSessionHeaderHistoryCell {
    version: &'static str,
    model: String,
    model_style: Style,
    reasoning_effort: Option<ReasoningEffortConfig>,
    directory: std::path::PathBuf,
    approval: AskForApproval,
    sandbox: SandboxPolicy,
    xtreme_ui_enabled: bool,
    is_collaboration: bool,
    collaboration_mode: CollaborationMode,
}

impl XcodexSessionHeaderHistoryCell {
    pub(crate) fn new(
        model: String,
        reasoning_effort: Option<ReasoningEffortConfig>,
        directory: std::path::PathBuf,
        version: &'static str,
        approval: AskForApproval,
        sandbox: SandboxPolicy,
        xtreme_ui_enabled: bool,
        is_collaboration: bool,
        collaboration_mode: CollaborationMode,
    ) -> Self {
        Self::new_with_style(
            model,
            Style::default(),
            reasoning_effort,
            directory,
            version,
            approval,
            sandbox,
            xtreme_ui_enabled,
            is_collaboration,
            collaboration_mode,
        )
    }

    pub(crate) fn new_with_style(
        model: String,
        model_style: Style,
        reasoning_effort: Option<ReasoningEffortConfig>,
        directory: std::path::PathBuf,
        version: &'static str,
        approval: AskForApproval,
        sandbox: SandboxPolicy,
        xtreme_ui_enabled: bool,
        is_collaboration: bool,
        collaboration_mode: CollaborationMode,
    ) -> Self {
        Self {
            version,
            model,
            model_style,
            reasoning_effort,
            directory,
            approval,
            sandbox,
            xtreme_ui_enabled,
            is_collaboration,
            collaboration_mode,
        }
    }

    fn collaboration_mode_label(&self) -> Option<&'static str> {
        if !self.is_collaboration {
            return None;
        }
        match self.collaboration_mode.mode {
            ModeKind::Plan => Some("Plan"),
            ModeKind::Default => Some("Code"),
            ModeKind::PairProgramming => Some("Pair Programming"),
            ModeKind::Execute => Some("Execute"),
        }
    }

    fn format_directory(&self, max_width: Option<usize>) -> String {
        Self::format_directory_inner(&self.directory, max_width)
    }

    fn format_directory_inner(directory: &Path, max_width: Option<usize>) -> String {
        let formatted = if let Some(rel) = crate::exec_command::relativize_to_home(directory) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
            }
        } else {
            directory.display().to_string()
        };

        if let Some(max_width) = max_width {
            if max_width == 0 {
                return String::new();
            }
            if UnicodeWidthStr::width(formatted.as_str()) > max_width {
                return crate::text_formatting::center_truncate_path(&formatted, max_width);
            }
        }

        formatted
    }

    fn reasoning_label(&self) -> Option<&'static str> {
        self.reasoning_effort.map(|effort| match effort {
            ReasoningEffortConfig::Minimal => "minimal",
            ReasoningEffortConfig::Low => "low",
            ReasoningEffortConfig::Medium => "medium",
            ReasoningEffortConfig::High => "high",
            ReasoningEffortConfig::XHigh => "xhigh",
            ReasoningEffortConfig::None => "none",
        })
    }
}

impl HistoryCell for XcodexSessionHeaderHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(inner_width) = card_inner_width(width, SESSION_HEADER_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let make_row = |spans: Vec<Span<'static>>| Line::from(spans);

        let title_spans = session_header_title_spans(self.version, self.xtreme_ui_enabled);

        const CHANGE_MODEL_HINT_COMMAND: &str = "/model";
        const CHANGE_MODEL_HINT_EXPLANATION: &str = " to change";
        const CHANGE_MODE_HINT_EXPLANATION: &str = " to toggle plan mode";
        const DIR_LABEL: &str = "directory:";
        let label_width = DIR_LABEL.len();

        let power_spans = session_header_power_spans(
            self.xtreme_ui_enabled,
            self.approval,
            &self.sandbox,
            label_width,
        );
        let model_label = format!(
            "{model_label:<label_width$}",
            model_label = "model:",
            label_width = label_width
        );
        let reasoning_label = self.reasoning_label();
        let mut model_spans = vec![
            Span::from(format!("{model_label} ")).dim(),
            Span::styled(self.model.clone(), self.model_style),
        ];
        if let Some(reasoning) = reasoning_label {
            model_spans.push(Span::from(" "));
            model_spans.push(Span::from(reasoning));
        }
        model_spans.push("   ".dim());
        model_spans
            .push(Span::from(CHANGE_MODEL_HINT_COMMAND).set_style(crate::theme::accent_style()));
        model_spans.push(CHANGE_MODEL_HINT_EXPLANATION.dim());

        let mode_spans: Option<Vec<Span<'static>>> = if self.is_collaboration {
            let collab_label = format!(
                "{collab_label:<label_width$}",
                collab_label = "mode:",
                label_width = label_width
            );
            let mode_text = if self.model == "loading" {
                "loading"
            } else if let Some(mode_label) = self.collaboration_mode_label() {
                mode_label
            } else {
                "Custom"
            };
            let mut spans = vec![
                Span::from(format!("{collab_label} ")).dim(),
                Span::styled(mode_text, self.model_style),
            ];
            spans.push("   ".dim());
            let shift_tab_span: Span<'static> = key_hint::shift(KeyCode::Tab).into();
            spans.push(shift_tab_span.cyan());
            spans.push(CHANGE_MODE_HINT_EXPLANATION.dim());
            Some(spans)
        } else {
            None
        };

        let dir_label = format!("{DIR_LABEL:<label_width$}");
        let dir_prefix = format!("{dir_label} ");
        let dir_prefix_width = UnicodeWidthStr::width(dir_prefix.as_str());
        let dir_max_width = inner_width.saturating_sub(dir_prefix_width);
        let dir = self.format_directory(Some(dir_max_width));
        let dir_spans = vec![Span::from(dir_prefix).dim(), Span::from(dir)];

        let mut lines = Vec::new();
        lines.push(make_row(title_spans));
        lines.push(make_row(Vec::new()));
        if let Some(spans) = power_spans {
            lines.push(make_row(spans));
        }
        if let Some(spans) = mode_spans {
            lines.push(make_row(spans));
        }
        lines.push(make_row(model_spans));
        if let Some(version) = codex_core::build_info::pyo3_python_version() {
            let python_label = format!(
                "{python_label:<label_width$}",
                python_label = "python:",
                label_width = label_width
            );
            lines.push(make_row(vec![
                Span::from(format!("{python_label} ")).dim(),
                Span::from(version),
            ]));
        }
        lines.push(make_row(dir_spans));

        with_border(lines)
    }
}

pub(crate) fn new_session_info(
    config: &Config,
    requested_model: &str,
    event: SessionConfiguredEvent,
    is_first_event: bool,
    is_collaboration: bool,
    collaboration_mode: CollaborationMode,
) -> SessionInfoCell {
    let SessionConfiguredEvent {
        model,
        reasoning_effort,
        ..
    } = event;

    let header = XcodexSessionHeaderHistoryCell::new(
        model.clone(),
        reasoning_effort,
        config.cwd.clone(),
        CODEX_CLI_VERSION,
        config.permissions.approval_policy.value(),
        config.permissions.sandbox_policy.get().clone(),
        xtreme::xtreme_ui_enabled(config),
        is_collaboration,
        collaboration_mode,
    );
    let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header)];

    if is_first_event {
        parts.push(Box::new(PlainHistoryCell::new(
            session_first_event_help_lines(),
        )));
    } else {
        if let Some(tooltips) = maybe_xcodex_tooltips_cell(config.show_tooltips) {
            parts.push(tooltips);
        } else if config.show_tooltips
            && let Some(tooltips) = tooltips::random_tooltip().map(FallbackTooltipHistoryCell::new)
        {
            parts.push(Box::new(tooltips));
        }
        if requested_model != model {
            let lines = vec![
                "model changed:".magenta().bold().into(),
                format!("requested: {requested_model}").into(),
                format!("used: {model}").into(),
            ];
            parts.push(Box::new(PlainHistoryCell::new(lines)));
        }
    }

    SessionInfoCell::new(parts)
}

pub(crate) fn new_session_info_with_help_lines(
    config: &Config,
    requested_model: &str,
    event: SessionConfiguredEvent,
    help_lines: Vec<Line<'static>>,
    is_collaboration: bool,
    collaboration_mode: CollaborationMode,
) -> SessionInfoCell {
    let SessionConfiguredEvent {
        model,
        reasoning_effort,
        ..
    } = event;

    let header = XcodexSessionHeaderHistoryCell::new(
        model.clone(),
        reasoning_effort,
        config.cwd.clone(),
        CODEX_CLI_VERSION,
        config.permissions.approval_policy.value(),
        config.permissions.sandbox_policy.get().clone(),
        xtreme::xtreme_ui_enabled(config),
        is_collaboration,
        collaboration_mode,
    );

    let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header)];
    parts.push(Box::new(PlainHistoryCell::new(help_lines)));

    if requested_model != model {
        let lines = vec![
            "model changed:".magenta().bold().into(),
            format!("requested: {requested_model}").into(),
            format!("used: {model}").into(),
        ];
        parts.push(Box::new(PlainHistoryCell::new(lines)));
    }

    SessionInfoCell::new(parts)
}

pub(crate) fn approval_agent_name() -> &'static str {
    "xcodex"
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TurnSummary {
    pub(crate) exec_commands: usize,
    pub(crate) mcp_calls: usize,
    pub(crate) patches: usize,
    pub(crate) files_changed: usize,
}

impl TurnSummary {
    pub(crate) fn is_empty(&self) -> bool {
        self.exec_commands == 0
            && self.mcp_calls == 0
            && self.patches == 0
            && self.files_changed == 0
    }
}

#[derive(Debug)]
pub(crate) struct XcodexFinalMessageSeparator {
    elapsed_seconds: Option<u64>,
    show_ramp_separator: bool,
    xtreme_ui_enabled: bool,
    completion_label: Option<String>,
    turn_summary: Option<TurnSummary>,
}

impl XcodexFinalMessageSeparator {
    pub(crate) fn new(
        elapsed_seconds: Option<u64>,
        show_ramp_separator: bool,
        xtreme_ui_enabled: bool,
        completion_label: Option<String>,
        turn_summary: Option<TurnSummary>,
    ) -> Self {
        Self {
            elapsed_seconds,
            show_ramp_separator,
            xtreme_ui_enabled,
            completion_label,
            turn_summary,
        }
    }
}

impl HistoryCell for XcodexFinalMessageSeparator {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let elapsed_seconds = self
            .elapsed_seconds
            .map(crate::status_indicator_widget::fmt_elapsed_compact);
        if let Some(elapsed_seconds) = elapsed_seconds {
            if self.show_ramp_separator {
                let mut suffix_parts: Vec<String> = Vec::new();
                if let Some(summary) = self.turn_summary.as_ref()
                    && !summary.is_empty()
                {
                    if summary.exec_commands > 0 {
                        suffix_parts.push(format!("{} cmds", summary.exec_commands));
                    }
                    if summary.mcp_calls > 0 {
                        suffix_parts.push(format!("{} tools", summary.mcp_calls));
                    }
                    if summary.patches > 0 {
                        suffix_parts.push(format!("{} edits", summary.patches));
                    }
                    if summary.files_changed > 0 {
                        suffix_parts.push(format!("{} files", summary.files_changed));
                    }
                }

                let suffix = if suffix_parts.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", suffix_parts.join(" · "))
                };

                let completion = self.completion_label.as_deref().unwrap_or("Overclocked");
                let mut spans: Vec<Span<'static>> = vec![
                    "─ ".dim(),
                    crate::xtreme::bolt_span(self.xtreme_ui_enabled),
                    format!(" {completion} in {elapsed_seconds}{suffix} ").dim(),
                ];
                let spans_width: usize =
                    spans.iter().map(|span| span.content.as_ref().width()).sum();
                spans.push(
                    "─"
                        .repeat((width as usize).saturating_sub(spans_width))
                        .dim(),
                );
                return vec![Line::from(spans)];
            }

            let worked_for = format!("─ Worked for {elapsed_seconds} ─");
            let worked_for_width = worked_for.width();
            vec![
                Line::from_iter([
                    worked_for,
                    "─".repeat((width as usize).saturating_sub(worked_for_width)),
                ])
                .dim(),
            ]
        } else {
            vec![Line::from_iter(["─".repeat(width as usize).dim()])]
        }
    }
}

pub(crate) fn placeholder_session_header_cell(
    config: &Config,
    placeholder_style: Style,
    model: &str,
    version: &'static str,
) -> Box<dyn HistoryCell> {
    if !config::is_xcodex_invocation() {
        return Box::new(
            crate::history_cell::SessionHeaderHistoryCell::new_with_style(
                model.to_string(),
                placeholder_style,
                None,
                config.cwd.clone(),
                version,
            ),
        );
    }

    Box::new(XcodexSessionHeaderHistoryCell::new_with_style(
        model.to_string(),
        placeholder_style,
        None,
        config.cwd.clone(),
        version,
        config.permissions.approval_policy.value(),
        config.permissions.sandbox_policy.get().clone(),
        xtreme::xtreme_ui_enabled(config),
        false,
        CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: model.to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn render_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn xcodex_tooltips_history_cell_renders_both_lines() {
        let cell = XcodexTooltipsHistoryCell::new(
            Some("xcodex tip".to_string()),
            Some("codex tip".to_string()),
        )
        .expect("cell");
        assert_eq!(
            render_lines(&cell.display_lines(80)),
            vec!["  ⚡Tips: xcodex tip", "  Tips: codex tip"],
        );
    }

    fn relevant_header_lines(lines: &[Line<'static>]) -> String {
        render_lines(lines)
            .into_iter()
            .filter(|line| {
                line.contains("mode:")
                    || line.contains("model:")
                    || line.contains("directory:")
                    || line.contains("/model")
                    || line.contains("shift + tab")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn collaboration_header_shows_mode_and_model_rows() {
        let cell = XcodexSessionHeaderHistoryCell::new(
            "gpt-5.3-codex high".to_string(),
            Some(ReasoningEffortConfig::High),
            std::path::PathBuf::from("/Users/test/repo"),
            "0.0.0",
            AskForApproval::Never,
            SandboxPolicy::DangerFullAccess,
            false,
            true,
            CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: "gpt-5.3-codex high".to_string(),
                    reasoning_effort: Some(ReasoningEffortConfig::High),
                    developer_instructions: None,
                },
            },
        );

        assert_snapshot!(relevant_header_lines(&cell.display_lines(120)));
    }
}
