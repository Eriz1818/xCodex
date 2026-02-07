pub(crate) struct ThemeSelectorOverlay {
    app_event_tx: AppEventSender,
    config: codex_core::config::Config,
    edit_variant: codex_core::themes::ThemeVariant,
    theme_entries: Vec<ThemeEntry>,
    selected_idx: usize,
    scroll_top: usize,
    search_query: String,
    follow_selection: bool,
    last_previewed: Option<String>,
    mode: ThemeSelectorMode,
    editor: Option<ThemeInlineEditor>,
    applied: bool,
    is_done: bool,
    frame_requester: Option<crate::tui::FrameRequester>,
    picker_mouse_mode: bool,
    restore_mouse_capture_enabled: Option<bool>,
    restore_alternate_scroll_enabled: Option<bool>,
    last_selector_area: Option<Rect>,
    last_preview_area: Option<Rect>,
    last_editor_area: Option<Rect>,
    preview_keys_overlay_open: bool,
    sample_code_tab_idx: usize,
}

#[derive(Clone, Debug)]
struct ThemeEntry {
    name: String,
    variant: codex_core::themes::ThemeVariant,
}

#[derive(Clone, Copy, Debug)]
struct SampleCodeTab {
    label: &'static str,
    fence_lang: &'static str,
    code: &'static str,
}

const SAMPLE_CODE_TABS: &[SampleCodeTab] = &[
    SampleCodeTab {
        label: "Rust",
        fence_lang: "rust",
        code: "use std::collections::HashMap;\n\n// Note: appease the borrow checker with snacks.\n\n/// Returns a greeting (no unsafe rituals required).\nfn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n\nfn main() {\n    let mut counts: HashMap<&str, usize> = HashMap::new();\n\n    for i in 0..=16 {\n        let msg = if i % 15 == 0 {\n            greet(\"CaptainBorrowChecker\")\n        } else if i % 3 == 0 {\n            \"Borrow\".to_string()\n        } else if i % 5 == 0 {\n            \"Checker\".to_string()\n        } else {\n            format!(\"{i}\")\n        };\n        *counts.entry(\"lines\").or_insert(0) += 1;\n        println!(\"{msg}\");\n    }\n}\n",
    },
    SampleCodeTab {
        label: "Python",
        fence_lang: "python",
        code: "from __future__ import annotations\n\nfrom dataclasses import dataclass\n\n# Note: type hints keep the gremlins calm.\n\n@dataclass(frozen=True)\nclass Item:\n    name: str\n    count: int = 0\n\ndef greet(name: str) -> str:\n    return f\"Hello, {name}!\"\n\ndef main() -> None:\n    items = [Item(\"borrow\"), Item(\"checker\")]\n    for i in range(0, 17):\n        if i % 15 == 0:\n            msg = greet(\"CaptainBorrowChecker\")\n        elif i % 3 == 0:\n            msg = \"Borrow\"\n        elif i % 5 == 0:\n            msg = \"Checker\"\n        else:\n            msg = str(i)\n        print(f\"{i:02d}: {msg}\")\n\nif __name__ == \"__main__\":\n    main()\n",
    },
    SampleCodeTab {
        label: "JavaScript",
        fence_lang: "javascript",
        code: "/** @param {string} name */\nfunction greet(name) {\n  // Note: this function is 99% vibes, 1% types.\n  return `Hello, ${name}!`;\n}\n\nfunction main() {\n  const counts = new Map();\n  for (let i = 0; i <= 16; i += 1) {\n    const msg =\n      i % 15 === 0\n        ? greet(\"CaptainBorrowChecker\")\n        : i % 3 === 0\n          ? \"Borrow\"\n          : i % 5 === 0\n            ? \"Checker\"\n            : String(i);\n\n    counts.set(\"lines\", (counts.get(\"lines\") ?? 0) + 1);\n    console.log(`${i.toString().padStart(2, \"0\")}: ${msg}`);\n  }\n}\n\nmain();\n",
    },
    SampleCodeTab {
        label: "TypeScript",
        fence_lang: "typescript",
        code: "type Counts = Map<string, number>;\n\nfunction greet(name: string): string {\n  // Note: strict mode demands a tribute.\n  return `Hello, ${name}!`;\n}\n\nfunction inc(map: Counts, key: string): void {\n  map.set(key, (map.get(key) ?? 0) + 1);\n}\n\nfunction main(): void {\n  const counts: Counts = new Map();\n  for (let i = 0; i <= 16; i += 1) {\n    let msg: string;\n    if (i % 15 === 0) {\n      msg = greet(\"CaptainBorrowChecker\");\n    } else if (i % 3 === 0) {\n      msg = \"Borrow\";\n    } else if (i % 5 === 0) {\n      msg = \"Checker\";\n    } else {\n      msg = String(i);\n    }\n\n    inc(counts, \"lines\");\n    console.log(`${i.toString().padStart(2, \"0\")}: ${msg}`);\n  }\n}\n\nmain();\n",
    },
];

impl ThemeSelectorOverlay {
    fn update_picker_mouse_mode(&mut self, tui: &mut tui::Tui) {
        let should_enable =
            matches!(self.mode, ThemeSelectorMode::Picker { .. }) || self.editor.is_some();
        if should_enable == self.picker_mouse_mode {
            return;
        }
        if should_enable {
            self.restore_mouse_capture_enabled = Some(tui.mouse_capture_enabled());
            self.restore_alternate_scroll_enabled = Some(tui.alternate_scroll_enabled());
        }
        self.picker_mouse_mode = should_enable;
        tui.set_mouse_capture_enabled(should_enable);
        tui.set_alternate_scroll_enabled(!should_enable);
    }

    fn restore_picker_mouse_mode(&mut self, tui: &mut tui::Tui) {
        if !self.picker_mouse_mode {
            return;
        }
        self.picker_mouse_mode = false;
        let mouse_capture = self.restore_mouse_capture_enabled.take().unwrap_or(true);
        let alternate_scroll = self.restore_alternate_scroll_enabled.take().unwrap_or(false);
        tui.set_mouse_capture_enabled(mouse_capture);
        tui.set_alternate_scroll_enabled(alternate_scroll);
    }

    fn new(
        app_event_tx: AppEventSender,
        config: codex_core::config::Config,
        terminal_bg: Option<(u8, u8, u8)>,
    ) -> Self {
        use codex_core::themes::ThemeCatalog;
        use codex_core::themes::ThemeVariant;

        let edit_variant = crate::theme::active_variant(&config, terminal_bg);
        let current_theme = match edit_variant {
            ThemeVariant::Light => config.xcodex.themes.light.as_deref(),
            ThemeVariant::Dark => config.xcodex.themes.dark.as_deref(),
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

        crate::render::highlight::set_syntax_highlighting_enabled(
            config.tui_transcript_syntax_highlight,
        );

        Self {
            app_event_tx,
            config,
            edit_variant,
            theme_entries,
            selected_idx,
            scroll_top: 0,
            search_query: String::new(),
            follow_selection: true,
            last_previewed: None,
            mode: ThemeSelectorMode::Picker { preview_scroll: 0 },
            editor: None,
            applied: false,
            is_done: false,
            frame_requester: None,
            picker_mouse_mode: false,
            restore_mouse_capture_enabled: None,
            restore_alternate_scroll_enabled: None,
            last_selector_area: None,
            last_preview_area: None,
            last_editor_area: None,
            preview_keys_overlay_open: false,
            sample_code_tab_idx: 0,
        }
    }

    fn render_preview_keys_overlay(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                cell.set_style(cell.style().add_modifier(Modifier::DIM));
            }
        }

        let overlay_area = area.inset(Insets::vh(1, 2));
        if overlay_area.is_empty() {
            return;
        }

        Clear.render(overlay_area, buf);

        let diff_highlight = if self.config.tui_transcript_diff_highlight {
            "on"
        } else {
            "off"
        };
        let prompt_highlight = if self.config.tui_transcript_user_prompt_highlight {
            "on"
        } else {
            "off"
        };
        let syntax_highlight = if self.config.tui_transcript_syntax_highlight {
            "on"
        } else {
            "off"
        };
        let minimal_composer = if self.config.tui_minimal_composer {
            "on"
        } else {
            "off"
        };

        let lines: Vec<Line<'static>> = vec![
            vec![
                Span::from(KEY_CTRL_U),
                "/".into(),
                Span::from(KEY_CTRL_D),
                "  ".into(),
                "scroll preview".into(),
            ]
            .into(),
            vec![
                Span::from(KEY_CTRL_G),
                "  ".into(),
                format!("toggle diff highlight ({diff_highlight})").into(),
            ]
            .into(),
            vec![
                Span::from(KEY_CTRL_H),
                "  ".into(),
                format!("toggle syntax highlight ({syntax_highlight})").into(),
            ]
            .into(),
            vec![
                Span::from(KEY_CTRL_P),
                "  ".into(),
                format!("toggle history prompt highlight ({prompt_highlight})").into(),
            ]
            .into(),
            vec![
                Span::from(KEY_CTRL_M),
                "  ".into(),
                format!("toggle minimal composer ({minimal_composer})").into(),
            ]
            .into(),
            vec![Span::from(KEY_CTRL_T), "  ".into(), "edit theme".into()].into(),
            Line::from(""),
            vec![Span::from(KEY_ESC), "  ".into(), "close".into()].into(),
        ];

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title("Preview keys")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(crate::theme::accent_style())
                    .style(crate::theme::transcript_style()),
            )
            .render(overlay_area, buf);
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
            ThemeVariant::Light => self.config.xcodex.themes.light.as_deref(),
            ThemeVariant::Dark => self.config.xcodex.themes.dark.as_deref(),
        }
        .unwrap_or("default");

        if let Some(idx) = self
            .theme_entries
            .iter()
            .position(|entry| entry.name == desired_theme)
        {
            self.selected_idx = idx;
            self.ensure_preview_applied();
            self.follow_selection = true;
        }
    }

    fn ensure_preview_applied(&mut self) {
        let theme = self.selected_theme().to_string();
        if self.last_previewed.as_deref() == Some(theme.as_str()) {
            return;
        }
        self.last_previewed = Some(theme.clone());
        self.app_event_tx
            .send(AppEvent::PreviewTheme { theme: theme.clone() });
        if let Some(editor) = self.editor.as_mut()
            && editor.base_theme_name != theme
        {
            use codex_core::themes::ThemeCatalog;

            let base_theme = ThemeCatalog::load(&self.config)
                .ok()
                .and_then(|catalog| catalog.get(theme.as_str()).cloned())
                .unwrap_or_else(ThemeCatalog::built_in_default);
            editor.replace_theme(self.edit_variant, theme, base_theme);
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        if self.search_query.is_empty() {
            return (0..self.theme_entries.len()).collect();
        }

        let needle = self.search_query.to_ascii_lowercase();
        self.theme_entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                if entry.name.to_ascii_lowercase().contains(&needle) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    fn selected_filtered_pos(&self, indices: &[usize]) -> usize {
        indices
            .iter()
            .position(|idx| *idx == self.selected_idx)
            .unwrap_or(0)
    }

    fn move_selection(&mut self, delta: isize) {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            return;
        }
        let len = indices.len() as isize;
        let pos = self.selected_filtered_pos(&indices) as isize;
        let next_pos = (pos + delta).rem_euclid(len) as usize;
        self.selected_idx = indices[next_pos];
        self.follow_selection = true;
        self.ensure_preview_applied();
    }

    fn open_editor(&mut self) {
        use codex_core::themes::ThemeCatalog;

        let base_theme_name = self.selected_theme().to_string();
        let base_theme = ThemeCatalog::load(&self.config)
            .ok()
            .and_then(|catalog| catalog.get(base_theme_name.as_str()).cloned())
            .unwrap_or_else(ThemeCatalog::built_in_default);

        self.editor = Some(ThemeInlineEditor::new(
            self.app_event_tx.clone(),
            self.edit_variant,
            base_theme_name,
            base_theme,
        ));

        // Ensure we re-apply the selected theme after closing edit mode.
        self.last_previewed = None;
    }

    fn ensure_visible(&mut self, list_height: u16, indices: &[usize]) {
        let visible = usize::from(list_height.max(1)).min(indices.len());
        if visible == 0 {
            self.scroll_top = 0;
            return;
        }
        let selected_pos = self.selected_filtered_pos(indices);
        if selected_pos < self.scroll_top {
            self.scroll_top = selected_pos;
        } else if selected_pos >= self.scroll_top + visible {
            self.scroll_top = selected_pos + 1 - visible;
        }
    }

    fn scroll_list_by(&mut self, delta_rows: isize, list_height: u16) {
        let indices = self.filtered_indices();
        let visible = usize::from(list_height.max(1)).min(indices.len());
        if visible == 0 {
            self.scroll_top = 0;
            return;
        }
        let max_scroll = indices.len().saturating_sub(visible);
        let next = (self.scroll_top as isize + delta_rows).clamp(0, max_scroll as isize) as usize;
        self.scroll_top = next;
        self.follow_selection = false;
    }

    fn handle_type_to_search_char(&mut self, ch: char) {
        if ch.is_ascii_control() || self.search_query.len() >= 64 {
            return;
        }
        self.search_query.push(ch);
        self.scroll_top = 0;

        let indices = self.filtered_indices();
        if let Some(first) = indices.first().copied() {
            self.selected_idx = first;
            self.follow_selection = true;
            self.ensure_preview_applied();
        }
    }

    fn handle_search_backspace(&mut self) {
        self.search_query.pop();
        if self.search_query.is_empty() {
            self.follow_selection = true;
        }
    }

    fn handle_search_escape(&mut self) -> bool {
        if self.search_query.is_empty() {
            return false;
        }
        self.search_query.clear();
        self.follow_selection = true;
        true
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

    fn render_preview(&self, area: Rect, buf: &mut Buffer, scroll: u16) -> u16 {
        if area.is_empty() {
            return 0;
        }

        crate::render::highlight::set_syntax_highlighting_enabled(
            self.config.tui_transcript_syntax_highlight,
        );

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_style(crate::theme::transcript_style());
            }
        }

        let Some(frame_requester) = self.frame_requester.as_ref() else {
            return 0;
        };

        let diff_highlight = self.config.tui_transcript_diff_highlight;
        let _diff_add = if diff_highlight {
            crate::theme::diff_add_highlight_style()
        } else {
            crate::theme::diff_add_text_style()
        };
        let _diff_del = if diff_highlight {
            crate::theme::diff_del_highlight_style()
        } else {
            crate::theme::diff_del_text_style()
        };
        let _diff_hunk = if diff_highlight {
            crate::theme::diff_hunk_highlight_style()
        } else {
            crate::theme::diff_hunk_text_style()
        };

        fn buffer_to_lines(buf: &Buffer) -> Vec<Line<'static>> {
            let mut out: Vec<Line<'static>> = Vec::with_capacity(buf.area.height as usize);
            for y in 0..buf.area.height {
                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut run_style: Option<Style> = None;
                let mut run = String::new();

                for x in 0..buf.area.width {
                    let cell = &buf[(x, y)];
                    let symbol = cell.symbol();
                    let style = cell.style();
                    let symbol = if symbol.is_empty() { " " } else { symbol };

                    if run_style != Some(style) && !run.is_empty() {
                        if let Some(prev_style) = run_style {
                            spans.push(Span::styled(std::mem::take(&mut run), prev_style));
                        } else {
                            spans.push(Span::from(std::mem::take(&mut run)));
                        }
                    }

                    if run.is_empty() || run_style != Some(style) {
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

        fn render_cell_to_lines(
            cell: &dyn HistoryCell,
            width: u16,
            base_style: Style,
        ) -> Vec<Line<'static>> {
            let width = width.max(1);
            let height = cell.desired_height(width).max(1);
            let cell_style = base_style;
            let mut cell_buf = Buffer::empty(Rect::new(0, 0, width, height));
            for y in 0..cell_buf.area.height {
                for x in 0..cell_buf.area.width {
                    cell_buf[(x, y)].set_symbol(" ");
                    cell_buf[(x, y)].set_style(cell_style);
                }
            }
            Paragraph::new(Text::from(cell.transcript_lines(width)))
                .style(cell_style)
                .render(*cell_buf.area(), &mut cell_buf);
            for y in 0..cell_buf.area.height {
                for x in 0..cell_buf.area.width {
                    let cell = &mut cell_buf[(x, y)];
                    if let Some(bg) = base_style.bg {
                        cell.set_style(cell.style().bg(bg));
                    }
                }
            }
            buffer_to_lines(&cell_buf)
        }

        let session_event = codex_core::protocol::SessionConfiguredEvent {
            session_id: ThreadId::new(),
            forked_from_id: None,
            thread_name: None,
            model: "gpt-5.2 medium".to_string(),
            model_provider_id: "openai".to_string(),
            approval_policy: self.config.approval_policy.value(),
            sandbox_policy: self.config.sandbox_policy.get().clone(),
            cwd: PathBuf::from("~/Dev/Pyfun/skynet/xcodex"),
            reasoning_effort: None,
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: None,
            rollout_path: Some(PathBuf::from("/tmp/theme-preview.jsonl")),
        };

        let mut preview_config = self.config.clone();
        preview_config.cwd = PathBuf::from("~/Dev/Pyfun/skynet/xcodex");

        let mut session_help_lines: Vec<Line<'static>> = vec![
            "  To get started, describe a task or try one of these commands:"
                .dim()
                .into(),
            Line::from(""),
        ];
        let mut command_lines =
            crate::xcodex_plugins::history_cell::session_first_event_command_lines(
                crate::theme::transcript_style(),
            );
        command_lines.truncate(2);
        session_help_lines.extend(command_lines);

        let collaboration_mode = codex_protocol::config_types::CollaborationMode {
            mode: codex_protocol::config_types::ModeKind::Default,
            settings: codex_protocol::config_types::Settings {
                model: "gpt-5.2 medium".to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        };
        let session_info = crate::xcodex_plugins::history_cell::new_session_info_with_help_lines(
            &preview_config,
            "gpt-5.2 medium",
            session_event,
            session_help_lines,
            false,
            collaboration_mode,
        );

        let mut lines: Vec<Line<'static>> = Vec::new();

        // (1) Session info (scrollable transcript content).
        lines.extend(session_info.display_lines(area.width));
        lines.push(Line::from(""));

        // (2) Quick examples block: diff + status + link + warning + error.
        lines.push(vec!["• ".dim(), "Quick examples".bold()].into());
        lines.extend([
            Line::from(vec![
                "  └ ".dim(),
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
                "  └ ".dim(),
                "link: ".dim(),
                Span::from("https://example.com")
                    .set_style(crate::theme::link_style().underlined()),
            ]),
        ]);
        lines.push(Line::from(""));

        {
            let deprecation = crate::history_cell::new_deprecation_notice(
                "Heads up: `/dance` is deprecated.".to_string(),
                Some(
                    "Use `/boogie` instead (or just type “boogie” loudly into the composer)."
                        .to_string(),
                ),
            );
            lines.extend(deprecation.display_lines(area.width));
            lines.push(Line::from(""));

            lines.extend(
                crate::history_cell::new_error_event(
                    "Error: `/boogie` triggered a snapstorm; composer auto-muted.".to_string(),
                )
                .display_lines(area.width),
            );
            lines.push(Line::from(""));

            let sample = SAMPLE_CODE_TABS
                .get(self.sample_code_tab_idx % SAMPLE_CODE_TABS.len())
                .copied()
                .unwrap_or(SAMPLE_CODE_TABS[0]);

            let mut changes = HashMap::new();
            let (path, unified_diff) = sample_diff_for_tab(sample);
            changes.insert(
                path,
                FileChange::Update {
                    unified_diff,
                    move_path: None,
                },
            );

            let patch = crate::history_cell::new_patch_event(
                changes,
                self.config.cwd.as_path(),
                self.config.tui_transcript_diff_highlight,
            );
            lines.extend(patch.display_lines(area.width));
            lines.push(Line::from(""));

            let mut sample_lines: Vec<Line<'static>> = Vec::new();
            sample_lines.push(sample_code_tabs_line(sample));
            sample_lines.push(Line::from("").style(crate::theme::transcript_style()));

            let wrap_width = usize::from(area.width).saturating_sub(2);
            let lang = sample.fence_lang;
            let code = sample.code;
            let markdown = format!("```{lang}\n{code}```\n");
            let rendered =
                crate::markdown_render::render_markdown_text_with_width(&markdown, Some(wrap_width));
            sample_lines.extend(rendered.lines);

            let sample_cell = crate::history_cell::AgentMessageCell::new(sample_lines, true);
            lines.extend(sample_cell.display_lines(area.width));
            lines.push(Line::from(""));

            let patch_failed = crate::history_cell::new_patch_apply_failure(
                "error: patch failed: src/main.rs:42\n".to_string(),
            );
            lines.extend(patch_failed.display_lines(area.width));
            lines.push(Line::from(""));

            let mut tool_call = crate::history_cell::new_active_mcp_tool_call(
                "preview-mcp-1".to_string(),
                codex_core::protocol::McpInvocation {
                    server: "filesystem".to_string(),
                    tool: "read_file".to_string(),
                    arguments: Some(json!({"path": "README.md"})),
                },
                self.config.animations,
            );
            let image_cell = tool_call.complete(
                Duration::from_millis(312),
                Ok(codex_protocol::mcp::CallToolResult {
                    content: vec![serde_json::json!({
                        "type": "text",
                        "text": "Found 1 file: README.md"
                    })],
                    is_error: Some(false),
                    structured_content: None,
                    meta: None,
                }),
            );
            lines.extend(tool_call.display_lines(area.width));
            if let Some(image_cell) = image_cell {
                lines.extend(image_cell.display_lines(area.width));
            }
            lines.push(Line::from(""));

            let approval_decision = crate::history_cell::new_approval_decision_cell(
                vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "cargo test -p codex-tui".to_string(),
                ],
                codex_core::protocol::ReviewDecision::Approved,
            );
            lines.extend(approval_decision.display_lines(area.width));
            lines.push(Line::from(""));

            let interaction = crate::history_cell::new_unified_exec_interaction(
                Some("python3 -i".to_string()),
                "print('hello from the other (side) background terminal')\n".to_string(),
            );
            lines.extend(interaction.display_lines(area.width));
            lines.push(Line::from(""));

            let processes = crate::xcodex_plugins::history_cell::new_unified_exec_processes_output(
                vec![
                    crate::xcodex_plugins::history_cell::BackgroundActivityEntry::new(
                        "term-17".to_string(),
                        "python3 -i\n>>>".to_string(),
                    ),
                    crate::xcodex_plugins::history_cell::BackgroundActivityEntry::new(
                        "term-23".to_string(),
                        "rg -n \"HistoryCell\" -S codex-rs/tui/src/history_cell.rs".to_string(),
                    ),
                ],
                vec![crate::xcodex_plugins::history_cell::BackgroundActivityEntry::new(
                    "hook-1".to_string(),
                    "pre-commit: cargo fmt && cargo test -p codex-tui".to_string(),
                )],
            );
            lines.extend(processes.display_lines(area.width));
        }

        lines.push(Line::from(""));

        let user_prompt = "Give me the highlight reel of whatever we just did, and toss in an approval modal for extra drama.";

        let user_cell = crate::history_cell::new_user_prompt_preview(
            user_prompt.to_string(),
            self.config.tui_transcript_user_prompt_highlight,
        );
        let mut user_prompt_style = user_message_style().patch(crate::theme::composer_style());
        if self.config.tui_transcript_user_prompt_highlight {
            user_prompt_style = user_prompt_style.patch(crate::theme::user_prompt_highlight_style());
        }
        lines.extend(render_cell_to_lines(
            &user_cell,
            area.width,
            user_prompt_style,
        ));

        lines.push(Line::from(""));

        // (3) Plan update sample.
        let plan_update = crate::history_cell::new_plan_update(UpdatePlanArgs {
            explanation: Some(
                "Make this whole little playthrough feel seamless, and don’t let any “mystery styling goblins” sneak in."
                    .to_string(),
            ),
            plan: vec![
                PlanItemArg {
                    step: "Add a “look ma, it works” snack-sized examples block".to_string(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Summon the approval modal inside the transcript (dramatic lighting optional)."
                        .to_string(),
                    status: StepStatus::InProgress,
                },
                PlanItemArg {
                    step: "Finish with a tidy “we survived” summary section".to_string(),
                    status: StepStatus::Pending,
                },
            ],
        });
        lines.extend(plan_update.display_lines(area.width));

        lines.push(Line::from(""));

        let mut tool_call = crate::exec_cell::new_active_exec_command(
            "preview-shell-1".to_string(),
            vec![
                "bash".to_string(),
                "-lc".to_string(),
                "rg -n \"Theme Preview\" codex-rs/tui/src/pager_overlay.rs".to_string(),
            ],
            Vec::new(),
            codex_core::protocol::ExecCommandSource::Agent,
            None,
            false,
        );
        tool_call.complete_call(
            "preview-shell-1",
            crate::exec_cell::CommandOutput {
                exit_code: 0,
                aggregated_output:
                    "910:                    Paragraph::new(Line::from(\"Theme Preview\"))\n"
                        .to_string(),
                formatted_output: "910: Paragraph::new(Line::from(\"Theme Preview\"))".to_string(),
            },
            std::time::Duration::from_millis(742),
        );
        lines.extend(tool_call.display_lines(area.width));

        lines.push(Line::from(""));

        let thought = crate::history_cell::new_reasoning_summary_block(
            "**Popcorn-powered planning**\n\nI’ll render this `preview` with real cells so the theme does the styling.\nIf it looks odd, I blame the popcorn, not the `palette`.".to_string(),
            false,
        );
        lines.extend(thought.display_lines(area.width));

        lines.push(Line::from(""));

        // (4) Approval required (render the real approval overlay into transcript).
        let approval = crate::bottom_pane::ApprovalOverlay::new(
            crate::bottom_pane::ApprovalRequest::Exec {
                id: "preview-install".to_string(),
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "cd /Users/MD-Dyson/Dev/Pyfun/skynet/xcodex/codex-rs && just xcodex-install"
                        .to_string(),
                ],
                reason: None,
                proposed_execpolicy_amendment: None,
            },
            self.app_event_tx.clone(),
            codex_core::features::Features::with_defaults(),
        );
        let width = area.width.max(1);
        let height = approval.desired_height(width);
        let mut approval_buf = Buffer::empty(Rect::new(0, 0, width, height));
        let base_style = user_message_style().patch(crate::theme::composer_style());
        for y in 0..approval_buf.area.height {
            for x in 0..approval_buf.area.width {
                approval_buf[(x, y)].set_symbol(" ");
                approval_buf[(x, y)].set_style(base_style);
            }
        }
        approval.render(*approval_buf.area(), &mut approval_buf);
        for y in 0..approval_buf.area.height {
            for x in 0..approval_buf.area.width {
                let cell = &mut approval_buf[(x, y)];
                if let Some(bg) = base_style.bg {
                    cell.set_style(cell.style().bg(bg));
                }
            }
        }
        lines.push(Line::from(vec![
            Span::from("Approval required:").set_style(crate::theme::accent_style()),
        ]));
        lines.extend(buffer_to_lines(&approval_buf));
        lines.push(Line::from(""));

        // (5) Final separator + summary header.
        lines.extend(
            crate::xcodex_plugins::history_cell::XcodexFinalMessageSeparator::new(
                Some(12),
                true,
                crate::xtreme::xtreme_ui_enabled(&self.config),
                None,
                None,
            )
            .display_lines(area.width),
        );

        lines.push(Line::from(""));

        let assistant = AgentMessageCell::new(
            {
                let mut rendered: Vec<Line<'static>> = Vec::new();
                crate::markdown::append_markdown(
                    "I crammed a whole chaotic mini-adventure into three lines:\n\
                    \n\
                    - `plan`\n\
                    - command\n\
                    - a dramatic “approved” moment\n\
                    \n\
                    There’s exactly one “typing area” cameo at the bottom—no duplicate keyboard gremlins rehearsing `mid-transcript`.\n\
                    Everything above it is just the story: one prompt, one `brainwave`, one button-press, and one smug little summary.",
                    None,
                    &mut rendered,
                );
                rendered
            },
            true,
        );
        lines.extend(assistant.display_lines(area.width));

        // (6) Bottom pane snapshot (scrollable transcript content).
        {
            let mut bottom_pane = BottomPane::new(BottomPaneParams {
                app_event_tx: self.app_event_tx.clone(),
                frame_requester: frame_requester.clone(),
                has_input_focus: true,
                enhanced_keys_supported: false,
                placeholder_text: "Ask xcodex to do anything".to_string(),
                disable_paste_burst: false,
                minimal_composer_borders: self.config.tui_minimal_composer,
                xtreme_ui_enabled: crate::xtreme::xtreme_ui_enabled(&self.config),
                animations_enabled: self.config.animations,
                skills: None,
            });
            bottom_pane.set_slash_popup_max_rows(3);
            bottom_pane.insert_str("/mo");
            bottom_pane.ensure_status_indicator();
            bottom_pane.update_status("Working".to_string(), Some("Theme preview".to_string()));
            bottom_pane.set_context_window(Some(100), Some(0));
            bottom_pane.set_status_bar_git_options(true, true);
            bottom_pane.set_status_bar_git_context(
                Some("feat/skynet-themes".to_string()),
                Some("~/Dev/Pyfun/skynet/xcodex".to_string()),
            );

            let width = area.width.max(1);
            let height = bottom_pane.desired_height(width).max(1);
            let mut bottom_buf = Buffer::empty(Rect::new(0, 0, width, height));
            bottom_pane.render(*bottom_buf.area(), &mut bottom_buf);

            // Render the footer in a separate pane so the snapshot can show:
            // composer + slash popup + status bar.
            let mut footer_pane = BottomPane::new(BottomPaneParams {
                app_event_tx: self.app_event_tx.clone(),
                frame_requester: frame_requester.clone(),
                has_input_focus: true,
                enhanced_keys_supported: false,
                placeholder_text: "Ask xcodex to do anything".to_string(),
                disable_paste_burst: false,
                minimal_composer_borders: self.config.tui_minimal_composer,
                xtreme_ui_enabled: crate::xtreme::xtreme_ui_enabled(&self.config),
                animations_enabled: self.config.animations,
                skills: None,
            });
            footer_pane.set_slash_popup_max_rows(3);
            footer_pane.ensure_status_indicator();
            footer_pane.update_status("Working".to_string(), Some("Theme preview".to_string()));
            footer_pane.set_context_window(Some(100), Some(0));
            footer_pane.set_status_bar_git_options(true, true);
            footer_pane.set_status_bar_git_context(
                Some("feat/themes".to_string()),
                Some("~/Dev/Pyfun/skynet/xcodex".to_string()),
            );

            let footer_height = footer_pane.desired_height(width).max(1);
            let mut footer_buf = Buffer::empty(Rect::new(0, 0, width, footer_height));
            footer_pane.render(*footer_buf.area(), &mut footer_buf);

            let rows_to_copy = 1u16;
            let mut combined_buf = Buffer::empty(Rect::new(
                0,
                0,
                width,
                bottom_buf.area.height.saturating_add(rows_to_copy),
            ));
            for y in 0..combined_buf.area.height {
                for x in 0..combined_buf.area.width {
                    combined_buf[(x, y)].set_symbol(" ");
                    combined_buf[(x, y)].set_style(crate::theme::transcript_style());
                }
            }

            for y in 0..bottom_buf.area.height {
                for x in 0..width {
                    combined_buf[(x, y)] = bottom_buf[(x, y)].clone();
                }
            }

            if footer_buf.area.height >= rows_to_copy {
                let src_y = footer_buf.area.height - rows_to_copy;
                let dst_y = combined_buf.area.height - rows_to_copy;
                for x in 0..width {
                    combined_buf[(x, dst_y)] = footer_buf[(x, src_y)].clone();
                }
            }

            lines.push(Line::from(""));
            lines.extend(buffer_to_lines(&combined_buf));
        }

        let content_area = area;
        let visible_rows = content_area.height as usize;
        let max_scroll =
            u16::try_from(lines.len().saturating_sub(visible_rows)).unwrap_or(u16::MAX);
        let scroll = scroll.min(max_scroll);

        Paragraph::new(Text::from(lines))
            .scroll((scroll, 0))
            .render_ref(content_area, buf);

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

        let indices = self.filtered_indices();

        let search_height = if area.height >= 2 { 1 } else { 0 };
        let search_area = Rect::new(area.x, area.y, area.width, search_height);
        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(search_height),
            area.width,
            area.height.saturating_sub(search_height),
        );

        if search_height == 1 {
            let query_span: Span<'static> = if self.search_query.is_empty() {
                "(type to search)".dim()
            } else {
                Span::from(self.search_query.clone()).set_style(crate::theme::accent_style())
            };
            let search_line: Line<'static> = vec![
                "Search: ".dim(),
                query_span,
                "  ".into(),
                format!("({} themes)", indices.len()).dim(),
            ]
            .into();
            Paragraph::new(search_line)
                .style(crate::theme::composer_style())
                .render_ref(search_area, buf);
        }

        if self.follow_selection {
            self.ensure_visible(list_area.height, &indices);
        }

        let visible = usize::from(list_area.height.max(1)).min(indices.len());
        let start = self.scroll_top.min(indices.len().saturating_sub(1));
        let end = (start + visible).min(indices.len());

        for (row, idx) in (start..end).enumerate() {
            let y = list_area.y + row as u16;
            let entry = &self.theme_entries[indices[idx]];
            let variant_label = match entry.variant {
                codex_core::themes::ThemeVariant::Light => "Light",
                codex_core::themes::ThemeVariant::Dark => "Dark",
            };
            let mut line = Line::from(format!("{variant_label}  {}", entry.name));
            if indices[idx] == self.selected_idx {
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
        self.update_picker_mouse_mode(tui);

        if matches!(event, TuiEvent::Draw) {
            return self.handle_draw(tui);
        }

        if let Some(editor) = self.editor.as_mut() {
            let close_editor = matches!(
                &event,
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                })
            ) || matches!(&event, TuiEvent::Key(e) if KEY_CTRL_T.is_press(*e));
            let quit_overlay = matches!(
                &event,
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    kind: KeyEventKind::Press,
                    ..
                })
            );

            let editor_area = self.last_editor_area;
            let consumed = editor.handle_event(&self.config, editor_area, &event);
            if editor.is_done {
                self.applied = true;
                self.is_done = true;
            }
            if consumed {
                tui.frame_requester().schedule_frame();
                return Ok(());
            }

            if close_editor {
                self.editor = None;
                self.ensure_preview_applied();
                tui.frame_requester().schedule_frame();
                return Ok(());
            }
            if quit_overlay {
                self.editor = None;
                self.cancel();
                tui.frame_requester().schedule_frame();
                return Ok(());
            }
        }

        match &mut self.mode {
            ThemeSelectorMode::Picker { preview_scroll } => match event {
                TuiEvent::Key(key_event) => match key_event {
                    e if self.preview_keys_overlay_open && KEY_ESC.is_press(e) => {
                        self.preview_keys_overlay_open = false;
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    KeyEvent {
                        code: KeyCode::Char('?'),
                        ..
                    } if self.preview_keys_overlay_open => {
                        self.preview_keys_overlay_open = false;
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    _ if self.preview_keys_overlay_open => Ok(()),
                    e if KEY_ESC.is_press(e) || KEY_Q.is_press(e) => {
                        if !self.handle_search_escape() {
                            self.cancel();
                            self.restore_picker_mouse_mode(tui);
                        }
                        Ok(())
                    }
                    e if KEY_ENTER.is_press(e) => {
                        self.apply_selection();
                        self.restore_picker_mouse_mode(tui);
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
                    KeyEvent {
                        code: KeyCode::BackTab,
                        ..
                    } => {
                        self.sample_code_tab_idx =
                            self.sample_code_tab_idx.saturating_add(1) % SAMPLE_CODE_TABS.len();
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
                    KeyEvent {
                        code: KeyCode::Backspace,
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => {
                        self.handle_search_backspace();
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
                    e if KEY_CTRL_G.is_press(e) => {
                        let next = !self.config.tui_transcript_diff_highlight;
                        self.config.tui_transcript_diff_highlight = next;
                        self.app_event_tx
                            .send(AppEvent::UpdateTranscriptDiffHighlight(next));
                        self.app_event_tx
                            .send(AppEvent::PersistTranscriptDiffHighlight(next));
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_H.is_press(e) => {
                        let next = !self.config.tui_transcript_syntax_highlight;
                        self.config.tui_transcript_syntax_highlight = next;
                        crate::render::highlight::set_syntax_highlighting_enabled(next);
                        self.app_event_tx
                            .send(AppEvent::UpdateTranscriptSyntaxHighlight(next));
                        self.app_event_tx
                            .send(AppEvent::PersistTranscriptSyntaxHighlight(next));
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_P.is_press(e) => {
                        let next = !self.config.tui_transcript_user_prompt_highlight;
                        self.config.tui_transcript_user_prompt_highlight = next;
                        self.app_event_tx
                            .send(AppEvent::UpdateTranscriptUserPromptHighlight(next));
                        self.app_event_tx
                            .send(AppEvent::PersistTranscriptUserPromptHighlight(next));
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_M.is_press(e) => {
                        let next = !self.config.tui_minimal_composer;
                        self.config.tui_minimal_composer = next;
                        self.app_event_tx
                            .send(AppEvent::UpdateMinimalComposer(next));
                        self.app_event_tx
                            .send(AppEvent::PersistMinimalComposer(next));
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    e if KEY_CTRL_T.is_press(e) => {
                        self.open_editor();
                        self.restore_picker_mouse_mode(tui);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    KeyEvent {
                        code: KeyCode::Char('?'),
                        ..
                    } => {
                        self.preview_keys_overlay_open = true;
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    KeyEvent {
                        code: KeyCode::Char(ch),
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => {
                        self.handle_type_to_search_char(ch);
                        tui.frame_requester().schedule_frame();
                        Ok(())
                    }
                    _ => Ok(()),
                },
                TuiEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column,
                    row,
                    ..
                }) => {
                    if self.preview_keys_overlay_open {
                        return Ok(());
                    }
                    if let Some(selector_area) = self.last_selector_area
                        && column >= selector_area.left()
                        && column < selector_area.right()
                        && row >= selector_area.top()
                        && row < selector_area.bottom()
                    {
                        let list_height = selector_area.height.saturating_sub(1);
                        self.scroll_list_by(-3, list_height);
                    } else if let Some(preview_area) = self.last_preview_area
                        && column >= preview_area.left()
                        && column < preview_area.right()
                        && row >= preview_area.top()
                        && row < preview_area.bottom()
                    {
                        *preview_scroll = preview_scroll.saturating_sub(1);
                    }
                    tui.frame_requester().schedule_frame();
                    Ok(())
                }
                TuiEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column,
                    row,
                    ..
                }) => {
                    if self.preview_keys_overlay_open {
                        return Ok(());
                    }
                    if let Some(selector_area) = self.last_selector_area
                        && column >= selector_area.left()
                        && column < selector_area.right()
                        && row >= selector_area.top()
                        && row < selector_area.bottom()
                    {
                        let list_height = selector_area.height.saturating_sub(1);
                        self.scroll_list_by(3, list_height);
                    } else if let Some(preview_area) = self.last_preview_area
                        && column >= preview_area.left()
                        && column < preview_area.right()
                        && row >= preview_area.top()
                        && row < preview_area.bottom()
                    {
                        *preview_scroll = preview_scroll.saturating_add(1);
                    }
                    tui.frame_requester().schedule_frame();
                    Ok(())
                }
                TuiEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    ..
                }) => {
                    if self.preview_keys_overlay_open {
                        return Ok(());
                    }
                    let Some(selector_area) = self.last_selector_area else {
                        return Ok(());
                    };
                    if column < selector_area.left()
                        || column >= selector_area.right()
                        || row < selector_area.top()
                        || row >= selector_area.bottom()
                    {
                        return Ok(());
                    }

                    // First row is the search bar.
                    let list_y0 = selector_area.y.saturating_add(1);
                    if row < list_y0 {
                        return Ok(());
                    }
                    let list_height = selector_area.height.saturating_sub(1);
                    if list_height == 0 {
                        return Ok(());
                    }

                    let click_row = row.saturating_sub(list_y0) as usize;
                    let indices = self.filtered_indices();
                    let visible = usize::from(list_height).min(indices.len());
                    if visible == 0 || click_row >= visible {
                        return Ok(());
                    }

                    let start = self.scroll_top.min(indices.len().saturating_sub(1));
                    let idx_in_filtered = start.saturating_add(click_row);
                    if idx_in_filtered >= indices.len() {
                        return Ok(());
                    }

                    self.selected_idx = indices[idx_in_filtered];
                    self.follow_selection = true;
                    self.ensure_preview_applied();
                    tui.frame_requester().schedule_frame();
                    Ok(())
                }
                _ => Ok(()),
            },
        }
    }

    fn handle_draw(&mut self, tui: &mut tui::Tui) -> Result<()> {
        self.ensure_preview_applied();
        let requested_scroll = match &self.mode {
            ThemeSelectorMode::Picker { preview_scroll } => *preview_scroll,
        };

        let viewport = tui.terminal.viewport_area;
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(viewport);
        let body_area = parts[0];
        let body_parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(body_area);
        let left = body_parts[0];
        let right = body_parts[1];
        let title_height = 2u16.min(left.height);
        let left_parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(title_height), Constraint::Min(0)])
            .split(left);
        let left_content_area = left_parts[1];
        let right_parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(title_height), Constraint::Min(0)])
            .split(right);
        let right_content_area = Rect::new(
            right_parts[1].x.saturating_add(1),
            right_parts[1].y,
            right_parts[1].width.saturating_sub(1),
            right_parts[1].height,
        );
        self.last_selector_area = Some(left_content_area);
        self.last_preview_area = Some(right_content_area);
        self.last_editor_area = None;

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
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
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

            let (editor_area, preview_area) = if self.editor.is_some() {
                let editor_h = right_content_area.height.min(7);
                let parts = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(editor_h), Constraint::Min(0)])
                    .split(right_content_area);
                (Some(parts[0]), parts[1])
            } else {
                (None, right_content_area)
            };

            self.last_preview_area = Some(preview_area);
            self.last_editor_area = editor_area;

            max_scroll = self.render_preview(preview_area, frame.buffer_mut(), requested_scroll);
            self.render_theme_list(left_content_area, frame.buffer_mut());

            if self.preview_keys_overlay_open {
                self.render_preview_keys_overlay(preview_area, frame.buffer_mut());
            }

            if let Some(editor_area) = editor_area
                && let Some(editor) = self.editor.as_mut()
            {
                editor.render(editor_area, frame.buffer_mut());
                if let Some((x, y)) = editor.render_modals(right_content_area, frame.buffer_mut()) {
                    frame.set_cursor_position((x, y));
                }
            }

            let footer_parts = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(footer_area);

            render_key_hints(
                footer_parts[0],
                frame.buffer_mut(),
                &[(&[KEY_UP, KEY_DOWN], "select"), (&[KEY_TAB], "toggle mode")],
            );
            render_key_hints(
                footer_parts[1],
                frame.buffer_mut(),
                &[(&[KEY_QUESTION], "preview keys")],
            );
        })?;

        let ThemeSelectorMode::Picker { preview_scroll } = &mut self.mode;
        if *preview_scroll > max_scroll {
            *preview_scroll = max_scroll;
            tui.frame_requester().schedule_frame();
        }
        Ok(())
    }

    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

#[cfg(test)]
mod theme_preview_tests {
    use super::*;
    use crate::app_event_sender::AppEventSender;
    use crate::theme;
    use codex_core::config::ConfigBuilder;
    use codex_core::themes::ThemeCatalog;
    use codex_core::themes::ThemeColor;
    use codex_core::themes::ThemeDefinition;
    use codex_core::themes::ThemePalette;
    use codex_core::themes::ThemeColorResolved;
    use codex_core::themes::ThemeVariant;
    use pretty_assertions::assert_eq;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use std::path::PathBuf;
    use tokio::sync::mpsc::unbounded_channel;

    const PREVIEW_WIDTH: u16 = 120;
    const PREVIEW_HEIGHT: u16 = 400;

    async fn test_config() -> codex_core::config::Config {
        let codex_home = std::env::temp_dir();
        ConfigBuilder::default()
            .codex_home(codex_home)
            .build()
            .await
            .expect("config")
    }

    fn test_theme_definition() -> ThemeDefinition {
        let mut theme = ThemeCatalog::built_in_default();
        theme.name = "test-preview".to_string();
        theme.variant = ThemeVariant::Dark;
        theme.palette = ThemePalette {
            black: ThemeColor::new("#0b0c10"),
            red: ThemeColor::new("#d72638"),
            green: ThemeColor::new("#3fbd6b"),
            yellow: ThemeColor::new("#f2c14e"),
            blue: ThemeColor::new("#3f8efc"),
            magenta: ThemeColor::new("#b388ff"),
            cyan: ThemeColor::new("#2ec4b6"),
            white: ThemeColor::new("#e6e6e6"),
            bright_black: ThemeColor::new("#4a4a4a"),
            bright_red: ThemeColor::new("#ff5964"),
            bright_green: ThemeColor::new("#8de969"),
            bright_yellow: ThemeColor::new("#ffe066"),
            bright_blue: ThemeColor::new("#7cb9ff"),
            bright_magenta: ThemeColor::new("#d4a5ff"),
            bright_cyan: ThemeColor::new("#7bdff2"),
            bright_white: ThemeColor::new("#ffffff"),
        };
        theme.roles.fg = ThemeColor::new("palette.white");
        theme.roles.bg = ThemeColor::new("palette.black");
        theme.roles.transcript_bg = Some(ThemeColor::new("#121621"));
        theme.roles.composer_bg = Some(ThemeColor::new("#1b2230"));
        theme.roles.user_prompt_highlight_bg = Some(ThemeColor::new("#2b3a67"));
        theme.roles.status_ramp_fg = Some(ThemeColor::new("#112233"));
        theme.roles.status_ramp_highlight = Some(ThemeColor::new("#f6d6d6"));
        theme.roles.accent = ThemeColor::new("#ff7a00");
        theme.roles.warning = ThemeColor::new("palette.yellow");
        theme
    }

    fn find_in_buffer(buf: &Buffer, needle: &str) -> (u16, u16) {
        let needle_chars: Vec<char> = needle.chars().collect();
        if needle_chars.is_empty() {
            panic!("needle not found in preview: {needle}");
        }
        let width = buf.area.width;
        let height = buf.area.height;
        if width < needle_chars.len() as u16 {
            panic!("needle not found in preview: {needle}");
        }
        for y in 0..height {
            for x in 0..=(width - needle_chars.len() as u16) {
                if (0..needle_chars.len()).all(|offset| {
                    let cell = &buf[(x + offset as u16, y)];
                    let symbol = cell.symbol();
                    let ch = symbol.chars().next().unwrap_or(' ');
                    ch == needle_chars[offset]
                }) {
                    return (y, x);
                }
            }
        }
        panic!("needle not found in preview: {needle}");
    }

    fn assert_span_fg(buf: &Buffer, y: u16, x: u16, len: usize, expected: Option<Color>) {
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            assert_eq!(cell.style().fg, expected);
        }
    }

    fn assert_span_fg_any(
        buf: &Buffer,
        y: u16,
        x: u16,
        len: usize,
        expected: &[Option<Color>],
    ) {
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            assert!(
                expected.contains(&cell.style().fg),
                "unexpected fg at ({},{})",
                x + offset as u16,
                y
            );
        }
    }

    fn span_has_fg_any(
        buf: &Buffer,
        y: u16,
        x: u16,
        len: usize,
        expected: &[Option<Color>],
    ) -> bool {
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            if !expected.contains(&cell.style().fg) {
                return false;
            }
        }
        true
    }

    fn assert_span_bg(buf: &Buffer, y: u16, x: u16, len: usize, expected: Option<Color>) {
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            assert_eq!(cell.style().bg, expected);
        }
    }

    fn find_styled_span<F>(buf: &Buffer, needle: &str, mut matches: F) -> Option<(u16, u16)>
    where
        F: FnMut(&Buffer, u16, u16) -> bool,
    {
        let needle_chars: Vec<char> = needle.chars().collect();
        if needle_chars.is_empty() {
            return None;
        }
        let width = buf.area.width;
        let height = buf.area.height;
        if width < needle_chars.len() as u16 {
            return None;
        }
        for y in 0..height {
            for x in 0..=(width - needle_chars.len() as u16) {
                if (0..needle_chars.len()).all(|offset| {
                    let cell = &buf[(x + offset as u16, y)];
                    let symbol = cell.symbol();
                    let ch = symbol.chars().next().unwrap_or(' ');
                    ch == needle_chars[offset]
                }) && matches(buf, y, x)
                {
                    return Some((y, x));
                }
            }
        }
        None
    }

    fn render_preview(config: codex_core::config::Config, theme: &ThemeDefinition) -> Buffer {
        theme::preview_definition(theme);
        let (tx_raw, _rx) = unbounded_channel();
        let mut overlay = ThemeSelectorOverlay::new(
            AppEventSender::new(tx_raw),
            config.clone(),
            None,
        );
        overlay.frame_requester = Some(crate::tui::FrameRequester::test_dummy());
        overlay.ensure_preview_applied();
        let area = Rect::new(0, 0, PREVIEW_WIDTH, PREVIEW_HEIGHT);
        let mut buf = Buffer::empty(area);
        overlay.render_preview(area, &mut buf, 0);
        buf
    }

    #[tokio::test]
    async fn theme_preview_style_guardrails() {
        let _guard = theme::test_style_guard();
        let mut config = test_config().await;
        config.tui_transcript_user_prompt_highlight = true;
        config.tui_transcript_syntax_highlight = true;
        config.tui_transcript_diff_highlight = true;
        config.tui_minimal_composer = false;

        let test_theme = test_theme_definition();
        let buf = render_preview(config.clone(), &test_theme);
        let accent_fg = theme::accent_style().fg;

        let (prompt_y, prompt_x) = find_in_buffer(&buf, "Give me the highlight reel");
        let expected_prompt_bg = theme::user_prompt_highlight_style().bg;
        assert!(
            expected_prompt_bg.is_some(),
            "expected prompt highlight background"
        );
        assert_span_bg(
            &buf,
            prompt_y,
            prompt_x,
            "Give me the highlight reel".len(),
            expected_prompt_bg,
        );

        let (approval_y, approval_x) =
            find_in_buffer(&buf, "Would you like to run the following command?");
        let approval_bg = user_message_style().patch(theme::composer_style()).bg;
        assert!(
            approval_bg.is_some(),
            "expected approval overlay background"
        );
        assert_span_bg(
            &buf,
            approval_y,
            approval_x,
            "Would you like to run the following command?".len(),
            approval_bg,
        );

        let (branch_y, branch_x) = find_in_buffer(&buf, "branch: feat/themes");
        assert_span_fg(
            &buf,
            branch_y,
            branch_x + "branch: ".len() as u16,
            "feat/themes".len(),
            accent_fg,
        );

        let warning_resolved = test_theme
            .resolve_role("roles.warning", &test_theme.roles.warning)
            .ok()
            .and_then(|resolved| match resolved {
                ThemeColorResolved::Rgb(rgb) => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
                ThemeColorResolved::Inherit => None,
            });
        let expected_warning_fgs = [theme::warning_style().fg, warning_resolved];
        let Some((warn_y, warn_x)) = find_styled_span(
            &buf,
            "warning",
            |buf, y, x| {
                span_has_fg_any(buf, y, x, "warning".len(), &expected_warning_fgs)
                    && span_modifiers_include(buf, y, x, "warning".len(), theme::warning_style())
            },
        ) else {
            panic!("expected styled warning sample in preview");
        };
        assert_span_fg_any(
            &buf,
            warn_y,
            warn_x,
            "warning".len(),
            &expected_warning_fgs,
        );
        assert_span_modifiers_include(
            &buf,
            warn_y,
            warn_x,
            "warning".len(),
            theme::warning_style(),
        );

        let (error_y, error_x) = find_styled_span(&buf, "error", |buf, y, x| {
            span_has_fg_any(buf, y, x, "error".len(), &[theme::error_style().fg])
                && span_modifiers_include(buf, y, x, "error".len(), theme::error_style())
        })
        .expect("expected styled error sample in preview");
        assert_span_fg(
            &buf,
            error_y,
            error_x,
            "error".len(),
            theme::error_style().fg,
        );
        assert_span_modifiers_include(
            &buf,
            error_y,
            error_x,
            "error".len(),
            theme::error_style(),
        );

        let (success_y, success_x) = find_styled_span(&buf, "success", |buf, y, x| {
            span_has_fg_any(buf, y, x, "success".len(), &[theme::success_style().fg])
                && span_modifiers_include(buf, y, x, "success".len(), theme::success_style())
        })
        .expect("expected styled success sample in preview");
        assert_span_fg(
            &buf,
            success_y,
            success_x,
            "success".len(),
            theme::success_style().fg,
        );
        assert_span_modifiers_include(
            &buf,
            success_y,
            success_x,
            "success".len(),
            theme::success_style(),
        );

        let (link_y, link_x) = find_styled_span(&buf, "https://example.com", |buf, y, x| {
            span_has_fg_any(buf, y, x, "https://example.com".len(), &[theme::link_style().fg])
        })
        .expect("expected styled link sample in preview");
        assert_span_fg(
            &buf,
            link_y,
            link_x,
            "https://example.com".len(),
            theme::link_style().fg,
        );
        assert_span_modifier(
            &buf,
            link_y,
            link_x,
            "https://example.com".len(),
            Modifier::UNDERLINED,
        );

        assert!(
            buffer_has_style(&buf, theme::diff_del_highlight_style()),
            "expected diff delete highlight styling in preview"
        );
        assert!(
            buffer_has_style(&buf, theme::diff_add_highlight_style()),
            "expected diff add highlight styling in preview"
        );

        let (status_base, status_highlight) = theme::status_ramp_palette();
        assert_eq!(status_base, (0x11, 0x22, 0x33));
        assert_eq!(status_highlight, (0xf6, 0xd6, 0xd6));

        theme::preview_definition(&test_theme);
        let (tx_raw, _rx) = unbounded_channel();
        let mut composer = crate::bottom_pane::ChatComposer::new(
            true,
            AppEventSender::new(tx_raw),
            false,
            "Ask xcodex to do anything".to_string(),
            false,
        );
        composer.attach_image(PathBuf::from("/tmp/theme-preview.png"));
        let width = 80;
        let height = composer.desired_height(width).max(1);
        let area = Rect::new(0, 0, width, height);
        let mut composer_buf = Buffer::empty(area);
        composer.render(area, &mut composer_buf);
        let (image_y, image_x) = find_in_buffer(&composer_buf, "[Image #1]");
        assert_span_fg(
            &composer_buf,
            image_y,
            image_x,
            "[Image #1]".len(),
            accent_fg,
        );

        let prev_highlight = crate::render::highlight::syntax_highlighting_enabled();
        let sample = SAMPLE_CODE_TABS[0];
        let markdown = format!("```{}\n{}```\n", sample.fence_lang, sample.code);
        let keyword_fg = theme::code_keyword_style().fg;
        assert!(keyword_fg.is_some(), "expected code keyword foreground");

        crate::render::highlight::set_syntax_highlighting_enabled(true);
        let highlighted =
            crate::markdown_render::render_markdown_text_with_width(&markdown, Some(80));

        crate::render::highlight::set_syntax_highlighting_enabled(false);
        let plain = crate::markdown_render::render_markdown_text_with_width(&markdown, Some(80));

        let highlighted_has_keyword = text_has_fg(&highlighted, keyword_fg);
        let plain_has_keyword = text_has_fg(&plain, keyword_fg);
        assert!(
            highlighted_has_keyword && !plain_has_keyword,
            "expected syntax highlighting to change code styles"
        );
        crate::render::highlight::set_syntax_highlighting_enabled(prev_highlight);

        let mut config_minimal = config;
        config_minimal.tui_minimal_composer = true;
        let buf_minimal = render_preview(config_minimal, &test_theme);
        let (branch_y_minimal, _) = find_in_buffer(&buf_minimal, "branch: feat/themes");
        assert_ne!(
            branch_y,
            branch_y_minimal,
            "expected minimal composer to change preview layout"
        );

        theme::preview_definition(&ThemeCatalog::built_in_default());
    }

    fn text_has_fg(text: &ratatui::text::Text<'static>, expected: Option<Color>) -> bool {
        text.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.style.fg == expected)
        })
    }

    fn buffer_has_style(buf: &Buffer, expected: Style) -> bool {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                if cell.style().fg == expected.fg && cell.style().bg == expected.bg {
                    return true;
                }
            }
        }
        false
    }

    fn assert_span_modifier(
        buf: &Buffer,
        y: u16,
        x: u16,
        len: usize,
        expected: Modifier,
    ) {
        let mut found = false;
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            if cell.style().add_modifier.contains(expected) {
                found = true;
                break;
            }
        }
        assert!(found, "expected modifier {expected:?} in span");
    }

    fn span_modifiers_include(
        buf: &Buffer,
        y: u16,
        x: u16,
        len: usize,
        style: Style,
    ) -> bool {
        for offset in 0..len {
            let cell = &buf[(x + offset as u16, y)];
            let cell_mods = cell.style().add_modifier;
            if !cell_mods.contains(style.add_modifier)
                || !cell.style().sub_modifier.contains(style.sub_modifier)
            {
                return false;
            }
        }
        true
    }

    fn assert_span_modifiers_include(
        buf: &Buffer,
        y: u16,
        x: u16,
        len: usize,
        style: Style,
    ) {
        assert!(span_modifiers_include(buf, y, x, len, style));
    }
}
enum ThemeSelectorMode {
    Picker { preview_scroll: u16 },
}

struct ThemeInlineEditor {
    app_event_tx: AppEventSender,
    variant: codex_core::themes::ThemeVariant,
    base_theme_name: String,
    theme: codex_core::themes::ThemeDefinition,
    tab: ThemeEditTab,
    selected_idx: usize,
    scroll_top: usize,
    color_picker: Option<ColorPickerState>,
    last_editor_area: Option<Rect>,
    last_picker_rect: Option<Rect>,
    last_picker_hex_rect: Option<Rect>,
    last_picker_r_rect: Option<Rect>,
    last_picker_g_rect: Option<Rect>,
    last_picker_b_rect: Option<Rect>,
    last_picker_r_value_rect: Option<Rect>,
    last_picker_g_value_rect: Option<Rect>,
    last_picker_b_value_rect: Option<Rect>,
    roles_cols: usize,
    roles_rows: usize,
    save_modal: Option<SaveThemeState>,
    is_done: bool,
}

impl ThemeInlineEditor {
    fn new(
        app_event_tx: AppEventSender,
        variant: codex_core::themes::ThemeVariant,
        base_theme_name: String,
        mut base_theme: codex_core::themes::ThemeDefinition,
    ) -> Self {
        base_theme.variant = variant;
        crate::theme::preview_definition(&base_theme);

        Self {
            app_event_tx,
            variant,
            base_theme_name,
            theme: base_theme,
            tab: ThemeEditTab::Palette,
            selected_idx: 0,
            scroll_top: 0,
            color_picker: None,
            last_editor_area: None,
            last_picker_rect: None,
            last_picker_hex_rect: None,
            last_picker_r_rect: None,
            last_picker_g_rect: None,
            last_picker_b_rect: None,
            last_picker_r_value_rect: None,
            last_picker_g_value_rect: None,
            last_picker_b_value_rect: None,
            roles_cols: 1,
            roles_rows: 1,
            save_modal: None,
            is_done: false,
        }
    }

    fn handle_event(
        &mut self,
        config: &codex_core::config::Config,
        editor_area: Option<Rect>,
        event: &TuiEvent,
    ) -> bool {
        if self.handle_modal_event(config, event) {
            return true;
        }

        match event {
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Tab,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.tab = self.tab.toggle();
                self.selected_idx = 0;
                self.scroll_top = 0;
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let step = if self.tab == ThemeEditTab::Roles {
                    self.roles_cols.max(1) as isize
                } else {
                    1
                };
                self.move_selection(-1, step);
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Down,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let step = if self.tab == ThemeEditTab::Roles {
                    self.roles_cols.max(1) as isize
                } else {
                    1
                };
                self.move_selection(1, step);
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Left,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) if self.tab == ThemeEditTab::Roles => {
                self.move_selection(-1, 1);
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Right,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) if self.tab == ThemeEditTab::Roles => {
                self.move_selection(1, 1);
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if let Some(key) = self.selected_key() {
                    self.open_color_picker(key);
                }
                true
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('s' | 'S'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let suggested = suggested_theme_name(&self.base_theme_name);
                self.save_modal = Some(SaveThemeState {
                    stage: SaveStage::Editing,
                    cursor: suggested.len(),
                    name: suggested,
                    overwrite_path: None,
                    error: None,
                });
                true
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                ..
            }) => {
                let Some(editor_area) = editor_area else {
                    return false;
                };
                if *column < editor_area.left()
                    || *column >= editor_area.right()
                    || *row < editor_area.top()
                    || *row >= editor_area.bottom()
                {
                    return false;
                }
                if let Some(key) = self.hit_test(editor_area, *column, *row) {
                    self.open_color_picker(key);
                    return true;
                }
                false
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column,
                row,
                ..
            }) => {
                let Some(editor_area) = editor_area else {
                    return false;
                };
                if self.tab != ThemeEditTab::Roles {
                    return false;
                }
                if *column < editor_area.left()
                    || *column >= editor_area.right()
                    || *row < editor_area.top()
                    || *row >= editor_area.bottom()
                {
                    return false;
                }
                let delta = -(self.roles_cols.max(1) as isize);
                let max_visible = self
                    .roles_cols
                    .saturating_mul(self.roles_rows)
                    .max(1);
                let max_scroll = role_keys().len().saturating_sub(max_visible);
                let next = (self.scroll_top as isize + delta)
                    .clamp(0, max_scroll as isize) as usize;
                self.scroll_top = next - (next % self.roles_cols.max(1));
                true
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column,
                row,
                ..
            }) => {
                let Some(editor_area) = editor_area else {
                    return false;
                };
                if self.tab != ThemeEditTab::Roles {
                    return false;
                }
                if *column < editor_area.left()
                    || *column >= editor_area.right()
                    || *row < editor_area.top()
                    || *row >= editor_area.bottom()
                {
                    return false;
                }
                let delta = self.roles_cols.max(1) as isize;
                let max_visible = self
                    .roles_cols
                    .saturating_mul(self.roles_rows)
                    .max(1);
                let max_scroll = role_keys().len().saturating_sub(max_visible);
                let next = (self.scroll_top as isize + delta)
                    .clamp(0, max_scroll as isize) as usize;
                self.scroll_top = next - (next % self.roles_cols.max(1));
                true
            }
            _ => false,
        }
    }

    fn replace_theme(
        &mut self,
        variant: codex_core::themes::ThemeVariant,
        base_theme_name: String,
        mut base_theme: codex_core::themes::ThemeDefinition,
    ) {
        base_theme.variant = variant;
        crate::theme::preview_definition(&base_theme);
        self.variant = variant;
        self.base_theme_name = base_theme_name;
        self.theme = base_theme;
        self.selected_idx = 0;
        self.scroll_top = 0;
        self.color_picker = None;
        self.save_modal = None;
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            self.last_editor_area = None;
            return;
        }
        self.last_editor_area = Some(area);

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(crate::theme::composer_style());
            }
        }

        let tabs = Line::from(vec![
            match self.tab {
                ThemeEditTab::Palette => "Palette".bold(),
                ThemeEditTab::Roles => "Palette".into(),
            },
            " ".into(),
            match self.tab {
                ThemeEditTab::Roles => "Roles".bold(),
                ThemeEditTab::Palette => "Roles".into(),
            },
            "   ".into(),
            match self.tab {
                ThemeEditTab::Palette => "(Tab toggle to Roles, Enter pick, Ctrl+S save theme)"
                    .dim(),
                ThemeEditTab::Roles => "(Tab toggle to Palette, Enter pick, Ctrl+S save theme)"
                    .dim(),
            },
        ]);
        tabs.render_ref(Rect::new(area.x, area.y, area.width, 1), buf);

        let content = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        match self.tab {
            ThemeEditTab::Palette => self.render_palette_swatches(content, buf),
            ThemeEditTab::Roles => self.render_roles_list(content, buf),
        }
    }

    fn render_modals(&mut self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        self.last_picker_rect = None;
        self.last_picker_hex_rect = None;
        self.last_picker_r_rect = None;
        self.last_picker_g_rect = None;
        self.last_picker_b_rect = None;
        self.last_picker_r_value_rect = None;
        self.last_picker_g_value_rect = None;
        self.last_picker_b_value_rect = None;
        if let Some(save) = self.save_modal.as_ref() {
            let w = area.width.min(60).max(24);
            let h = 6u16.min(area.height);
            let x = area.x + (area.width.saturating_sub(w)) / 2;
            let y = area.y + (area.height.saturating_sub(h)) / 2;
            let rect = Rect::new(x, y, w, h);
            Clear.render(rect, buf);
            for yy in rect.top()..rect.bottom() {
                for xx in rect.left()..rect.right() {
                    buf[(xx, yy)].set_symbol(" ");
                    buf[(xx, yy)].set_style(crate::theme::composer_style());
                }
            }

            "Save theme as".bold().render_ref(
                Rect::new(rect.x + 2, rect.y, rect.width.saturating_sub(4), 1),
                buf,
            );
            let input = Rect::new(rect.x + 2, rect.y + 2, rect.width.saturating_sub(4), 1);
            Paragraph::new(Line::from(save.name.clone()))
                .style(crate::theme::composer_style())
                .render_ref(input, buf);

            if save.stage == SaveStage::ConfirmOverwrite {
                "Overwrite? (y/n)".magenta().render_ref(
                    Rect::new(rect.x + 2, rect.y + 3, rect.width.saturating_sub(4), 1),
                    buf,
                );
            } else if let Some(err) = save.error.as_ref() {
                err.as_str().red().render_ref(
                    Rect::new(rect.x + 2, rect.y + 3, rect.width.saturating_sub(4), 1),
                    buf,
                );
            } else {
                "Enter to save, Esc to cancel".dim().render_ref(
                    Rect::new(rect.x + 2, rect.y + 3, rect.width.saturating_sub(4), 1),
                    buf,
                );
            }

            let cursor_x = input.x + save.cursor.min(usize::from(input.width)) as u16;
            return Some((cursor_x, input.y));
        }

        let Some(picker) = self.color_picker.as_ref() else {
            return None;
        };

        let w = area.width.min(72).max(28);
        let h = 10u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let rect = Rect::new(x, y, w, h);
        self.last_picker_rect = Some(rect);
        Clear.render(rect, buf);
        for yy in rect.top()..rect.bottom() {
            for xx in rect.left()..rect.right() {
                buf[(xx, yy)].set_symbol(" ");
                buf[(xx, yy)].set_style(crate::theme::composer_style());
            }
        }

        Line::from(vec!["Edit color ".bold(), picker.key.dim()])
            .render_ref(Rect::new(rect.x + 2, rect.y, rect.width - 4, 1), buf);

        let inherit_label = if picker.inherit {
            "inherit: on"
        } else {
            "inherit: off"
        };
        let derived_label = if picker.derived {
            "derived: on"
        } else {
            "derived: off"
        };
        let hex_line: Line<'static> = vec![
            "Hex: ".dim(),
            picker.hex.clone().into(),
            "   ".into(),
            inherit_label.dim(),
            " (i)".dim(),
            "   ".into(),
            derived_label.dim(),
            " (d)".dim(),
        ]
        .into();
        Paragraph::new(hex_line)
            .style(crate::theme::composer_style())
            .render_ref(Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1), buf);

        let hex_input_rect = Rect::new(rect.x + 7, rect.y + 1, rect.width - 4, 1);
        let r_rect = Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1);
        let g_rect = Rect::new(rect.x + 2, rect.y + 4, rect.width - 4, 1);
        let b_rect = Rect::new(rect.x + 2, rect.y + 5, rect.width - 4, 1);
        self.last_picker_hex_rect = Some(hex_input_rect);
        self.last_picker_r_rect = Some(r_rect);
        self.last_picker_g_rect = Some(g_rect);
        self.last_picker_b_rect = Some(b_rect);
        self.last_picker_r_value_rect = Some(Self::rgb_value_rect(r_rect, "R"));
        self.last_picker_g_value_rect = Some(Self::rgb_value_rect(g_rect, "G"));
        self.last_picker_b_value_rect = Some(Self::rgb_value_rect(b_rect, "B"));

        render_rgb_slider(
            r_rect,
            "R",
            picker.r,
            picker.focus == ColorPickerFocus::R,
            buf,
        );
        render_rgb_slider(
            g_rect,
            "G",
            picker.g,
            picker.focus == ColorPickerFocus::G,
            buf,
        );
        render_rgb_slider(
            b_rect,
            "B",
            picker.b,
            picker.focus == ColorPickerFocus::B,
            buf,
        );

        if let Some(err) = picker.error.as_ref() {
            err.as_str()
                .red()
                .render_ref(Rect::new(rect.x + 2, rect.y + 7, rect.width - 4, 1), buf);
        } else {
            "Esc cancels, Enter closes"
                .dim()
                .render_ref(Rect::new(rect.x + 2, rect.y + 7, rect.width - 4, 1), buf);
        }

        if picker.focus == ColorPickerFocus::Hex && !picker.inherit && !picker.derived {
            let input = Rect::new(rect.x + 7, rect.y + 1, rect.width - 4, 1);
            let cursor_x = input.x + picker.cursor.min(picker.hex.len()) as u16;
            return Some((cursor_x, input.y));
        }

        if !picker.inherit && !picker.derived {
            let (value_rect, value_text, cursor) = match picker.focus {
                ColorPickerFocus::R => (
                    Self::rgb_value_rect(r_rect, "R"),
                    picker.r_text.as_str(),
                    picker.r_cursor,
                ),
                ColorPickerFocus::G => (
                    Self::rgb_value_rect(g_rect, "G"),
                    picker.g_text.as_str(),
                    picker.g_cursor,
                ),
                ColorPickerFocus::B => (
                    Self::rgb_value_rect(b_rect, "B"),
                    picker.b_text.as_str(),
                    picker.b_cursor,
                ),
                ColorPickerFocus::Hex => (Rect::default(), "", 0),
            };
            if value_rect.width > 0 && !value_text.is_empty() {
                let digits_start =
                    value_rect.x + value_rect.width.saturating_sub(value_text.len() as u16);
                let cursor_x = digits_start + cursor.min(value_text.len()) as u16;
                return Some((cursor_x, value_rect.y));
            }
        }

        None
    }

    fn rgb_bar_rect(area: Rect, label: &'static str) -> Rect {
        let prefix = format!("{label}: ");
        let bar_w = area
            .width
            .saturating_sub(prefix.len() as u16 + 3 + 4);
        let bar_x = area.x.saturating_add(prefix.len() as u16 + 1);
        Rect::new(bar_x, area.y, bar_w, 1)
    }

    fn rgb_value_rect(area: Rect, label: &'static str) -> Rect {
        let bar = Self::rgb_bar_rect(area, label);
        let value_x = bar.right().saturating_add(2);
        Rect::new(value_x, area.y, 3, 1)
    }

    fn rgb_value_from_column(area: Rect, label: &'static str, column: u16) -> Option<u8> {
        let bar = Self::rgb_bar_rect(area, label);
        if bar.width == 0 || column < bar.left() || column >= bar.right() {
            return None;
        }
        let pos = column.saturating_sub(bar.x) as u32;
        let value = if bar.width == 0 {
            0
        } else {
            (pos.saturating_mul(255) / u32::from(bar.width)).min(255)
        };
        Some(value as u8)
    }

    fn move_selection(&mut self, delta: isize, step: isize) {
        let keys = self.visible_keys();
        if keys.is_empty() {
            return;
        }
        let len = keys.len() as isize;
        let next = (self.selected_idx as isize + delta * step).rem_euclid(len) as usize;
        self.selected_idx = next;

        if self.tab == ThemeEditTab::Roles {
            let visible = self
                .roles_cols
                .saturating_mul(self.roles_rows)
                .max(1);
            let cols = self.roles_cols.max(1);
            let selected_row_start = self.selected_idx - (self.selected_idx % cols);
            if self.selected_idx < self.scroll_top {
                self.scroll_top = selected_row_start;
            } else if self.selected_idx >= self.scroll_top + visible {
                let row_end = selected_row_start.saturating_add(cols.saturating_sub(1));
                self.scroll_top = row_end.saturating_add(1).saturating_sub(visible);
            }
        }
    }

    fn visible_keys(&self) -> &'static [&'static str] {
        match self.tab {
            ThemeEditTab::Palette => palette_keys(),
            ThemeEditTab::Roles => role_keys(),
        }
    }

    fn selected_key(&self) -> Option<&'static str> {
        self.visible_keys().get(self.selected_idx).copied()
    }

    fn hit_test(&self, editor_area: Rect, column: u16, row: u16) -> Option<&'static str> {
        let content = Rect::new(
            editor_area.x,
            editor_area.y.saturating_add(1),
            editor_area.width,
            editor_area.height.saturating_sub(1),
        );
        if content.is_empty() {
            return None;
        }

        match self.tab {
            ThemeEditTab::Palette => {
                let label_rows = 2usize;
                let col_width = (content.width / 8).max(2) as usize;
                let row_height = (content.height / 2).max(2) as usize;
                let x = column.saturating_sub(content.x) as usize;
                let y = row.saturating_sub(content.y) as usize;
                if y >= label_rows * row_height {
                    return None;
                }
                let row = y / row_height;
                let col = x / col_width;
                let idx = row.saturating_mul(8).saturating_add(col);
                palette_keys().get(idx).copied()
            }
            ThemeEditTab::Roles => {
                let cols = self.roles_cols.max(1) as u16;
                let col_width = (content.width / cols).max(1);
                let x = column.saturating_sub(content.x);
                let y = row.saturating_sub(content.y);
                let col = (x / col_width) as usize;
                let row = y as usize;
                let idx = self
                    .scroll_top
                    .saturating_add(row.saturating_mul(self.roles_cols.max(1)))
                    .saturating_add(col);
                role_keys().get(idx).copied()
            }
        }
    }

    fn render_palette_swatches(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let keys = palette_keys();
        let cols = 8usize;
        let rows = 2usize;
        let target_swatch_width = 9u16;
        let target_swatch_height = 9u16;
        let col_width = (area.width / cols as u16).max(2);
        let row_height = (area.height / rows as u16).max(2);
        let swatch_width = target_swatch_width.min(col_width);
        let swatch_height = target_swatch_height.min(row_height.saturating_sub(1));
        let max_rows = (area.height / row_height).min(rows as u16) as usize;
        let label_style = crate::theme::composer_style();
        let selected_style = crate::theme::composer_style()
            .patch(crate::theme::accent_style())
            .add_modifier(Modifier::BOLD);

        for row in 0..max_rows {
            let label_y = area.y + row as u16 * row_height;
            let swatch_y = label_y.saturating_add(1);

            let mut label_spans: Vec<Span<'static>> = Vec::with_capacity(cols * 2);
            let mut swatch_spans: Vec<Span<'static>> = Vec::with_capacity(cols * 2);

            for col in 0..cols {
                let idx = row * cols + col;
                if idx >= keys.len() {
                    break;
                }
                let key = keys[idx];
                let label = format!("{idx:02X}");
                let value = palette_value(&self.theme, key);
                let (inherit, rgb) =
                    parse_theme_color_as_rgb(&self.theme, value.as_str()).unwrap_or((false, None));
                let swatch_style = if inherit {
                    crate::theme::composer_style().patch(crate::theme::dim_style())
                } else if let Some((r, g, b)) = rgb {
                    let c = crate::terminal_palette::best_color((r, g, b));
                    Style::default().fg(c).bg(c)
                } else {
                    crate::theme::warning_style().patch(crate::theme::composer_style())
                };

                let label_span = Span::from(label).set_style(if idx == self.selected_idx {
                    selected_style
                } else {
                    label_style
                });
                label_spans.push(label_span);
                let padding = col_width.saturating_sub(2) as usize;
                if padding > 0 {
                    label_spans.push(Span::from(" ".repeat(padding)).set_style(label_style));
                }

                let swatch_span =
                    Span::from("█".repeat(swatch_width as usize)).set_style(swatch_style);
                swatch_spans.push(swatch_span);
                let swatch_padding = col_width.saturating_sub(swatch_width) as usize;
                if swatch_padding > 0 {
                    swatch_spans
                        .push(Span::from(" ".repeat(swatch_padding)).set_style(label_style));
                }
            }

            Line::from(label_spans).render_ref(
                Rect::new(area.x, label_y, area.width, 1),
                buf,
            );
            for offset in 0..swatch_height {
                let draw_y = swatch_y.saturating_add(offset);
                if draw_y >= area.bottom() {
                    break;
                }
                Line::from(swatch_spans.clone()).render_ref(
                    Rect::new(area.x, draw_y, area.width, 1),
                    buf,
                );
            }
        }
    }

    fn render_roles_list(&mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            self.roles_cols = 1;
            self.roles_rows = 1;
            return;
        }

        let keys = role_keys();
        if keys.is_empty() {
            self.roles_cols = 1;
            self.roles_rows = 1;
            return;
        }

        let cols = if area.width >= 90 {
            3
        } else if area.width >= 60 {
            2
        } else {
            1
        };
        let cols = cols.max(1);
        let rows = usize::from(area.height.max(1));
        self.roles_cols = cols;
        self.roles_rows = rows;
        let col_width = area.width / cols as u16;
        let visible = rows.saturating_mul(cols).min(keys.len());
        let mut start = self.scroll_top.min(keys.len().saturating_sub(1));
        start = start - (start % cols);
        let end = (start + visible).min(keys.len());

        for idx in start..end {
            let local = idx - start;
            let row = local / cols;
            let col = local % cols;
            let y = area.y + row as u16;
            let x = area.x + col as u16 * col_width;
            let key = keys[idx];
            let value = role_value(&self.theme, key).unwrap_or_default();
            let display_value = if value.trim().is_empty() && is_optional_role_key(key) {
                "derived".to_string()
            } else if value.trim().is_empty() {
                "unset".to_string()
            } else {
                value.clone()
            };
            let (inherit, rgb) =
                parse_theme_color_as_rgb(&self.theme, value.as_str()).unwrap_or((false, None));
            let swatch_style = if inherit {
                crate::theme::composer_style().patch(crate::theme::dim_style())
            } else if let Some((r, g, b)) = rgb {
                let c = crate::terminal_palette::best_color((r, g, b));
                Style::default().fg(c).bg(c)
            } else {
                crate::theme::warning_style().patch(crate::theme::composer_style())
            };

            let mut style = if display_value == "derived" || display_value == "unset" {
                crate::theme::composer_style().patch(crate::theme::dim_style())
            } else {
                crate::theme::composer_style()
            };
            if idx == self.selected_idx {
                style = style.patch(crate::theme::accent_style());
            }
            let label = role_label(key);
            let label_style = style.add_modifier(Modifier::BOLD);
            let line: Line<'static> = vec![
                Span::from("██").set_style(style),
                " ".into(),
                Span::from(display_value.clone()).set_style(style),
                " ".into(),
                Span::from(label).set_style(label_style),
            ]
            .into();
            Paragraph::new(line).style(style).render_ref(
                Rect::new(x, y, col_width, 1),
                buf,
            );
            let swatch_rect = Rect::new(x, y, col_width.min(2), 1);
            for sx in swatch_rect.left()..swatch_rect.right() {
                buf[(sx, y)].set_style(swatch_style);
                buf[(sx, y)].set_symbol("█");
            }
        }
    }

    fn open_color_picker(&mut self, key: &'static str) {
        let current = if key.starts_with("palette.") {
            palette_value(&self.theme, key)
        } else {
            role_value(&self.theme, key).unwrap_or_default()
        };

        let (inherit, rgb) = match parse_theme_color_as_rgb(&self.theme, current.as_str()) {
            Ok(value) => value,
            Err(err) => {
                self.color_picker = Some(ColorPickerState {
                    key,
                    original_value: current,
                    hex: "000000".to_string(),
                    cursor: 0,
                    r: 0,
                    g: 0,
                    b: 0,
                    r_text: "0".to_string(),
                    g_text: "0".to_string(),
                    b_text: "0".to_string(),
                    r_cursor: 1,
                    g_cursor: 1,
                    b_cursor: 1,
                    derived: false,
                    inherit: false,
                    focus: ColorPickerFocus::Hex,
                    error: Some(err),
                });
                return;
            }
        };

        let derived = current.trim().is_empty() && is_optional_role_key(key);
        let (r, g, b, hex) = if inherit || derived {
            (0, 0, 0, "000000".to_string())
        } else if let Some((r, g, b)) = rgb {
            (r, g, b, format!("{r:02X}{g:02X}{b:02X}"))
        } else {
            (0, 0, 0, "000000".to_string())
        };
        let r_text = r.to_string();
        let g_text = g.to_string();
        let b_text = b.to_string();
        let r_cursor = r_text.len();
        let g_cursor = g_text.len();
        let b_cursor = b_text.len();

        self.color_picker = Some(ColorPickerState {
            key,
            original_value: current,
            hex,
            cursor: 6,
            r,
            g,
            b,
            r_text,
            g_text,
            b_text,
            r_cursor,
            g_cursor,
            b_cursor,
            derived,
            inherit,
            focus: ColorPickerFocus::Hex,
            error: None,
        });
    }

    fn apply_color_picker_live(&mut self) {
        let Some(picker) = self.color_picker.as_ref() else {
            return;
        };

        if picker.derived && is_optional_role_key(picker.key) {
            let _ = set_role_value(&mut self.theme, picker.key, "");
            crate::theme::preview_definition(&self.theme);
            return;
        }

        let value = if picker.inherit {
            "inherit".to_string()
        } else if picker.hex.len() == 6 {
            format!("#{}", picker.hex)
        } else {
            return;
        };

        if picker.key.starts_with("palette.") {
            set_palette_value(&mut self.theme, picker.key, value.as_str());
        } else {
            let _ = set_role_value(&mut self.theme, picker.key, value.as_str());
        }
        crate::theme::preview_definition(&self.theme);
    }

    fn cancel_color_picker(&mut self) {
        let Some(picker) = self.color_picker.take() else {
            return;
        };

        if picker.key.starts_with("palette.") {
            set_palette_value(&mut self.theme, picker.key, picker.original_value.as_str());
        } else {
            let _ = set_role_value(&mut self.theme, picker.key, picker.original_value.as_str());
        }
        crate::theme::preview_definition(&self.theme);
    }

    fn sync_picker_hex_from_rgb(picker: &mut ColorPickerState) {
        if picker.inherit || picker.derived {
            return;
        }
        picker.hex = format!("{:02X}{:02X}{:02X}", picker.r, picker.g, picker.b);
        picker.cursor = picker.hex.len();
    }

    fn sync_picker_rgb_text_from_rgb(picker: &mut ColorPickerState) {
        picker.r_text = picker.r.to_string();
        picker.g_text = picker.g.to_string();
        picker.b_text = picker.b.to_string();
        picker.r_cursor = picker.r_text.len();
        picker.g_cursor = picker.g_text.len();
        picker.b_cursor = picker.b_text.len();
    }

    fn handle_modal_event(
        &mut self,
        config: &codex_core::config::Config,
        event: &TuiEvent,
    ) -> bool {
        if let Some(save) = self.save_modal.as_mut() {
            let TuiEvent::Key(key) = event else {
                return true;
            };
            match key.code {
                KeyCode::Esc => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        self.save_modal = None;
                    }
                    true
                }
                KeyCode::Enter => {
                    if matches!(key.kind, KeyEventKind::Press) && save.stage == SaveStage::Editing {
                        self.commit_save(config);
                    }
                    true
                }
                KeyCode::Char('y' | 'Y') => {
                    if matches!(key.kind, KeyEventKind::Press)
                        && save.stage == SaveStage::ConfirmOverwrite
                    {
                        self.commit_overwrite(config, true);
                    }
                    true
                }
                KeyCode::Char('n' | 'N') => {
                    if matches!(key.kind, KeyEventKind::Press)
                        && save.stage == SaveStage::ConfirmOverwrite
                    {
                        self.commit_overwrite(config, false);
                    }
                    true
                }
                _ => {
                    if save.stage == SaveStage::Editing {
                        apply_text_edit(&mut save.name, &mut save.cursor, key);
                    }
                    true
                }
            }
        } else if let Some(picker) = self.color_picker.as_mut() {
            let mut apply_live = false;
            let mut handled = false;
            match event {
                TuiEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    ..
                }) => {
                    if let Some(rect) = self.last_picker_rect
                        && (*column < rect.left()
                            || *column >= rect.right()
                            || *row < rect.top()
                            || *row >= rect.bottom())
                    {
                        self.color_picker = None;
                        handled = false;
                        return handled;
                    }

                    if let Some(hex_rect) = self.last_picker_hex_rect
                        && *row == hex_rect.y
                        && *column >= hex_rect.left()
                        && *column < hex_rect.right()
                    {
                        picker.focus = ColorPickerFocus::Hex;
                        if !picker.inherit && !picker.derived {
                            let cursor = column.saturating_sub(hex_rect.x) as usize;
                            picker.cursor = cursor.min(picker.hex.len());
                        }
                        handled = true;
                    }

                    if !handled && !picker.inherit && !picker.derived {
                        if let Some(r_rect) = self.last_picker_r_rect
                            && *row == r_rect.y
                        {
                            picker.focus = ColorPickerFocus::R;
                            if let Some(value) = Self::rgb_value_from_column(r_rect, "R", *column)
                            {
                                picker.r = value;
                                Self::sync_picker_rgb_text_from_rgb(picker);
                                Self::sync_picker_hex_from_rgb(picker);
                                apply_live = true;
                                handled = true;
                            }
                            if let Some(value_rect) = self.last_picker_r_value_rect
                                && *column >= value_rect.left()
                                && *column < value_rect.right()
                            {
                                let digits_start = value_rect
                                    .x
                                    .saturating_add(
                                        value_rect.width.saturating_sub(picker.r_text.len() as u16),
                                    );
                                let cursor = column.saturating_sub(digits_start) as usize;
                                picker.r_cursor = cursor.min(picker.r_text.len());
                                handled = true;
                            }
                        }

                        if let Some(g_rect) = self.last_picker_g_rect
                            && *row == g_rect.y
                        {
                            picker.focus = ColorPickerFocus::G;
                            if let Some(value) = Self::rgb_value_from_column(g_rect, "G", *column)
                            {
                                picker.g = value;
                                Self::sync_picker_rgb_text_from_rgb(picker);
                                Self::sync_picker_hex_from_rgb(picker);
                                apply_live = true;
                                handled = true;
                            }
                            if let Some(value_rect) = self.last_picker_g_value_rect
                                && *column >= value_rect.left()
                                && *column < value_rect.right()
                            {
                                let digits_start = value_rect
                                    .x
                                    .saturating_add(
                                        value_rect.width.saturating_sub(picker.g_text.len() as u16),
                                    );
                                let cursor = column.saturating_sub(digits_start) as usize;
                                picker.g_cursor = cursor.min(picker.g_text.len());
                                handled = true;
                            }
                        }

                        if let Some(b_rect) = self.last_picker_b_rect
                            && *row == b_rect.y
                        {
                            picker.focus = ColorPickerFocus::B;
                            if let Some(value) = Self::rgb_value_from_column(b_rect, "B", *column)
                            {
                                picker.b = value;
                                Self::sync_picker_rgb_text_from_rgb(picker);
                                Self::sync_picker_hex_from_rgb(picker);
                                apply_live = true;
                                handled = true;
                            }
                            if let Some(value_rect) = self.last_picker_b_value_rect
                                && *column >= value_rect.left()
                                && *column < value_rect.right()
                            {
                                let digits_start = value_rect
                                    .x
                                    .saturating_add(
                                        value_rect.width.saturating_sub(picker.b_text.len() as u16),
                                    );
                                let cursor = column.saturating_sub(digits_start) as usize;
                                picker.b_cursor = cursor.min(picker.b_text.len());
                                handled = true;
                            }
                        }
                    }
                }
                TuiEvent::Key(key) => match key.code {
                KeyCode::Esc => {
                    if matches!(key.kind, KeyEventKind::Press) {
                        self.cancel_color_picker();
                    }
                    handled = true;
                }
                KeyCode::Enter => {
                    if matches!(key.kind, KeyEventKind::Press) {
                        self.color_picker = None;
                    }
                    handled = true;
                }
                KeyCode::Char('i' | 'I') => {
                    if matches!(key.kind, KeyEventKind::Press) {
                        picker.inherit = !picker.inherit;
                        if picker.inherit {
                            picker.derived = false;
                        }
                        apply_live = true;
                    }
                    handled = true;
                }
                KeyCode::Char('d' | 'D') => {
                    if matches!(key.kind, KeyEventKind::Press) && is_optional_role_key(picker.key) {
                        picker.derived = !picker.derived;
                        if picker.derived {
                            picker.inherit = false;
                        }
                        apply_live = true;
                    }
                    handled = true;
                }
                KeyCode::Up => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        picker.focus = match picker.focus {
                            ColorPickerFocus::Hex => ColorPickerFocus::R,
                            ColorPickerFocus::R => ColorPickerFocus::R,
                            ColorPickerFocus::G => ColorPickerFocus::R,
                            ColorPickerFocus::B => ColorPickerFocus::G,
                        };
                    }
                    handled = true;
                }
                KeyCode::Down => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        picker.focus = match picker.focus {
                            ColorPickerFocus::Hex => ColorPickerFocus::R,
                            ColorPickerFocus::R => ColorPickerFocus::G,
                            ColorPickerFocus::G => ColorPickerFocus::B,
                            ColorPickerFocus::B => ColorPickerFocus::B,
                        };
                    }
                    handled = true;
                }
                KeyCode::Left => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && !picker.inherit
                        && !picker.derived
                    {
                        match picker.focus {
                            ColorPickerFocus::R => picker.r = picker.r.saturating_sub(1),
                            ColorPickerFocus::G => picker.g = picker.g.saturating_sub(1),
                            ColorPickerFocus::B => picker.b = picker.b.saturating_sub(1),
                            ColorPickerFocus::Hex => {}
                        }
                        Self::sync_picker_rgb_text_from_rgb(picker);
                        Self::sync_picker_hex_from_rgb(picker);
                        apply_live = true;
                    }
                    handled = true;
                }
                KeyCode::Right => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && !picker.inherit
                        && !picker.derived
                    {
                        match picker.focus {
                            ColorPickerFocus::R => picker.r = picker.r.saturating_add(1),
                            ColorPickerFocus::G => picker.g = picker.g.saturating_add(1),
                            ColorPickerFocus::B => picker.b = picker.b.saturating_add(1),
                            ColorPickerFocus::Hex => {}
                        }
                        Self::sync_picker_rgb_text_from_rgb(picker);
                        Self::sync_picker_hex_from_rgb(picker);
                        apply_live = true;
                    }
                    handled = true;
                }
                _ => {
                    if picker.focus == ColorPickerFocus::Hex
                        && !picker.inherit
                        && !picker.derived
                        && apply_hex_edit(&mut picker.hex, &mut picker.cursor, key)
                        && picker.hex.len() == 6
                        && let Ok((r, g, b)) = parse_hex_rgb(picker.hex.as_str())
                    {
                        picker.r = r;
                        picker.g = g;
                        picker.b = b;
                        Self::sync_picker_rgb_text_from_rgb(picker);
                        apply_live = true;
                    }
                    if !picker.inherit && !picker.derived {
                        match picker.focus {
                            ColorPickerFocus::R => {
                                if apply_rgb_edit(&mut picker.r_text, &mut picker.r_cursor, key) {
                                    if picker.r_text.is_empty() {
                                        picker.r = 0;
                                        picker.r_text = "0".to_string();
                                        picker.r_cursor = picker.r_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    } else if let Some(value) = parse_rgb_text(picker.r_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.r = clamped;
                                        picker.r_text = clamped.to_string();
                                        picker.r_cursor = picker.r_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                }
                            }
                            ColorPickerFocus::G => {
                                if apply_rgb_edit(&mut picker.g_text, &mut picker.g_cursor, key) {
                                    if picker.g_text.is_empty() {
                                        picker.g = 0;
                                        picker.g_text = "0".to_string();
                                        picker.g_cursor = picker.g_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    } else if let Some(value) = parse_rgb_text(picker.g_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.g = clamped;
                                        picker.g_text = clamped.to_string();
                                        picker.g_cursor = picker.g_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                }
                            }
                            ColorPickerFocus::B => {
                                if apply_rgb_edit(&mut picker.b_text, &mut picker.b_cursor, key) {
                                    if picker.b_text.is_empty() {
                                        picker.b = 0;
                                        picker.b_text = "0".to_string();
                                        picker.b_cursor = picker.b_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    } else if let Some(value) = parse_rgb_text(picker.b_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.b = clamped;
                                        picker.b_text = clamped.to_string();
                                        picker.b_cursor = picker.b_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                }
                            }
                            ColorPickerFocus::Hex => {}
                        }
                    }
                    handled = true;
                }
            },
                _ => {
                    handled = true;
                }
            }
            if apply_live {
                self.apply_color_picker_live();
            }
            handled
        } else {
            false
        }
    }

    fn commit_save(&mut self, config: &codex_core::config::Config) {
        use codex_core::themes::ThemeCatalog;

        let Some(save) = self.save_modal.as_mut() else {
            return;
        };

        let name = save.name.trim().to_string();
        if name.is_empty() {
            save.error = Some("Theme name cannot be empty.".to_string());
            return;
        }

        let mut out = self.theme.clone();
        out.name = name.clone();
        out.variant = self.variant;

        if let Err(err) = out.validate() {
            save.error = Some(format!("Theme is not valid: {err}"));
            return;
        }

        let catalog = match ThemeCatalog::load(config) {
            Ok(catalog) => catalog,
            Err(err) => {
                save.error = Some(format!("Failed to load themes: {err}"));
                return;
            }
        };

        if catalog.is_built_in_name(name.as_str()) {
            save.error = Some(format!(
                "Theme `{name}` is built-in and cannot be overwritten."
            ));
            return;
        }

        if let Some(existing) = catalog
            .user_theme_path(name.as_str())
            .map(ToOwned::to_owned)
        {
            save.stage = SaveStage::ConfirmOverwrite;
            save.overwrite_path = Some(existing);
            save.error = None;
            return;
        }

        let dir = codex_core::themes::themes_dir(&config.codex_home, &config.xcodex.themes);
        if let Err(err) = std::fs::create_dir_all(&dir) {
            save.error = Some(format!(
                "Failed to create themes directory `{}`: {err}",
                dir.display()
            ));
            return;
        }

        let yaml = match out.to_yaml() {
            Ok(yaml) => yaml,
            Err(err) => {
                save.error = Some(format!("Failed to serialize theme YAML: {err}"));
                return;
            }
        };

        let path = unique_theme_path(&dir, name.as_str());
        if let Err(err) = std::fs::write(&path, yaml) {
            save.error = Some(format!("Failed to write `{}`: {err}", path.display()));
            return;
        }

        self.app_event_tx.send(AppEvent::PersistThemeSelection {
            variant: self.variant,
            theme: name,
        });
        self.is_done = true;
    }

    fn commit_overwrite(&mut self, _config: &codex_core::config::Config, overwrite: bool) {
        let Some(save) = self.save_modal.as_mut() else {
            return;
        };

        if !overwrite {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = None;
            return;
        }

        let Some(path) = save.overwrite_path.clone() else {
            save.stage = SaveStage::Editing;
            save.error = None;
            return;
        };

        let name = save.name.trim().to_string();
        if name.is_empty() {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = Some("Theme name cannot be empty.".to_string());
            return;
        }

        let mut out = self.theme.clone();
        out.name = name.clone();
        out.variant = self.variant;

        let yaml = match out.to_yaml() {
            Ok(yaml) => yaml,
            Err(err) => {
                save.stage = SaveStage::Editing;
                save.overwrite_path = None;
                save.error = Some(format!("Failed to serialize theme YAML: {err}"));
                return;
            }
        };

        if let Err(err) = std::fs::write(&path, yaml) {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = Some(format!("Failed to write `{}`: {err}", path.display()));
            return;
        }

        self.app_event_tx.send(AppEvent::PersistThemeSelection {
            variant: self.variant,
            theme: name,
        });
        self.is_done = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemeEditTab {
    Palette,
    Roles,
}

impl ThemeEditTab {
    fn toggle(self) -> Self {
        match self {
            Self::Palette => Self::Roles,
            Self::Roles => Self::Palette,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorPickerFocus {
    Hex,
    R,
    G,
    B,
}

struct ColorPickerState {
    key: &'static str,
    original_value: String,
    // Uppercase RRGGBB (no leading #).
    hex: String,
    cursor: usize,
    r: u8,
    g: u8,
    b: u8,
    r_text: String,
    g_text: String,
    b_text: String,
    r_cursor: usize,
    g_cursor: usize,
    b_cursor: usize,
    derived: bool,
    inherit: bool,
    focus: ColorPickerFocus,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SaveStage {
    Editing,
    ConfirmOverwrite,
}

struct SaveThemeState {
    stage: SaveStage,
    name: String,
    cursor: usize,
    overwrite_path: Option<PathBuf>,
    error: Option<String>,
}

const THEME_PREVIEW_ROLE_KEYS: [&str; 44] = [
    "roles.fg",
    "roles.bg",
    "roles.transcript_bg",
    "roles.composer_bg",
    "roles.user_prompt_highlight_bg",
    "roles.status_bg",
    "roles.status_ramp_fg",
    "roles.status_ramp_highlight",
    "roles.selection_fg",
    "roles.selection_bg",
    "roles.cursor_fg",
    "roles.cursor_bg",
    "roles.border",
    "roles.accent",
    "roles.brand",
    "roles.command",
    "roles.success",
    "roles.warning",
    "roles.error",
    "roles.diff_add_fg",
    "roles.diff_add_bg",
    "roles.diff_del_fg",
    "roles.diff_del_bg",
    "roles.diff_hunk_fg",
    "roles.diff_hunk_bg",
    "roles.badge",
    "roles.link",
    "roles.code_keyword",
    "roles.code_operator",
    "roles.code_comment",
    "roles.code_string",
    "roles.code_number",
    "roles.code_type",
    "roles.code_function",
    "roles.code_constant",
    "roles.code_macro",
    "roles.code_punctuation",
    "roles.code_variable",
    "roles.code_property",
    "roles.code_attribute",
    "roles.code_module",
    "roles.code_label",
    "roles.code_tag",
    "roles.code_embedded",
];

const THEME_PREVIEW_PALETTE_KEYS: [&str; 16] = [
    "palette.black",
    "palette.red",
    "palette.green",
    "palette.yellow",
    "palette.blue",
    "palette.magenta",
    "palette.cyan",
    "palette.white",
    "palette.bright_black",
    "palette.bright_red",
    "palette.bright_green",
    "palette.bright_yellow",
    "palette.bright_blue",
    "palette.bright_magenta",
    "palette.bright_cyan",
    "palette.bright_white",
];

fn role_keys() -> &'static [&'static str] {
    &THEME_PREVIEW_ROLE_KEYS
}

fn role_label(key: &str) -> &'static str {
    match key {
        "roles.fg" => "Foreground",
        "roles.bg" => "Background",
        "roles.transcript_bg" => "Transcript",
        "roles.composer_bg" => "Composer",
        "roles.user_prompt_highlight_bg" => "Prompt Highlight",
        "roles.status_bg" => "Status",
        "roles.status_ramp_fg" => "Status Ramp FG",
        "roles.status_ramp_highlight" => "Status Ramp Highlight",
        "roles.selection_fg" => "Selection FG",
        "roles.selection_bg" => "Selection BG",
        "roles.cursor_fg" => "Cursor FG",
        "roles.cursor_bg" => "Cursor BG",
        "roles.border" => "Border",
        "roles.accent" => "Accent",
        "roles.brand" => "Brand",
        "roles.command" => "Command",
        "roles.success" => "Success",
        "roles.warning" => "Warning",
        "roles.error" => "Error",
        "roles.diff_add_fg" => "Diff Add FG",
        "roles.diff_add_bg" => "Diff Add BG",
        "roles.diff_del_fg" => "Diff Del FG",
        "roles.diff_del_bg" => "Diff Del BG",
        "roles.diff_hunk_fg" => "Diff Hunk FG",
        "roles.diff_hunk_bg" => "Diff Hunk BG",
        "roles.badge" => "Badge",
        "roles.link" => "Link",
        "roles.code_keyword" => "Code Keyword",
        "roles.code_operator" => "Code Operator",
        "roles.code_comment" => "Code Comment",
        "roles.code_string" => "Code String",
        "roles.code_number" => "Code Number",
        "roles.code_type" => "Code Type",
        "roles.code_function" => "Code Function",
        "roles.code_constant" => "Code Constant",
        "roles.code_macro" => "Code Macro",
        "roles.code_punctuation" => "Code Punctuation",
        "roles.code_variable" => "Code Variable",
        "roles.code_property" => "Code Property",
        "roles.code_attribute" => "Code Attribute",
        "roles.code_module" => "Code Module",
        "roles.code_label" => "Code Label",
        "roles.code_tag" => "Code Tag",
        "roles.code_embedded" => "Code Embedded",
        _ => "Role",
    }
}

fn palette_keys() -> &'static [&'static str] {
    &THEME_PREVIEW_PALETTE_KEYS
}

fn is_optional_role_key(key: &str) -> bool {
    matches!(
        key,
        "roles.transcript_bg"
            | "roles.composer_bg"
            | "roles.user_prompt_highlight_bg"
            | "roles.status_bg"
            | "roles.status_ramp_fg"
            | "roles.status_ramp_highlight"
            | "roles.badge"
            | "roles.link"
    )
}

fn apply_text_edit(text: &mut String, cursor: &mut usize, key: &KeyEvent) {
    match key {
        KeyEvent {
            code: KeyCode::Left,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyEvent {
            code: KeyCode::Right,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = (*cursor + 1).min(text.chars().count());
        }
        KeyEvent {
            code: KeyCode::Home,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = 0;
        }
        KeyEvent {
            code: KeyCode::End,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = text.chars().count();
        }
        KeyEvent {
            code: KeyCode::Backspace,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if *cursor == 0 {
                return;
            }
            let mut chars: Vec<char> = text.chars().collect();
            chars.remove(*cursor - 1);
            *cursor -= 1;
            *text = chars.into_iter().collect();
        }
        KeyEvent {
            code: KeyCode::Delete,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            let mut chars: Vec<char> = text.chars().collect();
            if *cursor >= chars.len() {
                return;
            }
            chars.remove(*cursor);
            *text = chars.into_iter().collect();
        }
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if ch.is_ascii_control() {
                return;
            }
            let mut chars: Vec<char> = text.chars().collect();
            let idx = (*cursor).min(chars.len());
            chars.insert(idx, *ch);
            *cursor += 1;
            *text = chars.into_iter().collect();
        }
        _ => {}
    }
}

fn apply_hex_edit(hex: &mut String, cursor: &mut usize, key: &KeyEvent) -> bool {
    let before = hex.clone();
    let before_cursor = *cursor;
    match key {
        KeyEvent {
            code: KeyCode::Left,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyEvent {
            code: KeyCode::Right,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = (*cursor + 1).min(hex.len());
        }
        KeyEvent {
            code: KeyCode::Backspace,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if *cursor == 0 || hex.is_empty() {
                return false;
            }
            hex.remove(*cursor - 1);
            *cursor -= 1;
        }
        KeyEvent {
            code: KeyCode::Delete,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if *cursor >= hex.len() {
                return false;
            }
            hex.remove(*cursor);
        }
        KeyEvent {
            code: KeyCode::Home,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = 0;
        }
        KeyEvent {
            code: KeyCode::End,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = hex.len();
        }
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if !ch.is_ascii_hexdigit() {
                return false;
            }
            if hex.len() >= 6 {
                return false;
            }
            let idx = (*cursor).min(hex.len());
            hex.insert(idx, ch.to_ascii_uppercase());
            *cursor += 1;
        }
        _ => return false,
    }

    before != *hex || before_cursor != *cursor
}

fn apply_rgb_edit(value: &mut String, cursor: &mut usize, key: &KeyEvent) -> bool {
    let before = value.clone();
    let before_cursor = *cursor;
    match key {
        KeyEvent {
            code: KeyCode::Left,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyEvent {
            code: KeyCode::Right,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = (*cursor + 1).min(value.len());
        }
        KeyEvent {
            code: KeyCode::Backspace,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if *cursor == 0 || value.is_empty() {
                return false;
            }
            value.remove(*cursor - 1);
            *cursor -= 1;
        }
        KeyEvent {
            code: KeyCode::Delete,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if *cursor >= value.len() {
                return false;
            }
            value.remove(*cursor);
        }
        KeyEvent {
            code: KeyCode::Home,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = 0;
        }
        KeyEvent {
            code: KeyCode::End,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            *cursor = value.len();
        }
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => {
            if !ch.is_ascii_digit() {
                return false;
            }
            if value.len() >= 3 {
                return false;
            }
            let idx = (*cursor).min(value.len());
            value.insert(idx, *ch);
            *cursor += 1;
        }
        _ => return false,
    }

    before != *value || before_cursor != *cursor
}

fn parse_rgb_text(text: &str) -> Option<u16> {
    if text.is_empty() {
        return None;
    }
    if !text.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok()
}

fn parse_hex_rgb(hex: &str) -> std::result::Result<(u8, u8, u8), ()> {
    if hex.len() != 6 {
        return Err(());
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| ())?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| ())?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| ())?;
    Ok((r, g, b))
}

fn render_rgb_slider(
    area: Rect,
    label: &'static str,
    value: u8,
    focused: bool,
    buf: &mut Buffer,
) {
    if area.is_empty() {
        return;
    }

    let prefix = format!("{label}: ");
    let value_text = format!("{value:>3}");
    let bar_w = area
        .width
        .saturating_sub(prefix.len() as u16 + value_text.len() as u16 + 4);
    let filled = if bar_w == 0 {
        0
    } else {
        ((u64::from(value) * u64::from(bar_w)) / 255).min(u64::from(bar_w)) as u16
    };

    let line: Line<'static> = vec![
        prefix.into(),
        "[".into(),
        "█".repeat(filled as usize).into(),
        " ".repeat(bar_w.saturating_sub(filled) as usize).into(),
        "] ".into(),
        value_text.into(),
    ]
    .into();

    let base = crate::theme::composer_style();
    let style = if focused {
        base.patch(crate::theme::accent_style())
            .add_modifier(Modifier::BOLD)
    } else {
        base
    };

    Paragraph::new(line).style(style).render_ref(area, buf);
}

fn parse_theme_color_as_rgb(
    theme: &codex_core::themes::ThemeDefinition,
    value: &str,
) -> std::result::Result<(bool, Option<(u8, u8, u8)>), String> {
    if value.trim().is_empty() {
        return Ok((false, None));
    }
    if value == "inherit" {
        return Ok((true, None));
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_rgb(hex)
            .map(|(r, g, b)| (false, Some((r, g, b))))
            .map_err(|()| format!("Invalid hex color: `{value}`"));
    }

    let color = codex_core::themes::ThemeColor::new(value.to_string());
    match color.resolve(&theme.palette) {
        Some(codex_core::themes::ThemeColorResolved::Inherit) => Ok((true, None)),
        Some(codex_core::themes::ThemeColorResolved::Rgb(codex_core::themes::ThemeRgb(
            r,
            g,
            b,
        ))) => Ok((false, Some((r, g, b)))),
        None => Err(format!("Unresolvable color: `{value}`")),
    }
}

fn role_value(theme: &codex_core::themes::ThemeDefinition, key: &'static str) -> Option<String> {
    match key {
        "roles.fg" => Some(theme.roles.fg.to_string()),
        "roles.bg" => Some(theme.roles.bg.to_string()),
        "roles.transcript_bg" => Some(
            theme
                .roles
                .transcript_bg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        "roles.composer_bg" => Some(
            theme
                .roles
                .composer_bg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        "roles.user_prompt_highlight_bg" => Some(
            theme
                .roles
                .user_prompt_highlight_bg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        "roles.status_bg" => Some(
            theme
                .roles
                .status_bg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        "roles.selection_fg" => Some(theme.roles.selection_fg.to_string()),
        "roles.selection_bg" => Some(theme.roles.selection_bg.to_string()),
        "roles.cursor_fg" => Some(theme.roles.cursor_fg.to_string()),
        "roles.cursor_bg" => Some(theme.roles.cursor_bg.to_string()),
        "roles.border" => Some(theme.roles.border.to_string()),
        "roles.accent" => Some(theme.roles.accent.to_string()),
        "roles.brand" => Some(theme.roles.brand.to_string()),
        "roles.command" => Some(theme.roles.command.to_string()),
        "roles.success" => Some(theme.roles.success.to_string()),
        "roles.warning" => Some(theme.roles.warning.to_string()),
        "roles.error" => Some(theme.roles.error.to_string()),
        "roles.diff_add_fg" => Some(theme.roles.diff_add_fg.to_string()),
        "roles.diff_add_bg" => Some(theme.roles.diff_add_bg.to_string()),
        "roles.diff_del_fg" => Some(theme.roles.diff_del_fg.to_string()),
        "roles.diff_del_bg" => Some(theme.roles.diff_del_bg.to_string()),
        "roles.diff_hunk_fg" => Some(theme.roles.diff_hunk_fg.to_string()),
        "roles.diff_hunk_bg" => Some(theme.roles.diff_hunk_bg.to_string()),
        "roles.badge" => theme.roles.badge.as_ref().map(ToString::to_string),
        "roles.link" => theme.roles.link.as_ref().map(ToString::to_string),
        "roles.code_keyword" => Some(theme.roles.code_keyword.to_string()),
        "roles.code_operator" => Some(theme.roles.code_operator.to_string()),
        "roles.code_comment" => Some(theme.roles.code_comment.to_string()),
        "roles.code_string" => Some(theme.roles.code_string.to_string()),
        "roles.code_number" => Some(theme.roles.code_number.to_string()),
        "roles.code_type" => Some(theme.roles.code_type.to_string()),
        "roles.code_function" => Some(theme.roles.code_function.to_string()),
        "roles.code_constant" => Some(theme.roles.code_constant.to_string()),
        "roles.code_macro" => Some(theme.roles.code_macro.to_string()),
        "roles.code_punctuation" => Some(theme.roles.code_punctuation.to_string()),
        "roles.code_variable" => Some(theme.roles.code_variable.to_string()),
        "roles.code_property" => Some(theme.roles.code_property.to_string()),
        "roles.code_attribute" => Some(theme.roles.code_attribute.to_string()),
        "roles.code_module" => Some(theme.roles.code_module.to_string()),
        "roles.code_label" => Some(theme.roles.code_label.to_string()),
        "roles.code_tag" => Some(theme.roles.code_tag.to_string()),
        "roles.code_embedded" => Some(theme.roles.code_embedded.to_string()),
        _ => None,
    }
}

fn set_role_value(
    theme: &mut codex_core::themes::ThemeDefinition,
    key: &'static str,
    value: &str,
) -> std::result::Result<(), String> {
    use codex_core::themes::ThemeColor;

    match key {
        "roles.transcript_bg" => {
            if value.trim().is_empty() {
                theme.roles.transcript_bg = None;
            } else {
                theme.roles.transcript_bg = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        "roles.composer_bg" => {
            if value.trim().is_empty() {
                theme.roles.composer_bg = None;
            } else {
                theme.roles.composer_bg = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        "roles.user_prompt_highlight_bg" => {
            if value.trim().is_empty() {
                theme.roles.user_prompt_highlight_bg = None;
            } else {
                theme.roles.user_prompt_highlight_bg = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        "roles.status_bg" => {
            if value.trim().is_empty() {
                theme.roles.status_bg = None;
            } else {
                theme.roles.status_bg = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        "roles.badge" => {
            if value.trim().is_empty() {
                theme.roles.badge = None;
            } else {
                theme.roles.badge = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        "roles.link" => {
            if value.trim().is_empty() {
                theme.roles.link = None;
            } else {
                theme.roles.link = Some(ThemeColor::new(value.trim()));
            }
            return Ok(());
        }
        _ => {}
    }

    if value.trim().is_empty() {
        return Err("Value cannot be empty (use `inherit` or a hex color).".to_string());
    }

    let value = value.trim();
    let dst: &mut ThemeColor = match key {
        "roles.fg" => &mut theme.roles.fg,
        "roles.bg" => &mut theme.roles.bg,
        "roles.selection_fg" => &mut theme.roles.selection_fg,
        "roles.selection_bg" => &mut theme.roles.selection_bg,
        "roles.cursor_fg" => &mut theme.roles.cursor_fg,
        "roles.cursor_bg" => &mut theme.roles.cursor_bg,
        "roles.border" => &mut theme.roles.border,
        "roles.accent" => &mut theme.roles.accent,
        "roles.brand" => &mut theme.roles.brand,
        "roles.command" => &mut theme.roles.command,
        "roles.success" => &mut theme.roles.success,
        "roles.warning" => &mut theme.roles.warning,
        "roles.error" => &mut theme.roles.error,
        "roles.diff_add_fg" => &mut theme.roles.diff_add_fg,
        "roles.diff_add_bg" => &mut theme.roles.diff_add_bg,
        "roles.diff_del_fg" => &mut theme.roles.diff_del_fg,
        "roles.diff_del_bg" => &mut theme.roles.diff_del_bg,
        "roles.diff_hunk_fg" => &mut theme.roles.diff_hunk_fg,
        "roles.diff_hunk_bg" => &mut theme.roles.diff_hunk_bg,
        "roles.code_keyword" => &mut theme.roles.code_keyword,
        "roles.code_operator" => &mut theme.roles.code_operator,
        "roles.code_comment" => &mut theme.roles.code_comment,
        "roles.code_string" => &mut theme.roles.code_string,
        "roles.code_number" => &mut theme.roles.code_number,
        "roles.code_type" => &mut theme.roles.code_type,
        "roles.code_function" => &mut theme.roles.code_function,
        "roles.code_constant" => &mut theme.roles.code_constant,
        "roles.code_macro" => &mut theme.roles.code_macro,
        "roles.code_punctuation" => &mut theme.roles.code_punctuation,
        "roles.code_variable" => &mut theme.roles.code_variable,
        "roles.code_property" => &mut theme.roles.code_property,
        "roles.code_attribute" => &mut theme.roles.code_attribute,
        "roles.code_module" => &mut theme.roles.code_module,
        "roles.code_label" => &mut theme.roles.code_label,
        "roles.code_tag" => &mut theme.roles.code_tag,
        "roles.code_embedded" => &mut theme.roles.code_embedded,
        _ => return Err("Unknown role key.".to_string()),
    };
    dst.set(value.to_string());
    Ok(())
}

fn palette_value(theme: &codex_core::themes::ThemeDefinition, key: &'static str) -> String {
    match key {
        "palette.black" => theme.palette.black.to_string(),
        "palette.red" => theme.palette.red.to_string(),
        "palette.green" => theme.palette.green.to_string(),
        "palette.yellow" => theme.palette.yellow.to_string(),
        "palette.blue" => theme.palette.blue.to_string(),
        "palette.magenta" => theme.palette.magenta.to_string(),
        "palette.cyan" => theme.palette.cyan.to_string(),
        "palette.white" => theme.palette.white.to_string(),
        "palette.bright_black" => theme.palette.bright_black.to_string(),
        "palette.bright_red" => theme.palette.bright_red.to_string(),
        "palette.bright_green" => theme.palette.bright_green.to_string(),
        "palette.bright_yellow" => theme.palette.bright_yellow.to_string(),
        "palette.bright_blue" => theme.palette.bright_blue.to_string(),
        "palette.bright_magenta" => theme.palette.bright_magenta.to_string(),
        "palette.bright_cyan" => theme.palette.bright_cyan.to_string(),
        "palette.bright_white" => theme.palette.bright_white.to_string(),
        _ => "inherit".to_string(),
    }
}

fn set_palette_value(
    theme: &mut codex_core::themes::ThemeDefinition,
    key: &'static str,
    value: &str,
) {
    let value = value.trim();
    let dst: &mut codex_core::themes::ThemeColor = match key {
        "palette.black" => &mut theme.palette.black,
        "palette.red" => &mut theme.palette.red,
        "palette.green" => &mut theme.palette.green,
        "palette.yellow" => &mut theme.palette.yellow,
        "palette.blue" => &mut theme.palette.blue,
        "palette.magenta" => &mut theme.palette.magenta,
        "palette.cyan" => &mut theme.palette.cyan,
        "palette.white" => &mut theme.palette.white,
        "palette.bright_black" => &mut theme.palette.bright_black,
        "palette.bright_red" => &mut theme.palette.bright_red,
        "palette.bright_green" => &mut theme.palette.bright_green,
        "palette.bright_yellow" => &mut theme.palette.bright_yellow,
        "palette.bright_blue" => &mut theme.palette.bright_blue,
        "palette.bright_magenta" => &mut theme.palette.bright_magenta,
        "palette.bright_cyan" => &mut theme.palette.bright_cyan,
        "palette.bright_white" => &mut theme.palette.bright_white,
        _ => return,
    };
    if value.is_empty() {
        dst.set("inherit".to_string());
    } else {
        dst.set(value.to_string());
    }
}

fn sample_code_tabs_line(active: SampleCodeTab) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        "Sample code: ".bold(),
        "(Shift+Tab)".dim(),
        " ".into(),
    ];

    for tab in SAMPLE_CODE_TABS {
        let is_active = tab.fence_lang == active.fence_lang;
        let style = if is_active {
            crate::theme::accent_style().add_modifier(Modifier::BOLD)
        } else {
            crate::theme::transcript_dim_style()
        };
        let label = tab.label;
        spans.push(Span::from(format!("[{label}]")).set_style(style));
        spans.push(" ".into());
    }

    Line::from(spans).style(crate::theme::transcript_style())
}

fn sample_diff_for_tab(active: SampleCodeTab) -> (PathBuf, String) {
    match active.fence_lang {
        "python" => (
            PathBuf::from("preview/sample.py"),
            "--- a/preview/sample.py\n+++ b/preview/sample.py\n@@ -1,3 +1,3 @@\n-# Note: type hints keep the gremlins calm.\n+# Note: type hints keep the gremlins fed.\n def greet(name: str) -> str:\n-    return f\"hello, {name}.\"\n+    return f\"hello, {name}!\"\n".to_string(),
        ),
        "javascript" => (
            PathBuf::from("preview/sample.js"),
            "--- a/preview/sample.js\n+++ b/preview/sample.js\n@@ -1,4 +1,4 @@\n-// Note: this function is 99% vibes, 1% types.\n+// Note: this function is 99% vibes, 1% semicolons.\n function greet(name) {\n-  return `Hello, ${name}.`;\n+  return `Hello, ${name}!`;\n }\n".to_string(),
        ),
        "typescript" => (
            PathBuf::from("preview/sample.ts"),
            "--- a/preview/sample.ts\n+++ b/preview/sample.ts\n@@ -1,4 +1,4 @@\n-// Note: strict mode demands a tribute.\n+// Note: strict mode demands two tributes.\n function greet(name: string): string {\n-  return `Hello, ${name}.`;\n+  return `Hello, ${name}!`;\n }\n".to_string(),
        ),
        _ => (
            PathBuf::from("preview/sample.rs"),
            "--- a/preview/sample.rs\n+++ b/preview/sample.rs\n@@ -1,4 +1,4 @@\n-// Note: appease the borrow checker with snacks.\n+// Note: appease the borrow checker with more snacks.\n fn greet(name: &str) -> String {\n-    format!(\"Hello, {name}.\")\n+    format!(\"Hello, {name}!\")\n }\n".to_string(),
        ),
    }
}

fn suggested_theme_name(base_theme_name: &str) -> String {
    if base_theme_name == "default" {
        "my-theme".to_string()
    } else {
        format!("{base_theme_name}-custom")
    }
}

fn unique_theme_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let mut slug = String::new();
    for ch in name.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
        } else if matches!(lower, '-' | '_' | ' ') && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let base = if slug.is_empty() { "theme" } else { slug };

    for idx in 0.. {
        let filename = if idx == 0 {
            format!("{base}.yaml")
        } else {
            format!("{base}-{idx}.yaml")
        };
        let path = dir.join(filename);
        if !path.exists() {
            return path;
        }
    }
    unreachable!()
}
