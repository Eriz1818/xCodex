use std::io::Result;
use std::sync::Arc;
use std::time::Duration;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::BottomPaneParams;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::FooterMode;
use crate::bottom_pane::FooterProps;
use crate::bottom_pane::ThemeEditorView;
use crate::bottom_pane::render_footer;
use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::Insets;
use crate::render::renderable::InsetRenderable;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;
use crate::tui;
use crate::tui::TuiEvent;
use codex_core::features::Features;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

pub(crate) enum Overlay {
    Transcript(TranscriptOverlay),
    Static(StaticOverlay),
    ThemeSelector(ThemeSelectorOverlay),
}

impl Overlay {
    pub(crate) fn new_transcript(cells: Vec<Arc<dyn HistoryCell>>) -> Self {
        Self::Transcript(TranscriptOverlay::new(cells))
    }

    pub(crate) fn new_static_with_lines(lines: Vec<Line<'static>>, title: String) -> Self {
        Self::Static(StaticOverlay::with_title(lines, title))
    }

    pub(crate) fn new_static_with_renderables(
        renderables: Vec<Box<dyn Renderable>>,
        title: String,
    ) -> Self {
        Self::Static(StaticOverlay::with_renderables(renderables, title))
    }

    pub(crate) fn new_theme_selector(
        app_event_tx: AppEventSender,
        config: codex_core::config::Config,
        terminal_bg: Option<(u8, u8, u8)>,
    ) -> Self {
        Self::ThemeSelector(ThemeSelectorOverlay::new(app_event_tx, config, terminal_bg))
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match self {
            Overlay::Transcript(o) => o.handle_event(tui, event),
            Overlay::Static(o) => o.handle_event(tui, event),
            Overlay::ThemeSelector(o) => o.handle_event(tui, event),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        match self {
            Overlay::Transcript(o) => o.is_done(),
            Overlay::Static(o) => o.is_done(),
            Overlay::ThemeSelector(o) => o.is_done(),
        }
    }
}

const KEY_UP: KeyBinding = key_hint::plain(KeyCode::Up);
const KEY_DOWN: KeyBinding = key_hint::plain(KeyCode::Down);
const KEY_K: KeyBinding = key_hint::plain(KeyCode::Char('k'));
const KEY_J: KeyBinding = key_hint::plain(KeyCode::Char('j'));
const KEY_PAGE_UP: KeyBinding = key_hint::plain(KeyCode::PageUp);
const KEY_PAGE_DOWN: KeyBinding = key_hint::plain(KeyCode::PageDown);
const KEY_SPACE: KeyBinding = key_hint::plain(KeyCode::Char(' '));
const KEY_SHIFT_SPACE: KeyBinding = key_hint::shift(KeyCode::Char(' '));
const KEY_HOME: KeyBinding = key_hint::plain(KeyCode::Home);
const KEY_END: KeyBinding = key_hint::plain(KeyCode::End);
const KEY_CTRL_F: KeyBinding = key_hint::ctrl(KeyCode::Char('f'));
const KEY_CTRL_D: KeyBinding = key_hint::ctrl(KeyCode::Char('d'));
const KEY_CTRL_B: KeyBinding = key_hint::ctrl(KeyCode::Char('b'));
const KEY_CTRL_U: KeyBinding = key_hint::ctrl(KeyCode::Char('u'));
const KEY_TAB: KeyBinding = key_hint::plain(KeyCode::Tab);
const KEY_Q: KeyBinding = key_hint::plain(KeyCode::Char('q'));
const KEY_ESC: KeyBinding = key_hint::plain(KeyCode::Esc);
const KEY_ENTER: KeyBinding = key_hint::plain(KeyCode::Enter);
const KEY_CTRL_T: KeyBinding = key_hint::ctrl(KeyCode::Char('t'));
const KEY_CTRL_C: KeyBinding = key_hint::ctrl(KeyCode::Char('c'));
const KEY_D: KeyBinding = key_hint::plain(KeyCode::Char('d'));
const KEY_E: KeyBinding = key_hint::plain(KeyCode::Char('e'));

// Common pager navigation hints rendered on the first line
const PAGER_KEY_HINTS: &[(&[KeyBinding], &str)] = &[
    (&[KEY_UP, KEY_DOWN], "to scroll"),
    (&[KEY_PAGE_UP, KEY_PAGE_DOWN], "to page"),
    (&[KEY_HOME, KEY_END], "to jump"),
];

// Render a single line of key hints from (key(s), description) pairs.
fn render_key_hints(area: Rect, buf: &mut Buffer, pairs: &[(&[KeyBinding], &str)]) {
    let mut spans: Vec<Span<'static>> = vec![" ".into()];
    let mut first = true;
    for (keys, desc) in pairs {
        if !first {
            spans.push("   ".into());
        }
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                spans.push("/".into());
            }
            spans.push(Span::from(key));
        }
        spans.push(" ".into());
        spans.push(Span::from(desc.to_string()));
        first = false;
    }
    Paragraph::new(vec![Line::from(spans).dim()]).render_ref(area, buf);
}

pub(crate) struct ThemeSelectorOverlay {
    app_event_tx: AppEventSender,
    config: codex_core::config::Config,
    edit_variant: codex_core::themes::ThemeVariant,
    theme_entries: Vec<ThemeEntry>,
    selected_idx: usize,
    scroll_top: usize,
    last_previewed: Option<String>,
    mode: ThemeSelectorMode,
    applied: bool,
    is_done: bool,
    frame_requester: Option<crate::tui::FrameRequester>,
}

#[derive(Clone, Debug)]
struct ThemeEntry {
    name: String,
    variant: codex_core::themes::ThemeVariant,
}

impl ThemeSelectorOverlay {
    fn new(
        app_event_tx: AppEventSender,
        config: codex_core::config::Config,
        terminal_bg: Option<(u8, u8, u8)>,
    ) -> Self {
        use codex_core::themes::ThemeCatalog;
        use codex_core::themes::ThemeVariant;

        let edit_variant = crate::theme::active_variant(&config, terminal_bg);
        let current_theme = match edit_variant {
            ThemeVariant::Light => config.themes.light.as_deref(),
            ThemeVariant::Dark => config.themes.dark.as_deref(),
        }
        .unwrap_or("default")
        .to_string();

        let mut theme_entries: Vec<ThemeEntry> = match ThemeCatalog::load(&config) {
            Ok(catalog) => catalog
                .list_names()
                .map(|(name, variant)| ThemeEntry {
                    name: name.to_string(),
                    variant,
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        if !theme_entries.iter().any(|entry| entry.name == "default") {
            theme_entries.insert(
                0,
                ThemeEntry {
                    name: "default".to_string(),
                    variant: ThemeVariant::Dark,
                },
            );
        }

        fn variant_order(variant: ThemeVariant) -> u8 {
            match variant {
                ThemeVariant::Light => 0,
                ThemeVariant::Dark => 1,
            }
        }

        theme_entries.sort_by(|a, b| {
            if a.name == "default" {
                std::cmp::Ordering::Less
            } else if b.name == "default" {
                std::cmp::Ordering::Greater
            } else {
                variant_order(a.variant)
                    .cmp(&variant_order(b.variant))
                    .then_with(|| a.name.cmp(&b.name))
            }
        });

        let selected_idx = theme_entries
            .iter()
            .position(|entry| entry.name == current_theme)
            .unwrap_or(0);

        Self {
            app_event_tx,
            config,
            edit_variant,
            theme_entries,
            selected_idx,
            scroll_top: 0,
            last_previewed: None,
            mode: ThemeSelectorMode::Picker {
                preview_scroll: 0,
                diff_bg: true,
            },
            applied: false,
            is_done: false,
            frame_requester: None,
        }
    }

    fn selected_theme_variant(&self) -> codex_core::themes::ThemeVariant {
        self.theme_entries
            .get(self.selected_idx)
            .or_else(|| self.theme_entries.first())
            .map(|entry| entry.variant)
            .unwrap_or(codex_core::themes::ThemeVariant::Dark)
    }

    fn selected_theme(&self) -> &str {
        self.theme_entries
            .get(self.selected_idx)
            .or_else(|| self.theme_entries.first())
            .map(|entry| entry.name.as_str())
            .unwrap_or("default")
    }

    fn set_edit_variant(&mut self, variant: codex_core::themes::ThemeVariant) {
        use codex_core::themes::ThemeVariant;

        self.edit_variant = variant;
        let desired_theme = match variant {
            ThemeVariant::Light => self.config.themes.light.as_deref(),
            ThemeVariant::Dark => self.config.themes.dark.as_deref(),
        }
        .unwrap_or("default");

        if let Some(idx) = self
            .theme_entries
            .iter()
            .position(|entry| entry.name == desired_theme)
        {
            self.selected_idx = idx;
            self.ensure_preview_applied();
        }
    }

    fn ensure_preview_applied(&mut self) {
        let theme = self.selected_theme().to_string();
        if self.last_previewed.as_deref() == Some(theme.as_str()) {
            return;
        }
        self.last_previewed = Some(theme.clone());
        self.app_event_tx.send(AppEvent::PreviewTheme { theme });
    }

    fn move_selection(&mut self, delta: isize) {
        if self.theme_entries.is_empty() {
            return;
        }
        let len = self.theme_entries.len() as isize;
        let next = (self.selected_idx as isize + delta).rem_euclid(len) as usize;
        self.selected_idx = next;
        self.ensure_preview_applied();
    }

    fn open_editor(&mut self) {
        use codex_core::themes::ThemeCatalog;

        let base_theme_name = self.selected_theme().to_string();
        let base_theme = ThemeCatalog::load(&self.config)
            .ok()
            .and_then(|catalog| catalog.get(base_theme_name.as_str()).cloned())
            .unwrap_or_else(ThemeCatalog::built_in_default);

        let suggested_name = if base_theme_name == "default" {
            "my-theme".to_string()
        } else {
            format!("{base_theme_name}-custom")
        };

        self.mode = ThemeSelectorMode::Editor(ThemeEditorView::new(
            self.config.clone(),
            self.selected_theme_variant(),
            base_theme_name,
            base_theme,
            suggested_name,
            self.app_event_tx.clone(),
        ));
    }

    fn visible_items(&self, list_height: u16) -> usize {
        usize::from(list_height.max(1)).min(self.theme_entries.len())
    }

    fn ensure_visible(&mut self, list_height: u16) {
        let visible = self.visible_items(list_height);
        if visible == 0 {
            self.scroll_top = 0;
            return;
        }
        if self.selected_idx < self.scroll_top {
            self.scroll_top = self.selected_idx;
        } else if self.selected_idx >= self.scroll_top + visible {
            self.scroll_top = self.selected_idx + 1 - visible;
        }
    }

    fn apply_selection(&mut self) {
        let theme = self.selected_theme().to_string();
        let variant = self.edit_variant;
        self.applied = true;
        self.app_event_tx
            .send(AppEvent::PersistThemeSelection { variant, theme });
        self.is_done = true;
    }

    fn cancel(&mut self) {
        if !self.applied {
            self.app_event_tx.send(AppEvent::CancelThemePreview);
        }
        self.is_done = true;
    }

    fn render_preview(&self, area: Rect, buf: &mut Buffer, scroll: u16, diff_bg: bool) -> u16 {
        if area.is_empty() {
            return 0;
        }

        fn buffer_to_lines(buf: &Buffer) -> Vec<Line<'static>> {
            let mut out = Vec::new();
            for y in 0..buf.area.height {
                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut run_style: Option<Style> = None;
                let mut run = String::new();

                for x in 0..buf.area.width {
                    let cell = &buf[(x, y)];
                    let symbol = cell.symbol();
                    let style = cell.style();

                    if run_style != Some(style) && !run.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut run), run_style.unwrap()));
                    }

                    if run.is_empty() {
                        run_style = Some(style);
                    } else if run_style != Some(style) {
                        run_style = Some(style);
                    }

                    run.push_str(symbol);
                }

                if let Some(style) = run_style
                    && !run.is_empty()
                {
                    spans.push(Span::styled(run, style));
                }

                out.push(Line::from(spans));
            }
            out
        }

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_style(crate::theme::transcript_style());
            }
        }

        let Some(frame_requester) = self.frame_requester.as_ref() else {
            return 0;
        };

        let user_style = crate::theme::transcript_style().patch(user_message_style());
        let diff_add = if diff_bg {
            crate::theme::diff_add_style()
        } else {
            crate::theme::diff_add_text_style()
        };
        let diff_del = if diff_bg {
            crate::theme::diff_del_style()
        } else {
            crate::theme::diff_del_text_style()
        };
        let diff_hunk = if diff_bg {
            crate::theme::diff_hunk_style()
        } else {
            crate::theme::diff_hunk_text_style()
        };
        let thought_style = crate::theme::transcript_dim_style().add_modifier(Modifier::ITALIC);

        let approval = crate::bottom_pane::ApprovalOverlay::new(
            crate::bottom_pane::ApprovalRequest::Exec {
                id: "preview-install".to_string(),
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "cd /Users/eriz/Dev/Pyfun/codex/codex-rs && just xcodex-install".to_string(),
                ],
                reason: None,
                proposed_execpolicy_amendment: None,
            },
            self.app_event_tx.clone(),
            Features::with_defaults(),
        );
        let approval_height = approval.desired_height(area.width);
        let mut approval_buf = Buffer::empty(Rect::new(0, 0, area.width, approval_height));
        for y in 0..approval_buf.area.height {
            for x in 0..approval_buf.area.width {
                approval_buf[(x, y)].set_symbol(" ");
                approval_buf[(x, y)].set_style(crate::theme::transcript_style());
            }
        }
        approval.render(*approval_buf.area(), &mut approval_buf);
        let approval_lines = buffer_to_lines(&approval_buf);

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(""),
            Line::from(vec![
                "› ".bold().dim(),
                Span::from("Show me the diff and explain it.").set_style(user_style),
            ]),
            Line::from(vec![
                "  ".into(),
                "diff preview ".dim(),
                if diff_bg {
                    "(bg highlight)".dim()
                } else {
                    "(text-only)".dim()
                },
            ]),
            Line::from(vec![
                Span::from("@@ -1,3 +1,4 @@").set_style(diff_hunk),
                " ".into(),
                "config".dim(),
            ]),
            Line::from(vec![Span::from("- old line").set_style(diff_del)]),
            Line::from(vec![Span::from("+ new line").set_style(diff_add)]),
            Line::from(vec![
                "• ".into(),
                "status: ".dim(),
                Span::from("Working").set_style(crate::theme::accent_style()),
                " · ".dim(),
                Span::from("warning").set_style(crate::theme::warning_style()),
                " · ".dim(),
                Span::from("error").set_style(crate::theme::error_style()),
                " · ".dim(),
                Span::from("success").set_style(crate::theme::success_style()),
            ]),
            Line::from(vec![
                "• ".into(),
                "link: ".dim(),
                "https://example.com".set_style(crate::theme::link_style().underlined()),
            ]),
            Line::from(""),
            Line::from("Adjusting background colors").style(thought_style),
            Line::from(""),
            Line::from("In `styles_for`, I noticed that `composer_bg` can end up too close to the transcript background. I want the composer surface to be derived from the theme’s background while still reading as slightly lighter, without depending on terminal defaults.")
                .style(thought_style),
            Line::from(""),
            Line::from("Inspecting color mapping").style(thought_style),
            Line::from(""),
            Line::from("I’ll check how `ThemeColorResolved` flows into the TUI styles and ensure we only blend based on the theme’s resolved RGB background (not the terminal’s fg/bg). Then the composer, status, and suggestion surfaces should stay consistent across terminals.")
                .style(thought_style),
            Line::from(""),
            Line::from(vec!["Approval required:".set_style(crate::theme::warning_style().bold())]),
        ];
        lines.extend(approval_lines);
        lines.extend([
            Line::from(""),
            Line::from(vec!["assistant: ".dim(), "Here’s the plan…".into()]),
            Line::from(""),
            Line::from(vec![
                "• ".into(),
                "Ran ".dim(),
                Span::from("cd /Users/eriz/Dev/Pyfun/codex/codex-rs")
                    .set_style(crate::theme::accent_style()),
                " && ".dim(),
                Span::from("cargo test -p codex-core").set_style(crate::theme::accent_style()),
            ]),
            Line::from(vec![
                "  └ ".dim(),
                "test result: ok(".dim(),
                Span::from("5 passed").set_style(crate::theme::success_style()),
                "; ".dim(),
                "0 failed".dim(),
                ")".dim(),
            ]),
        ]);

        let mut bottom_pane = BottomPane::new(BottomPaneParams {
            app_event_tx: self.app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            has_input_focus: true,
            enhanced_keys_supported: false,
            placeholder_text: "Ask xcodex to do anything".to_string(),
            disable_paste_burst: false,
            minimal_composer_borders: self.config.tui_composer_minimal_borders,
            xtreme_ui_enabled: crate::xtreme::xtreme_ui_enabled(&self.config),
            animations_enabled: self.config.animations,
            skills: None,
        });
        bottom_pane.set_slash_popup_max_rows(3);
        bottom_pane.set_composer_text("/".to_string());
        bottom_pane.ensure_status_indicator();
        bottom_pane.update_status("Working".to_string(), Some("Theme preview".to_string()));
        bottom_pane.set_context_window(Some(100), Some(0));
        bottom_pane.set_status_bar_git_options(true, true);
        bottom_pane.set_status_bar_git_context(
            Some("feat/themes".to_string()),
            Some("~/Dev/Pyfun/codex".to_string()),
        );

        let footer_height = 1u16;
        let desired_bottom_height = bottom_pane.desired_height(area.width);
        let max_bottom_height = area.height.saturating_sub(footer_height).saturating_sub(3);
        let bottom_height = desired_bottom_height.min(max_bottom_height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(bottom_height),
                Constraint::Length(footer_height),
            ])
            .split(area);

        let transcript_area = chunks[0];
        let bottom_pane_area = chunks[1];
        let footer_area = chunks[2];
        let (info_area, transcript_area) = if transcript_area.height >= 6 {
            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(5), Constraint::Min(0)])
                .split(transcript_area);
            (parts[0], parts[1])
        } else {
            (Rect::new(0, 0, 0, 0), transcript_area)
        };

        if !info_area.is_empty() {
            let title = Line::from(vec![
                "⚡ ".into(),
                "xtreme-Codex".bold(),
                format!(" (v{})", env!("CARGO_PKG_VERSION")).dim(),
            ]);
            let info = vec![
                Line::from(vec!["power:".dim(), " ".into(), "⚡⚡⚡".into()]),
                Line::from(vec![
                    "model:".dim(),
                    " ".into(),
                    "gpt-5.2 medium".into(),
                    "  ".into(),
                    "/mode".dim(),
                    " to change".dim(),
                ]),
                Line::from(vec![
                    "directory:".dim(),
                    " ".into(),
                    "~/Dev/Pyfun/codex".into(),
                ]),
            ];
            Paragraph::new(Text::from(info))
                .style(crate::theme::transcript_style())
                .block(Block::bordered().title(title))
                .render_ref(info_area, buf);
        }

        let visible_rows = transcript_area.height as usize;
        let max_scroll =
            u16::try_from(lines.len().saturating_sub(visible_rows)).unwrap_or(u16::MAX);
        let scroll = scroll.min(max_scroll);

        Paragraph::new(Text::from(lines))
            .style(crate::theme::transcript_style())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render_ref(transcript_area, buf);

        bottom_pane.render(bottom_pane_area, buf);

        render_footer(
            footer_area,
            buf,
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: true,
                context_window_percent: Some(100),
                context_window_used_tokens: Some(0),
                status_bar_git_branch: Some("feat/themes"),
                status_bar_worktree: Some("~/Dev/Pyfun/codex"),
                show_status_bar_git_branch: true,
                show_status_bar_worktree: true,
            },
        );

        max_scroll
    }

    fn render_theme_list(&mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(crate::theme::composer_style());
            }
        }

        let list_area = area;
        self.ensure_visible(list_area.height);

        let visible = self.visible_items(list_area.height);
        let start = self
            .scroll_top
            .min(self.theme_entries.len().saturating_sub(1));
        let end = (start + visible).min(self.theme_entries.len());

        for (row, idx) in (start..end).enumerate() {
            let y = list_area.y + row as u16;
            let entry = &self.theme_entries[idx];
            let variant_label = match entry.variant {
                codex_core::themes::ThemeVariant::Light => "Light",
                codex_core::themes::ThemeVariant::Dark => "Dark",
            };
            let mut line = Line::from(format!("{variant_label}  {}", entry.name));
            if idx == self.selected_idx {
                let style = crate::theme::composer_style()
                    .patch(crate::theme::accent_style())
                    .add_modifier(Modifier::BOLD);
                line = line.set_style(style);
            } else {
                line = line.set_style(crate::theme::composer_style());
            }
            line.render(
                Rect::new(list_area.x + 2, y, list_area.width.saturating_sub(2), 1),
                buf,
            );
        }

        // Footer key hints are rendered by the overlay layout, not in the list widget.
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        if self.frame_requester.is_none() {
            self.frame_requester = Some(tui.frame_requester());
        }

        if matches!(event, TuiEvent::Draw) {
            return self.handle_draw(tui);
        }

        match &mut self.mode {
            ThemeSelectorMode::Editor(editor) => match event {
                TuiEvent::Key(key_event) => {
                    editor.handle_key_event(key_event);
                    if editor.is_complete() {
                        self.is_done = true;
                    }
                    tui.frame_requester().schedule_frame();
                    Ok(())
                }
                _ => Ok(()),
            },
            ThemeSelectorMode::Picker {
                preview_scroll,
                diff_bg,
            } => match event {
                TuiEvent::Key(key_event) => match key_event {
                    e if KEY_ESC.is_press(e) || KEY_Q.is_press(e) => {
                        self.cancel();
                        Ok(())
                    }
                    e if KEY_ENTER.is_press(e) => {
                        self.apply_selection();
                        Ok(())
                    }
                    e if KEY_TAB.is_press(e) => {
                        let next = match self.edit_variant {
                            codex_core::themes::ThemeVariant::Light => {
                                codex_core::themes::ThemeVariant::Dark
                            }
                            codex_core::themes::ThemeVariant::Dark => {
                                codex_core::themes::ThemeVariant::Light
                            }
                        };
                        self.set_edit_variant(next);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_UP.is_press(e) => {
                        self.move_selection(-1);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_DOWN.is_press(e) => {
                        self.move_selection(1);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_U.is_press(e) => {
                        *preview_scroll = preview_scroll.saturating_sub(3);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_D.is_press(e) => {
                        *preview_scroll = preview_scroll.saturating_add(3);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_D.is_press(e) => {
                        *diff_bg = !*diff_bg;
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_E.is_press(e) => {
                        self.open_editor();
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    _ => Ok(()),
                },
                _ => Ok(()),
            },
        }
    }

    fn handle_draw(&mut self, tui: &mut tui::Tui) -> Result<()> {
        match &mut self.mode {
            ThemeSelectorMode::Editor(editor) => {
                tui.draw(u16::MAX, |frame| {
                    let area = frame.area();
                    for y in area.top()..area.bottom() {
                        for x in area.left()..area.right() {
                            frame.buffer_mut()[(x, y)].set_symbol(" ");
                            frame.buffer_mut()[(x, y)].set_style(crate::theme::transcript_style());
                        }
                    }
                    editor.render(area, frame.buffer_mut());
                    if let Some((x, y)) = editor.cursor_pos(area) {
                        frame.set_cursor_position((x, y));
                    }
                })?;
                Ok(())
            }
            ThemeSelectorMode::Picker { .. } => {
                self.ensure_preview_applied();
                let (requested_scroll, diff_bg) = match &self.mode {
                    ThemeSelectorMode::Picker {
                        preview_scroll,
                        diff_bg,
                    } => (*preview_scroll, *diff_bg),
                    ThemeSelectorMode::Editor(_) => (0, true),
                };

                let mut max_scroll = 0u16;
                tui.draw(u16::MAX, |frame| {
                    let area = frame.area();
                    for y in area.top()..area.bottom() {
                        for x in area.left()..area.right() {
                            frame.buffer_mut()[(x, y)].set_symbol(" ");
                            frame.buffer_mut()[(x, y)].set_style(crate::theme::transcript_style());
                        }
                    }

                    let parts = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(0), Constraint::Length(1)])
                        .split(area);

                    let body_area = parts[0];
                    let footer_area = parts[1];

                    let body_parts = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
                        .split(body_area);

                    let left = body_parts[0];
                    let right = body_parts[1];

                    let title_height = 2u16.min(left.height);

                    let left_parts = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(title_height), Constraint::Min(0)])
                        .split(left);
                    let left_title_area = left_parts[0];
                    let left_content_area = left_parts[1];

                    for y in left_title_area.top()..left_title_area.bottom() {
                        for x in left_title_area.left()..left_title_area.right() {
                            frame.buffer_mut()[(x, y)].set_symbol(" ");
                            frame.buffer_mut()[(x, y)].set_style(crate::theme::composer_style());
                        }
                    }

                    let selected_title = Line::from(vec![
                        "Themes (selecting for ".dim(),
                        match self.edit_variant {
                            codex_core::themes::ThemeVariant::Light => "Light mode".into(),
                            codex_core::themes::ThemeVariant::Dark => "Dark mode".into(),
                        },
                        ")".dim(),
                    ]);
                    let wrapped_title =
                        crate::wrapping::word_wrap_line(&selected_title, usize::from(left.width));
                    let wrapped_title: Vec<Line<'_>> = wrapped_title
                        .into_iter()
                        .take(usize::from(title_height))
                        .collect();
                    Paragraph::new(Text::from(wrapped_title))
                        .style(crate::theme::composer_style())
                        .render_ref(left_title_area, frame.buffer_mut());

                    let right_parts = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(title_height), Constraint::Min(0)])
                        .split(right);
                    let right_title_area = right_parts[0];
                    let right_content_area = right_parts[1];

                    let right_title_area = Rect::new(
                        right_title_area.x.saturating_add(1),
                        right_title_area.y,
                        right_title_area.width.saturating_sub(1),
                        right_title_area.height,
                    );
                    Paragraph::new(Line::from("Theme Preview"))
                        .style(crate::theme::transcript_style())
                        .render_ref(right_title_area, frame.buffer_mut());

                    let right_content_area = Rect::new(
                        right_content_area.x.saturating_add(1),
                        right_content_area.y,
                        right_content_area.width.saturating_sub(1),
                        right_content_area.height,
                    );
                    max_scroll = self.render_preview(
                        right_content_area,
                        frame.buffer_mut(),
                        requested_scroll,
                        diff_bg,
                    );
                    self.render_theme_list(left_content_area, frame.buffer_mut());

                    let footer_parts = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
                        .split(footer_area);

                    render_key_hints(
                        footer_parts[0],
                        frame.buffer_mut(),
                        &[(&[KEY_UP, KEY_DOWN], "select"), (&[KEY_TAB], "toggle mode")],
                    );
                    render_key_hints(
                        footer_parts[1],
                        frame.buffer_mut(),
                        &[
                            (&[KEY_CTRL_U, KEY_CTRL_D], "scroll preview"),
                            (&[KEY_D], "toggle diff highlight"),
                        ],
                    );
                })?;

                if let ThemeSelectorMode::Picker { preview_scroll, .. } = &mut self.mode
                    && *preview_scroll > max_scroll
                {
                    *preview_scroll = max_scroll;
                    tui.frame_requester().schedule_frame();
                }
                Ok(())
            }
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

enum ThemeSelectorMode {
    Picker { preview_scroll: u16, diff_bg: bool },
    Editor(ThemeEditorView),
}

/// Generic widget for rendering a pager view.
struct PagerView {
    renderables: Vec<Box<dyn Renderable>>,
    scroll_offset: usize,
    title: String,
    last_content_height: Option<usize>,
    last_rendered_height: Option<usize>,
    /// If set, on next render ensure this chunk is visible.
    pending_scroll_chunk: Option<usize>,
}

impl PagerView {
    fn new(renderables: Vec<Box<dyn Renderable>>, title: String, scroll_offset: usize) -> Self {
        Self {
            renderables,
            scroll_offset,
            title,
            last_content_height: None,
            last_rendered_height: None,
            pending_scroll_chunk: None,
        }
    }

    fn content_height(&self, width: u16) -> usize {
        self.renderables
            .iter()
            .map(|c| c.desired_height(width) as usize)
            .sum()
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if !area.is_empty() {
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    buf[(x, y)].set_symbol(" ");
                    buf[(x, y)].set_style(crate::theme::transcript_style());
                }
            }
        }
        self.render_header(area, buf);
        let content_area = self.content_area(area);
        self.update_last_content_height(content_area.height);
        let content_height = self.content_height(content_area.width);
        self.last_rendered_height = Some(content_height);
        // If there is a pending request to scroll a specific chunk into view,
        // satisfy it now that wrapping is up to date for this width.
        if let Some(idx) = self.pending_scroll_chunk.take() {
            self.ensure_chunk_visible(idx, content_area);
        }
        self.scroll_offset = self
            .scroll_offset
            .min(content_height.saturating_sub(content_area.height as usize));

        self.render_content(content_area, buf);

        self.render_bottom_bar(area, content_area, buf, content_height);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        Span::from("/ ".repeat(area.width as usize / 2))
            .dim()
            .render_ref(area, buf);
        let header = format!("/ {}", self.title);
        header.dim().render_ref(area, buf);
    }

    fn render_content(&self, area: Rect, buf: &mut Buffer) {
        let mut y = -(self.scroll_offset as isize);
        let mut drawn_bottom = area.y;
        for renderable in &self.renderables {
            let top = y;
            let height = renderable.desired_height(area.width) as isize;
            y += height;
            let bottom = y;
            if bottom < area.y as isize {
                continue;
            }
            if top > area.y as isize + area.height as isize {
                break;
            }
            if top < 0 {
                let drawn = render_offset_content(area, buf, &**renderable, (-top) as u16);
                drawn_bottom = drawn_bottom.max(area.y + drawn);
            } else {
                let draw_height = (height as u16).min(area.height.saturating_sub(top as u16));
                let draw_area = Rect::new(area.x, area.y + top as u16, area.width, draw_height);
                renderable.render(draw_area, buf);
                drawn_bottom = drawn_bottom.max(draw_area.y.saturating_add(draw_area.height));
            }
        }

        for y in drawn_bottom..area.bottom() {
            if area.width == 0 {
                break;
            }
            buf[(area.x, y)] = Cell::from('~');
            for x in area.x + 1..area.right() {
                buf[(x, y)] = Cell::from(' ');
            }
        }
    }

    fn render_bottom_bar(
        &self,
        full_area: Rect,
        content_area: Rect,
        buf: &mut Buffer,
        total_len: usize,
    ) {
        let sep_y = content_area.bottom();
        let sep_rect = Rect::new(full_area.x, sep_y, full_area.width, 1);

        Span::from("─".repeat(sep_rect.width as usize))
            .style(crate::theme::border_style())
            .render_ref(sep_rect, buf);
        let percent = if total_len == 0 {
            100
        } else {
            let max_scroll = total_len.saturating_sub(content_area.height as usize);
            if max_scroll == 0 {
                100
            } else {
                (((self.scroll_offset.min(max_scroll)) as f32 / max_scroll as f32) * 100.0).round()
                    as u8
            }
        };
        let pct_text = format!(" {percent}% ");
        let pct_w = pct_text.chars().count() as u16;
        let pct_x = sep_rect.x + sep_rect.width - pct_w - 1;
        Span::from(pct_text)
            .style(crate::theme::dim_style())
            .render_ref(Rect::new(pct_x, sep_rect.y, pct_w, 1), buf);
    }

    fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) -> Result<()> {
        match key_event {
            e if KEY_UP.is_press(e) || KEY_K.is_press(e) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            e if KEY_DOWN.is_press(e) || KEY_J.is_press(e) => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            e if KEY_PAGE_UP.is_press(e)
                || KEY_SHIFT_SPACE.is_press(e)
                || KEY_CTRL_B.is_press(e) =>
            {
                let page_height = self.page_height(tui.terminal.viewport_area);
                self.scroll_offset = self.scroll_offset.saturating_sub(page_height);
            }
            e if KEY_PAGE_DOWN.is_press(e) || KEY_SPACE.is_press(e) || KEY_CTRL_F.is_press(e) => {
                let page_height = self.page_height(tui.terminal.viewport_area);
                self.scroll_offset = self.scroll_offset.saturating_add(page_height);
            }
            e if KEY_CTRL_D.is_press(e) => {
                let area = self.content_area(tui.terminal.viewport_area);
                let half_page = (area.height as usize).saturating_add(1) / 2;
                self.scroll_offset = self.scroll_offset.saturating_add(half_page);
            }
            e if KEY_CTRL_U.is_press(e) => {
                let area = self.content_area(tui.terminal.viewport_area);
                let half_page = (area.height as usize).saturating_add(1) / 2;
                self.scroll_offset = self.scroll_offset.saturating_sub(half_page);
            }
            e if KEY_HOME.is_press(e) => {
                self.scroll_offset = 0;
            }
            e if KEY_END.is_press(e) => {
                self.scroll_offset = usize::MAX;
            }
            _ => {
                return Ok(());
            }
        }
        tui.frame_requester()
            .schedule_frame_in(Duration::from_millis(16));
        Ok(())
    }

    /// Returns the height of one page in content rows.
    ///
    /// Prefers the last rendered content height (excluding header/footer chrome);
    /// if no render has occurred yet, falls back to the content area height
    /// computed from the given viewport.
    fn page_height(&self, viewport_area: Rect) -> usize {
        self.last_content_height
            .unwrap_or_else(|| self.content_area(viewport_area).height as usize)
    }

    fn update_last_content_height(&mut self, height: u16) {
        self.last_content_height = Some(height as usize);
    }

    fn content_area(&self, area: Rect) -> Rect {
        let mut area = area;
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(2);
        area
    }
}

impl PagerView {
    fn is_scrolled_to_bottom(&self) -> bool {
        if self.scroll_offset == usize::MAX {
            return true;
        }
        let Some(height) = self.last_content_height else {
            return false;
        };
        if self.renderables.is_empty() {
            return true;
        }
        let Some(total_height) = self.last_rendered_height else {
            return false;
        };
        if total_height <= height {
            return true;
        }
        let max_scroll = total_height.saturating_sub(height);
        self.scroll_offset >= max_scroll
    }

    /// Request that the given text chunk index be scrolled into view on next render.
    fn scroll_chunk_into_view(&mut self, chunk_index: usize) {
        self.pending_scroll_chunk = Some(chunk_index);
    }

    fn ensure_chunk_visible(&mut self, idx: usize, area: Rect) {
        if area.height == 0 || idx >= self.renderables.len() {
            return;
        }
        let first = self
            .renderables
            .iter()
            .take(idx)
            .map(|r| r.desired_height(area.width) as usize)
            .sum();
        let last = first + self.renderables[idx].desired_height(area.width) as usize;
        let current_top = self.scroll_offset;
        let current_bottom = current_top.saturating_add(area.height.saturating_sub(1) as usize);
        if first < current_top {
            self.scroll_offset = first;
        } else if last > current_bottom {
            self.scroll_offset = last.saturating_sub(area.height.saturating_sub(1) as usize);
        }
    }
}

/// A renderable that caches its desired height.
struct CachedRenderable {
    renderable: Box<dyn Renderable>,
    height: std::cell::Cell<Option<u16>>,
    last_width: std::cell::Cell<Option<u16>>,
}

impl CachedRenderable {
    fn new(renderable: impl Into<Box<dyn Renderable>>) -> Self {
        Self {
            renderable: renderable.into(),
            height: std::cell::Cell::new(None),
            last_width: std::cell::Cell::new(None),
        }
    }
}

impl Renderable for CachedRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable.render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        if self.last_width.get() != Some(width) {
            let height = self.renderable.desired_height(width);
            self.height.set(Some(height));
            self.last_width.set(Some(width));
        }
        self.height.get().unwrap_or(0)
    }
}

struct CellRenderable {
    cell: Arc<dyn HistoryCell>,
    style: Style,
}

impl Renderable for CellRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let p =
            Paragraph::new(Text::from(self.cell.transcript_lines(area.width))).style(self.style);
        p.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.cell.desired_transcript_height(width)
    }
}

pub(crate) struct TranscriptOverlay {
    view: PagerView,
    cells: Vec<Arc<dyn HistoryCell>>,
    highlight_cell: Option<usize>,
    is_done: bool,
}

impl TranscriptOverlay {
    pub(crate) fn new(transcript_cells: Vec<Arc<dyn HistoryCell>>) -> Self {
        Self {
            view: PagerView::new(
                Self::render_cells(&transcript_cells, None),
                "T R A N S C R I P T".to_string(),
                usize::MAX,
            ),
            cells: transcript_cells,
            highlight_cell: None,
            is_done: false,
        }
    }

    fn render_cells(
        cells: &[Arc<dyn HistoryCell>],
        highlight_cell: Option<usize>,
    ) -> Vec<Box<dyn Renderable>> {
        cells
            .iter()
            .enumerate()
            .flat_map(|(i, c)| {
                let mut v: Vec<Box<dyn Renderable>> = Vec::new();
                let mut cell_renderable = if c.as_any().is::<UserHistoryCell>() {
                    Box::new(CachedRenderable::new(CellRenderable {
                        cell: c.clone(),
                        style: if highlight_cell == Some(i) {
                            user_message_style().reversed()
                        } else {
                            user_message_style()
                        },
                    })) as Box<dyn Renderable>
                } else {
                    Box::new(CachedRenderable::new(CellRenderable {
                        cell: c.clone(),
                        style: Style::default(),
                    })) as Box<dyn Renderable>
                };
                if !c.is_stream_continuation() && i > 0 {
                    cell_renderable = Box::new(InsetRenderable::new(
                        cell_renderable,
                        Insets::tlbr(1, 0, 0, 0),
                    ));
                }
                v.push(cell_renderable);
                v
            })
            .collect()
    }

    pub(crate) fn insert_cell(&mut self, cell: Arc<dyn HistoryCell>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        self.cells.push(cell);
        self.view.renderables = Self::render_cells(&self.cells, self.highlight_cell);
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    pub(crate) fn set_highlight_cell(&mut self, cell: Option<usize>) {
        self.highlight_cell = cell;
        self.view.renderables = Self::render_cells(&self.cells, self.highlight_cell);
        if let Some(idx) = self.highlight_cell {
            self.view.scroll_chunk_into_view(idx);
        }
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_key_hints(line1, buf, PAGER_KEY_HINTS);

        let mut pairs: Vec<(&[KeyBinding], &str)> =
            vec![(&[KEY_Q], "to quit"), (&[KEY_ESC], "to edit prev")];
        if self.highlight_cell.is_some() {
            pairs.push((&[KEY_ENTER], "to edit message"));
        }
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.view.render(top, buf);
        self.render_hints(bottom, buf);
    }
}

impl TranscriptOverlay {
    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) => match key_event {
                e if KEY_Q.is_press(e) || KEY_CTRL_C.is_press(e) || KEY_CTRL_T.is_press(e) => {
                    self.is_done = true;
                    Ok(())
                }
                other => self.view.handle_key_event(tui, other),
            },
            TuiEvent::Draw => {
                tui.draw(u16::MAX, |frame| {
                    self.render(frame.area(), frame.buffer);
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

pub(crate) struct StaticOverlay {
    view: PagerView,
    is_done: bool,
}

impl StaticOverlay {
    pub(crate) fn with_title(lines: Vec<Line<'static>>, title: String) -> Self {
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        Self::with_renderables(vec![Box::new(CachedRenderable::new(paragraph))], title)
    }

    pub(crate) fn with_renderables(renderables: Vec<Box<dyn Renderable>>, title: String) -> Self {
        Self {
            view: PagerView::new(renderables, title, 0),
            is_done: false,
        }
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_key_hints(line1, buf, PAGER_KEY_HINTS);
        let pairs: Vec<(&[KeyBinding], &str)> = vec![(&[KEY_Q], "to quit")];
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.view.render(top, buf);
        self.render_hints(bottom, buf);
    }
}

impl StaticOverlay {
    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) => match key_event {
                e if KEY_Q.is_press(e) || KEY_CTRL_C.is_press(e) => {
                    self.is_done = true;
                    Ok(())
                }
                other => self.view.handle_key_event(tui, other),
            },
            TuiEvent::Draw => {
                tui.draw(u16::MAX, |frame| {
                    self.render(frame.area(), frame.buffer);
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

fn render_offset_content(
    area: Rect,
    buf: &mut Buffer,
    renderable: &dyn Renderable,
    scroll_offset: u16,
) -> u16 {
    let height = renderable.desired_height(area.width);
    let mut tall_buf = Buffer::empty(Rect::new(
        0,
        0,
        area.width,
        height.min(area.height + scroll_offset),
    ));
    renderable.render(*tall_buf.area(), &mut tall_buf);
    let copy_height = area
        .height
        .min(tall_buf.area().height.saturating_sub(scroll_offset));
    for y in 0..copy_height {
        let src_y = y + scroll_offset;
        for x in 0..area.width {
            buf[(area.x + x, area.y + y)] = tall_buf[(x, src_y)].clone();
        }
    }

    copy_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_core::protocol::ExecCommandSource;
    use codex_core::protocol::ReviewDecision;
    use insta::assert_snapshot;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::exec_cell::CommandOutput;
    use crate::history_cell;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::new_patch_event;
    use codex_core::protocol::FileChange;
    use codex_protocol::parse_command::ParsedCommand;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Text;

    #[derive(Debug)]
    struct TestCell {
        lines: Vec<Line<'static>>,
    }

    impl crate::history_cell::HistoryCell for TestCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            self.lines.clone()
        }

        fn transcript_lines(&self, _width: u16) -> Vec<Line<'static>> {
            self.lines.clone()
        }
    }

    fn paragraph_block(label: &str, lines: usize) -> Box<dyn Renderable> {
        let text = Text::from(
            (0..lines)
                .map(|i| Line::from(format!("{label}{i}")))
                .collect::<Vec<_>>(),
        );
        Box::new(Paragraph::new(text)) as Box<dyn Renderable>
    }

    #[test]
    fn edit_prev_hint_is_visible() {
        let mut overlay = TranscriptOverlay::new(vec![Arc::new(TestCell {
            lines: vec![Line::from("hello")],
        })]);

        // Render into a small buffer and assert the backtrack hint is present
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        // Flatten buffer to a string and check for the hint text
        let mut s = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                s.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            s.push('\n');
        }
        assert!(
            s.contains("edit prev"),
            "expected 'edit prev' hint in overlay footer, got: {s:?}"
        );
    }

    #[test]
    fn transcript_overlay_snapshot_basic() {
        // Prepare a transcript overlay with a few lines
        let mut overlay = TranscriptOverlay::new(vec![
            Arc::new(TestCell {
                lines: vec![Line::from("alpha")],
            }),
            Arc::new(TestCell {
                lines: vec![Line::from("beta")],
            }),
            Arc::new(TestCell {
                lines: vec![Line::from("gamma")],
            }),
        ]);
        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    fn buffer_to_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let symbol = buf[(x, y)].symbol();
                if symbol.is_empty() {
                    out.push(' ');
                } else {
                    out.push(symbol.chars().next().unwrap_or(' '));
                }
            }
            // Trim trailing spaces for stability.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn transcript_overlay_apply_patch_scroll_vt100_clears_previous_page() {
        let cwd = PathBuf::from("/repo");
        let mut cells: Vec<Arc<dyn HistoryCell>> = Vec::new();

        let mut approval_changes = HashMap::new();
        approval_changes.insert(
            PathBuf::from("foo.txt"),
            FileChange::Add {
                content: "hello\nworld\n".to_string(),
            },
        );
        let approval_cell: Arc<dyn HistoryCell> = Arc::new(new_patch_event(approval_changes, &cwd));
        cells.push(approval_cell);

        let mut apply_changes = HashMap::new();
        apply_changes.insert(
            PathBuf::from("foo.txt"),
            FileChange::Add {
                content: "hello\nworld\n".to_string(),
            },
        );
        let apply_begin_cell: Arc<dyn HistoryCell> = Arc::new(new_patch_event(apply_changes, &cwd));
        cells.push(apply_begin_cell);

        let apply_end_cell: Arc<dyn HistoryCell> =
            history_cell::new_approval_decision_cell(vec!["ls".into()], ReviewDecision::Approved)
                .into();
        cells.push(apply_end_cell);

        let mut exec_cell = crate::exec_cell::new_active_exec_command(
            "exec-1".into(),
            vec!["bash".into(), "-lc".into(), "ls".into()],
            vec![ParsedCommand::Unknown { cmd: "ls".into() }],
            ExecCommandSource::Agent,
            None,
            true,
        );
        exec_cell.complete_call(
            "exec-1",
            CommandOutput {
                exit_code: 0,
                aggregated_output: "src\nREADME.md\n".into(),
                formatted_output: "src\nREADME.md\n".into(),
            },
            Duration::from_millis(420),
        );
        let exec_cell: Arc<dyn HistoryCell> = Arc::new(exec_cell);
        cells.push(exec_cell);

        let mut overlay = TranscriptOverlay::new(cells);
        let area = Rect::new(0, 0, 80, 12);
        let mut buf = Buffer::empty(area);

        overlay.render(area, &mut buf);
        overlay.view.scroll_offset = 0;
        overlay.render(area, &mut buf);

        let snapshot = buffer_to_text(&buf, area);
        assert_snapshot!("transcript_overlay_apply_patch_scroll_vt100", snapshot);
    }

    #[test]
    fn transcript_overlay_keeps_scroll_pinned_at_bottom() {
        let mut overlay = TranscriptOverlay::new(
            (0..20)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");

        assert!(
            overlay.view.is_scrolled_to_bottom(),
            "expected initial render to leave view at bottom"
        );

        overlay.insert_cell(Arc::new(TestCell {
            lines: vec!["tail".into()],
        }));

        assert_eq!(overlay.view.scroll_offset, usize::MAX);
    }

    #[test]
    fn transcript_overlay_preserves_manual_scroll_position() {
        let mut overlay = TranscriptOverlay::new(
            (0..20)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");

        overlay.view.scroll_offset = 0;

        overlay.insert_cell(Arc::new(TestCell {
            lines: vec!["tail".into()],
        }));

        assert_eq!(overlay.view.scroll_offset, 0);
    }

    #[test]
    fn static_overlay_snapshot_basic() {
        // Prepare a static overlay with a few lines and a title
        let mut overlay = StaticOverlay::with_title(
            vec!["one".into(), "two".into(), "three".into()],
            "S T A T I C".to_string(),
        );
        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    /// Render transcript overlay and return visible line numbers (`line-NN`) in order.
    fn transcript_line_numbers(overlay: &mut TranscriptOverlay, area: Rect) -> Vec<usize> {
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let content_area = overlay.view.content_area(top);

        let mut nums = Vec::new();
        for y in content_area.y..content_area.bottom() {
            let mut line = String::new();
            for x in content_area.x..content_area.right() {
                line.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            if let Some(n) = line
                .split_whitespace()
                .find_map(|w| w.strip_prefix("line-"))
                .and_then(|s| s.parse().ok())
            {
                nums.push(n);
            }
        }
        nums
    }

    #[test]
    fn transcript_overlay_paging_is_continuous_and_round_trips() {
        let mut overlay = TranscriptOverlay::new(
            (0..50)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line-{i:02}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let area = Rect::new(0, 0, 40, 15);

        // Prime layout so last_content_height is populated and paging uses the real content height.
        let mut buf = Buffer::empty(area);
        overlay.view.scroll_offset = 0;
        overlay.render(area, &mut buf);
        let page_height = overlay.view.page_height(area);

        // Scenario 1: starting from the top, PageDown should show the next page of content.
        overlay.view.scroll_offset = 0;
        let page1 = transcript_line_numbers(&mut overlay, area);
        let page1_len = page1.len();
        let expected_page1: Vec<usize> = (0..page1_len).collect();
        assert_eq!(
            page1, expected_page1,
            "first page should start at line-00 and show a full page of content"
        );

        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let page2 = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            page2.len(),
            page1_len,
            "second page should have the same number of visible lines as the first page"
        );
        let expected_page2_first = *page1.last().unwrap() + 1;
        assert_eq!(
            page2[0], expected_page2_first,
            "second page after PageDown should immediately follow the first page"
        );

        // Scenario 2: from an interior offset (start=3), PageDown then PageUp should round-trip.
        let interior_offset = 3usize;
        overlay.view.scroll_offset = interior_offset;
        let before = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let _ = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_sub(page_height);
        let after = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            before, after,
            "PageDown+PageUp from interior offset ({interior_offset}) should round-trip"
        );

        // Scenario 3: from the top of the second page, PageUp then PageDown should round-trip.
        overlay.view.scroll_offset = page_height;
        let before2 = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_sub(page_height);
        let _ = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let after2 = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            before2, after2,
            "PageUp+PageDown from the top of the second page should round-trip"
        );
    }

    #[test]
    fn static_overlay_wraps_long_lines() {
        let mut overlay = StaticOverlay::with_title(
            vec!["a very long line that should wrap when rendered within a narrow pager overlay width".into()],
            "S T A T I C".to_string(),
        );
        let mut term = Terminal::new(TestBackend::new(24, 8)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    #[test]
    fn pager_view_content_height_counts_renderables() {
        let pv = PagerView::new(
            vec![paragraph_block("a", 2), paragraph_block("b", 3)],
            "T".to_string(),
            0,
        );

        assert_eq!(pv.content_height(80), 5);
    }

    #[test]
    fn pager_view_ensure_chunk_visible_scrolls_down_when_needed() {
        let mut pv = PagerView::new(
            vec![
                paragraph_block("a", 1),
                paragraph_block("b", 3),
                paragraph_block("c", 3),
            ],
            "T".to_string(),
            0,
        );
        let area = Rect::new(0, 0, 20, 8);

        pv.scroll_offset = 0;
        let content_area = pv.content_area(area);
        pv.ensure_chunk_visible(2, content_area);

        let mut buf = Buffer::empty(area);
        pv.render(area, &mut buf);
        let rendered = buffer_to_text(&buf, area);

        assert!(
            rendered.contains("c0"),
            "expected chunk top in view: {rendered:?}"
        );
        assert!(
            rendered.contains("c1"),
            "expected chunk middle in view: {rendered:?}"
        );
        assert!(
            rendered.contains("c2"),
            "expected chunk bottom in view: {rendered:?}"
        );
    }

    #[test]
    fn pager_view_ensure_chunk_visible_scrolls_up_when_needed() {
        let mut pv = PagerView::new(
            vec![
                paragraph_block("a", 2),
                paragraph_block("b", 3),
                paragraph_block("c", 3),
            ],
            "T".to_string(),
            0,
        );
        let area = Rect::new(0, 0, 20, 3);

        pv.scroll_offset = 6;
        pv.ensure_chunk_visible(0, area);

        assert_eq!(pv.scroll_offset, 0);
    }

    #[test]
    fn pager_view_is_scrolled_to_bottom_accounts_for_wrapped_height() {
        let mut pv = PagerView::new(vec![paragraph_block("a", 10)], "T".to_string(), 0);
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);

        pv.render(area, &mut buf);

        assert!(
            !pv.is_scrolled_to_bottom(),
            "expected view to report not at bottom when offset < max"
        );

        pv.scroll_offset = usize::MAX;
        pv.render(area, &mut buf);

        assert!(
            pv.is_scrolled_to_bottom(),
            "expected view to report at bottom after scrolling to end"
        );
    }
}
