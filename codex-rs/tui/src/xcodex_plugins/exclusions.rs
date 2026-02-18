use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::FileSearchPopup;
use crate::bottom_pane::TextArea;
use crate::bottom_pane::TextAreaState;
use crate::chatwidget::ChatWidget;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;
use codex_core::config::types::ExclusionConfig;
use codex_core::config::types::ExclusionOnMatch;
use codex_core::config::types::LogRedactionsMode;
use codex_file_search as file_search;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Styled as _;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

pub(crate) fn handle_exclusions_command(chat: &mut ChatWidget, _rest: &str) -> bool {
    open_exclusions_menu(chat);
    true
}

pub(crate) fn open_exclusions_menu(chat: &mut ChatWidget) {
    open_exclusions_menu_at(chat, ExclusionTab::Presets.index(), None);
}

pub(crate) fn open_exclusions_menu_at(
    chat: &mut ChatWidget,
    tab: usize,
    selected_idx: Option<usize>,
) {
    let tab = ExclusionTab::from_index(tab);
    let config = chat.config_ref();
    let search_dir = chat.session_cwd().to_path_buf();
    let view = ExclusionsSettingsView::new(
        config.exclusion.clone(),
        config.exclusion.layer_hook_sanitization_enabled(),
        tab,
        selected_idx,
        search_dir,
        chat.app_event_tx(),
    );
    chat.show_view(Box::new(view));
}

struct ExclusionRow {
    label: String,
    hint: String,
    action: Option<ExclusionAction>,
    checked: Option<bool>,
    is_dimmed: bool,
}

const MAX_FILE_SEARCH_RESULTS: NonZeroUsize = NonZeroUsize::new(20).unwrap();
const NUM_FILE_SEARCH_THREADS: NonZeroUsize = NonZeroUsize::new(2).unwrap();

struct ExclusionFileSearch {
    state: Arc<Mutex<FileSearchState>>,
    search_dir: PathBuf,
}

struct FileSearchState {
    popup: FileSearchPopup,
    latest_query: String,
    session: Option<file_search::FileSearchSession>,
    session_token: usize,
    ignore_filenames: Vec<String>,
}

impl ExclusionFileSearch {
    fn new(search_dir: PathBuf, ignore_filenames: Vec<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FileSearchState {
                popup: FileSearchPopup::new(),
                latest_query: String::new(),
                session: None,
                session_token: 0,
                ignore_filenames,
            })),
            search_dir,
        }
    }

    fn update_ignore_filenames(&mut self, ignore_filenames: Vec<String>) {
        let mut state = self.state.lock().expect("file search state poisoned");
        if state.ignore_filenames == ignore_filenames {
            return;
        }
        state.ignore_filenames = ignore_filenames;
        state.session = None;
        state.latest_query.clear();
        state.popup.set_empty_prompt();
    }

    fn on_user_query(&mut self, query: &str) {
        let mut state = self.state.lock().expect("file search state poisoned");
        if query == state.latest_query {
            return;
        }
        state.latest_query.clear();
        state.latest_query.push_str(query);

        if query.is_empty() {
            state.session = None;
            state.popup.set_empty_prompt();
            return;
        }

        if state.session.is_none() {
            self.start_session_locked(&mut state);
        }
        if let Some(session) = state.session.as_ref() {
            session.update_query(query);
        }
        state.popup.set_query(query);
    }

    fn start_session_locked(&self, state: &mut FileSearchState) {
        state.session_token = state.session_token.wrapping_add(1);
        let session_token = state.session_token;
        let reporter = Arc::new(FileSearchReporter {
            state: self.state.clone(),
            session_token,
        });
        let session = file_search::create_session(
            vec![self.search_dir.clone()],
            file_search::FileSearchOptions {
                limit: MAX_FILE_SEARCH_RESULTS,
                exclude: Vec::new(),
                ignore_filenames: state.ignore_filenames.clone(),
                threads: NUM_FILE_SEARCH_THREADS,
                compute_indices: true,
                respect_gitignore: true,
            },
            reporter,
            None,
        );
        match session {
            Ok(session) => state.session = Some(session),
            Err(err) => {
                tracing::warn!("file search session failed to start: {err}");
                state.session = None;
            }
        }
    }

    fn set_empty_prompt(&mut self) {
        let mut state = self.state.lock().expect("file search state poisoned");
        state.popup.set_empty_prompt();
    }

    fn move_up(&mut self) {
        let mut state = self.state.lock().expect("file search state poisoned");
        state.popup.move_up();
    }

    fn move_down(&mut self) {
        let mut state = self.state.lock().expect("file search state poisoned");
        state.popup.move_down();
    }

    fn selected_match(&self) -> Option<String> {
        let state = self.state.lock().expect("file search state poisoned");
        state
            .popup
            .selected_match()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn with_popup<F>(&self, mut f: F)
    where
        F: FnMut(&FileSearchPopup),
    {
        let state = self.state.lock().expect("file search state poisoned");
        f(&state.popup);
    }
}

struct FileSearchReporter {
    state: Arc<Mutex<FileSearchState>>,
    session_token: usize,
}

impl FileSearchReporter {
    fn send_snapshot(&self, snapshot: &file_search::FileSearchSnapshot) {
        let mut state = self.state.lock().expect("file search state poisoned");
        if state.session_token != self.session_token
            || state.latest_query.is_empty()
            || snapshot.query.is_empty()
        {
            return;
        }
        state
            .popup
            .set_matches(&snapshot.query, snapshot.matches.clone());
    }
}

impl file_search::SessionReporter for FileSearchReporter {
    fn on_update(&self, snapshot: &file_search::FileSearchSnapshot) {
        self.send_snapshot(snapshot);
    }

    fn on_complete(&self) {}
}

#[derive(Clone, Copy)]
enum ExclusionAction {
    Toggle(Toggle),
    Preset(Preset),
    LogMode(LogRedactionsMode),
    EditAllowlist,
    EditBlocklist,
    EditIgnoreFiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditMode {
    Allowlist,
    Blocklist,
    IgnoreFiles,
}

pub(crate) struct ExclusionsSettingsView {
    tab: ExclusionTab,
    selected_row: usize,
    exclusion: ExclusionConfig,
    hooks_sanitize_payloads: bool,
    edit_mode: Option<EditMode>,
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    search_dir: PathBuf,
    file_search: Option<ExclusionFileSearch>,
    file_search_active: bool,
    complete: bool,
    app_event_tx: AppEventSender,
}

impl ExclusionsSettingsView {
    fn new(
        exclusion: ExclusionConfig,
        hooks_sanitize_payloads: bool,
        tab: ExclusionTab,
        selected_idx: Option<usize>,
        search_dir: PathBuf,
        app_event_tx: AppEventSender,
    ) -> Self {
        let mut view = Self {
            tab,
            selected_row: selected_idx.unwrap_or(0),
            exclusion,
            hooks_sanitize_payloads,
            edit_mode: None,
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            search_dir,
            file_search: None,
            file_search_active: false,
            complete: false,
            app_event_tx,
        };
        view.clamp_selected_row();
        view
    }

    fn footer_hint_line() -> Line<'static> {
        let key_style = crate::theme::accent_style().add_modifier(Modifier::BOLD);
        let hint_style = crate::theme::dim_style();
        vec![
            Span::from("Tab").set_style(key_style),
            Span::from(": switch section").set_style(hint_style),
            "  ".into(),
            Span::from("↑/↓").set_style(key_style),
            Span::from(": select").set_style(hint_style),
            "  ".into(),
            Span::from("Space/Enter").set_style(key_style),
            Span::from(": toggle").set_style(hint_style),
            "  ".into(),
            Span::from("Esc").set_style(key_style),
            Span::from(": close").set_style(hint_style),
        ]
        .into()
    }

    fn tab_line(&self) -> Line<'static> {
        let active_style = crate::theme::accent_style().add_modifier(Modifier::BOLD);
        let inactive_style = crate::theme::dim_style();
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (idx, tab) in ExclusionTab::ALL.iter().copied().enumerate() {
            let label = format!("[ {} ]", tab.name());
            let style = if tab == self.tab {
                active_style
            } else {
                inactive_style
            };
            spans.push(Span::from(label).set_style(style));
            if idx + 1 < ExclusionTab::ALL.len() {
                spans.push("  ".into());
            }
        }
        spans.into()
    }

    fn header_lines(&self) -> Vec<Line<'static>> {
        vec![
            Line::from("Exclusions".bold()),
            self.tab_line(),
            Line::from(""),
            Line::from(
                Span::from("Tab to switch sections, space to toggle.")
                    .set_style(crate::theme::dim_style()),
            ),
            Line::from(""),
        ]
    }

    fn rows_for_tab(&self) -> Vec<ExclusionRow> {
        match self.tab {
            ExclusionTab::Presets => self.build_preset_rows(),
            ExclusionTab::Layers => self.build_layer_rows(),
            ExclusionTab::Patterns => self.build_pattern_rows(),
            ExclusionTab::Logging => self.build_logging_rows(),
            ExclusionTab::Files => self.build_files_rows(),
        }
    }

    fn build_layer_rows(&self) -> Vec<ExclusionRow> {
        let exclusions_enabled = self.exclusion.enabled;
        let path_matching_enabled = exclusions_enabled && self.exclusion.path_matching;
        let toggles = [
            (
                "Enabled",
                Toggle::Enabled,
                "Enable exclusions (persists to config).",
                false,
            ),
            (
                "Paranoid mode",
                Toggle::ParanoidMode,
                "Enable paranoid-only layers (L2 + L4).",
                !exclusions_enabled,
            ),
            (
                "Path matching (L1)",
                Toggle::PathMatching,
                "Block excluded file paths on reads and listings.",
                !exclusions_enabled,
            ),
            (
                "  └ Shell preflight (L1)",
                Toggle::PreflightShellPaths,
                "Block shell commands that reference excluded paths.",
                !path_matching_enabled,
            ),
            (
                "Output sanitization (L2)",
                Toggle::LayerOutput,
                "Redact tool outputs before they reach the model.",
                !exclusions_enabled,
            ),
            (
                "Send firewall (L3)",
                Toggle::LayerSend,
                "Block excluded file payloads from being sent.",
                !exclusions_enabled,
            ),
            (
                "Request interceptor (L4)",
                Toggle::LayerRequest,
                "Scan the full prompt payload before sending.",
                !exclusions_enabled,
            ),
            (
                "Hook payload sanitizer (L5)",
                Toggle::HooksPayloads,
                "Redact hook payload strings before dispatch.",
                !exclusions_enabled,
            ),
        ];

        toggles
            .iter()
            .map(|(label, toggle, hint, disabled)| ExclusionRow {
                label: (*label).to_string(),
                hint: (*hint).to_string(),
                action: (!disabled).then_some(ExclusionAction::Toggle(*toggle)),
                checked: Some(if !disabled {
                    toggle.is_enabled(&self.exclusion, self.hooks_sanitize_payloads)
                } else {
                    false
                }),
                is_dimmed: *disabled,
            })
            .collect()
    }

    fn build_preset_rows(&self) -> Vec<ExclusionRow> {
        let presets = [
            (
                "Allow all (exclusions off)",
                Preset::AllowAll,
                "Disable exclusion matching and redaction.",
            ),
            (
                "Block all (no prompts)",
                Preset::BlockAll,
                "Block excluded paths and content without asking.",
            ),
            (
                "Ask and allow",
                Preset::AskAndAllow,
                "Prompt before accessing excluded paths.",
            ),
        ];

        let active_preset = presets.iter().find_map(|(_, preset, _)| {
            preset
                .matches(&self.exclusion, self.hooks_sanitize_payloads)
                .then_some(*preset)
        });
        let dim_inactive = active_preset.is_some();

        presets
            .iter()
            .map(|(label, preset, hint)| {
                let is_current = preset.matches(&self.exclusion, self.hooks_sanitize_payloads);
                ExclusionRow {
                    label: (*label).to_string(),
                    hint: (*hint).to_string(),
                    action: Some(ExclusionAction::Preset(*preset)),
                    checked: Some(is_current),
                    is_dimmed: dim_inactive && !is_current,
                }
            })
            .collect()
    }

    fn build_pattern_rows(&self) -> Vec<ExclusionRow> {
        let mut rows = Vec::new();
        rows.push(ExclusionRow {
            label: format!(
                "Edit allowlist ({} entries)",
                self.exclusion.secret_patterns_allowlist.len()
            ),
            hint: "Add patterns that should bypass exclusion filtering (one per line).".to_string(),
            action: Some(ExclusionAction::EditAllowlist),
            checked: None,
            is_dimmed: false,
        });
        rows.push(ExclusionRow {
            label: format!(
                "Edit blocklist ({} entries)",
                self.exclusion.secret_patterns_blocklist.len()
            ),
            hint: "Add extra secret patterns to scan and block/redact (one per line).".to_string(),
            action: Some(ExclusionAction::EditBlocklist),
            checked: None,
            is_dimmed: false,
        });

        let toggles = [
            (
                "Content hashing",
                Toggle::ContentHashing,
                "Cache content gateway decisions by hashing text.",
            ),
            (
                "Substring matching",
                Toggle::SubstringMatching,
                "Detect ignored paths referenced in text.",
            ),
            (
                "Secret patterns",
                Toggle::SecretPatterns,
                "Scan for secret patterns in text.",
            ),
            (
                "Built-in secret patterns",
                Toggle::SecretPatternsBuiltin,
                "Include built-in secret regexes in scans.",
            ),
        ];

        rows.extend(toggles.iter().map(|(label, toggle, hint)| ExclusionRow {
            label: (*label).to_string(),
            hint: (*hint).to_string(),
            action: Some(ExclusionAction::Toggle(*toggle)),
            checked: Some(toggle.is_enabled(&self.exclusion, self.hooks_sanitize_payloads)),
            is_dimmed: false,
        }));

        rows
    }

    fn build_logging_rows(&self) -> Vec<ExclusionRow> {
        let mut rows = Vec::new();
        let modes = [
            (
                "Redaction log: Off",
                LogRedactionsMode::Off,
                "Disable redaction logging.",
            ),
            (
                "Redaction log: Summary",
                LogRedactionsMode::Summary,
                "Log a summary without payloads.",
            ),
            (
                "Redaction log: Raw",
                LogRedactionsMode::Raw,
                "Log full payload redactions.",
            ),
        ];

        rows.extend(modes.iter().map(|(label, mode, hint)| ExclusionRow {
            label: (*label).to_string(),
            hint: (*hint).to_string(),
            action: Some(ExclusionAction::LogMode(*mode)),
            checked: Some(self.exclusion.log_redactions == *mode),
            is_dimmed: false,
        }));

        let toggles = [
            (
                "Summary banner",
                Toggle::ShowSummaryBanner,
                "Show an inline banner when exclusions apply.",
            ),
            (
                "Summary history",
                Toggle::ShowSummaryHistory,
                "Store exclusion summaries in the transcript.",
            ),
            (
                "Approval prompt: reveal matched values",
                Toggle::PromptRevealSecretMatches,
                "Show full secret-pattern matches by default in exclusion prompts (may display secrets).",
            ),
        ];

        rows.extend(toggles.iter().map(|(label, toggle, hint)| ExclusionRow {
            label: (*label).to_string(),
            hint: (*hint).to_string(),
            action: Some(ExclusionAction::Toggle(*toggle)),
            checked: Some(toggle.is_enabled(&self.exclusion, self.hooks_sanitize_payloads)),
            is_dimmed: false,
        }));

        rows
    }

    fn build_files_rows(&self) -> Vec<ExclusionRow> {
        let mut rows = Vec::new();
        rows.push(ExclusionRow {
            label: format!("Edit ignore files ({} entries)", self.exclusion.files.len()),
            hint: "Add or remove ignore files (one per line).".to_string(),
            action: Some(ExclusionAction::EditIgnoreFiles),
            checked: None,
            is_dimmed: false,
        });

        if self.exclusion.files.is_empty() {
            rows.push(ExclusionRow {
                label: "No ignore files configured.".to_string(),
                hint: "Add exclusions files to populate this list.".to_string(),
                action: None,
                checked: None,
                is_dimmed: true,
            });
            return rows;
        }

        rows.extend(self.exclusion.files.iter().map(|file| ExclusionRow {
            label: file.clone(),
            hint: "Ignore file loaded for exclusions.".to_string(),
            action: None,
            checked: None,
            is_dimmed: true,
        }));

        rows
    }

    fn row_count(&self) -> usize {
        self.rows_for_tab().len().max(1)
    }

    fn clamp_selected_row(&mut self) {
        let max = self.row_count().saturating_sub(1);
        self.selected_row = self.selected_row.min(max);
    }

    fn switch_tab(&mut self) {
        self.tab = self.tab.next();
        self.selected_row = 0;
        self.clamp_selected_row();
    }

    fn move_up(&mut self) {
        if self.selected_row > 0 {
            self.selected_row = self.selected_row.saturating_sub(1);
        }
    }

    fn move_down(&mut self) {
        let max = self.row_count().saturating_sub(1);
        self.selected_row = (self.selected_row + 1).min(max);
    }

    fn apply_update(&mut self, next: NextSettings) {
        let mut exclusion = next.exclusion.clone();
        exclusion.layer_hook_sanitization = Some(next.hooks_sanitize_payloads);
        self.exclusion = exclusion.clone();
        self.hooks_sanitize_payloads = next.hooks_sanitize_payloads;
        if let Some(file_search) = self.file_search.as_mut() {
            file_search.update_ignore_filenames(self.exclusion.files.clone());
        }
        self.app_event_tx.send(AppEvent::UpdateExclusionSettings {
            exclusion: exclusion.clone(),
            hooks_sanitize_payloads: next.hooks_sanitize_payloads,
        });
        self.app_event_tx.send(AppEvent::PersistExclusionSettings {
            exclusion,
            hooks_sanitize_payloads: next.hooks_sanitize_payloads,
        });
    }

    fn apply_action(&mut self, action: ExclusionAction) {
        match action {
            ExclusionAction::Toggle(toggle) => {
                let next =
                    apply_toggle_rules(&self.exclusion, self.hooks_sanitize_payloads, toggle);
                self.apply_update(next);
            }
            ExclusionAction::Preset(preset) => {
                let next = preset.apply(&self.exclusion, self.hooks_sanitize_payloads);
                self.apply_update(next);
            }
            ExclusionAction::LogMode(mode) => {
                if self.exclusion.log_redactions == mode {
                    return;
                }
                let mut exclusion = self.exclusion.clone();
                exclusion.log_redactions = mode;
                self.apply_update(NextSettings {
                    exclusion,
                    hooks_sanitize_payloads: self.hooks_sanitize_payloads,
                });
            }
            ExclusionAction::EditAllowlist => self.start_edit(EditMode::Allowlist),
            ExclusionAction::EditBlocklist => self.start_edit(EditMode::Blocklist),
            ExclusionAction::EditIgnoreFiles => self.start_edit(EditMode::IgnoreFiles),
        }
    }

    fn start_edit(&mut self, mode: EditMode) {
        self.edit_mode = Some(mode);
        let seed = match mode {
            EditMode::Allowlist => self.exclusion.secret_patterns_allowlist.join("\n"),
            EditMode::Blocklist => self.exclusion.secret_patterns_blocklist.join("\n"),
            EditMode::IgnoreFiles => self.exclusion.files.join("\n"),
        };
        self.textarea.set_text_clearing_elements(&seed);
        self.textarea_state.replace(TextAreaState::default());
        self.update_file_search();
    }

    fn cancel_edit(&mut self) {
        self.edit_mode = None;
        self.file_search_active = false;
    }

    fn commit_edit(&mut self) {
        let mut items: Vec<String> = self
            .textarea
            .text()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        items.sort();
        items.dedup();

        let mut exclusion = self.exclusion.clone();
        match self.edit_mode {
            Some(EditMode::Allowlist) => exclusion.secret_patterns_allowlist = items,
            Some(EditMode::Blocklist) => exclusion.secret_patterns_blocklist = items,
            Some(EditMode::IgnoreFiles) => exclusion.files = items,
            None => {}
        }

        self.apply_update(NextSettings {
            exclusion,
            hooks_sanitize_payloads: self.hooks_sanitize_payloads,
        });
        self.edit_mode = None;
        self.file_search_active = false;
    }

    fn update_file_search(&mut self) {
        if self.edit_mode != Some(EditMode::IgnoreFiles) {
            self.file_search_active = false;
            return;
        }

        let Some(query) = Self::current_at_token(&self.textarea) else {
            self.file_search_active = false;
            return;
        };

        if self.file_search.is_none() {
            self.file_search = Some(ExclusionFileSearch::new(
                self.search_dir.clone(),
                self.exclusion.files.clone(),
            ));
        }
        let Some(file_search) = self.file_search.as_mut() else {
            self.file_search_active = false;
            return;
        };

        if query.is_empty() {
            file_search.set_empty_prompt();
        } else {
            file_search.on_user_query(&query);
        }
        self.file_search_active = true;
    }

    fn current_at_token(textarea: &TextArea) -> Option<String> {
        let cursor_offset = textarea.cursor();
        let text = textarea.text();
        let safe_cursor = Self::clamp_to_char_boundary(text, cursor_offset);
        let before_cursor = &text[..safe_cursor];
        let after_cursor = &text[safe_cursor..];

        let start_idx = before_cursor
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        let end_rel_idx = after_cursor
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(after_cursor.len());
        let end_idx = safe_cursor + end_rel_idx;
        let token = text.get(start_idx..end_idx)?;
        if !token.starts_with('@') {
            return None;
        }
        Some(token.trim_start_matches('@').to_string())
    }

    fn insert_selected_path(&mut self, path: &str) {
        let cursor_offset = self.textarea.cursor();
        let text = self.textarea.text();
        let safe_cursor = Self::clamp_to_char_boundary(text, cursor_offset);
        let before_cursor = &text[..safe_cursor];
        let after_cursor = &text[safe_cursor..];

        let start_idx = before_cursor
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        let end_rel_idx = after_cursor
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(after_cursor.len());
        let end_idx = safe_cursor + end_rel_idx;

        let needs_quotes = path.chars().any(char::is_whitespace);
        let inserted = if needs_quotes && !path.contains('"') {
            format!("\"{path}\"")
        } else {
            path.to_string()
        };

        let mut new_text =
            String::with_capacity(text.len() - (end_idx - start_idx) + inserted.len() + 1);
        new_text.push_str(&text[..start_idx]);
        new_text.push_str(&inserted);
        new_text.push(' ');
        new_text.push_str(&text[end_idx..]);
        self.textarea.set_text_clearing_elements(&new_text);
        let new_cursor = start_idx.saturating_add(inserted.len()).saturating_add(1);
        self.textarea.set_cursor(new_cursor);
    }

    fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
        if cursor >= text.len() {
            return text.len();
        }
        if text.is_char_boundary(cursor) {
            return cursor;
        }
        let mut pos = cursor;
        while pos > 0 && !text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    fn selected_hint_line(&self) -> Option<Line<'static>> {
        let rows = self.rows_for_tab();
        let row = rows.get(self.selected_row)?;
        if row.hint.is_empty() {
            return None;
        }
        Some(
            Span::from(row.hint.clone())
                .set_style(crate::theme::dim_style())
                .into(),
        )
    }

    fn body_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let rows = self.rows_for_tab();

        let selected_prefix = |selected: bool| -> Span<'static> {
            if selected {
                Span::from("› ")
                    .set_style(crate::theme::accent_style().add_modifier(Modifier::BOLD))
            } else {
                "  ".into()
            }
        };

        let checkbox = |enabled: Option<bool>, dimmed: bool| -> Span<'static> {
            match enabled {
                Some(true) => Span::from("[x] ").set_style(crate::theme::success_style()),
                Some(false) => Span::from("[ ] ").set_style(crate::theme::dim_style()),
                None => Span::from("    ").set_style(if dimmed {
                    crate::theme::dim_style()
                } else {
                    crate::theme::dim_style()
                }),
            }
        };

        for (idx, row) in rows.iter().enumerate() {
            let selected = idx == self.selected_row;
            let label = if row.is_dimmed {
                Span::from(row.label.clone()).set_style(crate::theme::dim_style())
            } else {
                Span::from(row.label.clone())
            };
            lines.push(
                vec![
                    selected_prefix(selected),
                    checkbox(row.checked, row.is_dimmed),
                    label,
                ]
                .into(),
            );
        }

        if let Some(hint) = self.selected_hint_line() {
            lines.push(Line::from(""));
            lines.push(hint);
        }

        lines
    }
}

impl BottomPaneView for ExclusionsSettingsView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.edit_mode.is_some() {
            match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.cancel_edit(),
                KeyEvent {
                    code: KeyCode::Up, ..
                } if self.file_search_active => {
                    if let Some(file_search) = self.file_search.as_mut() {
                        file_search.move_up();
                    }
                }
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                } if self.file_search_active => {
                    if let Some(file_search) = self.file_search.as_mut() {
                        file_search.move_down();
                    }
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => {
                    if self.file_search_active
                        && let Some(file_search) = self.file_search.as_ref()
                        && let Some(path) = file_search.selected_match()
                    {
                        self.insert_selected_path(&path);
                        self.update_file_search();
                        return;
                    }
                    self.commit_edit();
                }
                other => {
                    self.textarea.input(other);
                    self.update_file_search();
                }
            }
            return;
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.switch_tab(),
            KeyEvent {
                code: KeyCode::BackTab,
                ..
            } => {
                self.tab = self.tab.prev();
                self.selected_row = 0;
                self.clamp_selected_row();
            }
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_up(),
            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_down(),
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                let rows = self.rows_for_tab();
                if let Some(row) = rows.get(self.selected_row)
                    && let Some(action) = row.action
                {
                    self.apply_action(action);
                }
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.complete = true;
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

impl Renderable for ExclusionsSettingsView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let footer_hint = if self.edit_mode.is_some() {
            vec![
                "Enter".cyan(),
                " to save, ".dim(),
                "Esc".cyan(),
                " to cancel".dim(),
            ]
            .into()
        } else {
            Self::footer_hint_line()
        };
        let [content_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        let base_style = user_message_style().patch(crate::theme::composer_style());
        Block::default().style(base_style).render(content_area, buf);
        Block::default().style(base_style).render(footer_area, buf);

        let inner_area = content_area.inset(Insets::vh(1, 2));
        if let Some(mode) = self.edit_mode {
            let title = match mode {
                EditMode::Allowlist => "Exclusions — Edit allowlist",
                EditMode::Blocklist => "Exclusions — Edit blocklist",
                EditMode::IgnoreFiles => "Exclusions — Edit ignore files",
            };
            let mut header_lines = vec![
                Line::from(title.bold()),
                Line::from(""),
                Line::from("One entry per line.".dim()),
            ];
            if mode == EditMode::IgnoreFiles {
                header_lines.push(Line::from("Type @ to search files.".dim()));
            }
            let header_height = u16::try_from(header_lines.len())
                .unwrap_or(0)
                .min(inner_area.height);
            let [header_area, body_area] =
                Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)])
                    .areas(inner_area);
            Paragraph::new(header_lines)
                .style(base_style)
                .render(header_area, buf);

            let block = Block::default().style(base_style);
            block.render(body_area, buf);
            let mut popup_height = 0;
            if self.file_search_active
                && let Some(file_search) = self.file_search.as_ref()
            {
                file_search.with_popup(|popup| {
                    popup_height = popup.calculate_required_height();
                });
            }
            let [input_area, popup_area] = if popup_height > 0 {
                Layout::vertical([Constraint::Min(3), Constraint::Length(popup_height)])
                    .areas(body_area)
            } else {
                [body_area, Rect::default()]
            };
            StatefulWidgetRef::render_ref(
                &(&self.textarea),
                input_area.inset(Insets::vh(0, 0)),
                buf,
                &mut self.textarea_state.borrow_mut(),
            );
            if popup_height > 0
                && let Some(file_search) = self.file_search.as_ref()
            {
                file_search.with_popup(|popup| {
                    popup.render_ref(popup_area, buf);
                });
            }
        } else {
            let header_lines = self.header_lines();
            let header_height = u16::try_from(header_lines.len())
                .unwrap_or(0)
                .min(inner_area.height);
            let [header_area, body_area] =
                Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)])
                    .areas(inner_area);

            Paragraph::new(header_lines)
                .style(base_style)
                .render(header_area, buf);

            let body_lines = self.body_lines();
            Paragraph::new(body_lines)
                .style(base_style)
                .render(body_area, buf);
        }

        let hint_area = Rect {
            x: footer_area.x + 2,
            y: footer_area.y,
            width: footer_area.width.saturating_sub(2),
            height: footer_area.height,
        };
        Paragraph::new(footer_hint)
            .style(base_style)
            .render(hint_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.edit_mode.is_some() {
            return 12.min(width.max(1) * 10);
        }
        let max_height = 22;
        let header_height = u16::try_from(self.header_lines().len()).unwrap_or(0);
        let body_height = u16::try_from(self.body_lines().len()).unwrap_or(0);
        let min_height = header_height.saturating_add(body_height).saturating_add(3);
        let measured = min_height.min(max_height);
        measured.min(width.max(1) * 10)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExclusionTab {
    Presets,
    Layers,
    Patterns,
    Logging,
    Files,
}

impl ExclusionTab {
    const ALL: [ExclusionTab; 5] = [
        ExclusionTab::Presets,
        ExclusionTab::Layers,
        ExclusionTab::Patterns,
        ExclusionTab::Logging,
        ExclusionTab::Files,
    ];

    fn name(self) -> &'static str {
        match self {
            ExclusionTab::Presets => "Presets",
            ExclusionTab::Layers => "Layers",
            ExclusionTab::Patterns => "Patterns",
            ExclusionTab::Logging => "Logging",
            ExclusionTab::Files => "Files",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    fn from_index(idx: usize) -> Self {
        Self::ALL[idx % Self::ALL.len()]
    }

    fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    fn prev(self) -> Self {
        let idx = self.index();
        if idx == 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[idx - 1]
        }
    }
}

#[derive(Clone, Copy)]
enum Toggle {
    Enabled,
    ParanoidMode,
    PathMatching,
    LayerOutput,
    LayerSend,
    LayerRequest,
    HooksPayloads,
    ContentHashing,
    SubstringMatching,
    SecretPatterns,
    SecretPatternsBuiltin,
    ShowSummaryBanner,
    ShowSummaryHistory,
    PromptRevealSecretMatches,
    PreflightShellPaths,
}

impl Toggle {
    fn is_enabled(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> bool {
        match self {
            Toggle::Enabled => current.enabled,
            Toggle::ParanoidMode => current.paranoid_mode,
            Toggle::PathMatching => current.path_matching,
            Toggle::LayerOutput => current.layer_output_sanitization_enabled(),
            Toggle::LayerSend => current.layer_send_firewall_enabled(),
            Toggle::LayerRequest => current.layer_request_interceptor_enabled(),
            Toggle::HooksPayloads => hooks_sanitize_payloads,
            Toggle::ContentHashing => current.content_hashing,
            Toggle::SubstringMatching => current.substring_matching,
            Toggle::SecretPatterns => current.secret_patterns,
            Toggle::SecretPatternsBuiltin => current.secret_patterns_builtin,
            Toggle::ShowSummaryBanner => current.show_summary_banner,
            Toggle::ShowSummaryHistory => current.show_summary_history,
            Toggle::PromptRevealSecretMatches => current.prompt_reveal_secret_matches,
            Toggle::PreflightShellPaths => current.preflight_shell_paths,
        }
    }

    fn apply(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> NextSettings {
        let mut exclusion = current.clone();
        let mut hooks_sanitize_payloads = hooks_sanitize_payloads;
        match self {
            Toggle::Enabled => exclusion.enabled = !exclusion.enabled,
            Toggle::ParanoidMode => exclusion.paranoid_mode = !exclusion.paranoid_mode,
            Toggle::PathMatching => exclusion.path_matching = !exclusion.path_matching,
            Toggle::LayerOutput => {
                exclusion.layer_output_sanitization =
                    Some(!exclusion.layer_output_sanitization_enabled());
            }
            Toggle::LayerSend => {
                exclusion.layer_send_firewall = Some(!exclusion.layer_send_firewall_enabled());
            }
            Toggle::LayerRequest => {
                exclusion.layer_request_interceptor =
                    Some(!exclusion.layer_request_interceptor_enabled());
            }
            Toggle::HooksPayloads => hooks_sanitize_payloads = !hooks_sanitize_payloads,
            Toggle::ContentHashing => exclusion.content_hashing = !exclusion.content_hashing,
            Toggle::SubstringMatching => {
                exclusion.substring_matching = !exclusion.substring_matching
            }
            Toggle::SecretPatterns => exclusion.secret_patterns = !exclusion.secret_patterns,
            Toggle::SecretPatternsBuiltin => {
                exclusion.secret_patterns_builtin = !exclusion.secret_patterns_builtin;
            }
            Toggle::ShowSummaryBanner => {
                exclusion.show_summary_banner = !exclusion.show_summary_banner
            }
            Toggle::ShowSummaryHistory => {
                exclusion.show_summary_history = !exclusion.show_summary_history
            }
            Toggle::PromptRevealSecretMatches => {
                exclusion.prompt_reveal_secret_matches = !exclusion.prompt_reveal_secret_matches
            }
            Toggle::PreflightShellPaths => {
                exclusion.preflight_shell_paths = !exclusion.preflight_shell_paths
            }
        }
        NextSettings {
            exclusion,
            hooks_sanitize_payloads,
        }
    }
}

fn apply_toggle_rules(
    current: &ExclusionConfig,
    hooks_sanitize_payloads: bool,
    toggle: Toggle,
) -> NextSettings {
    let mut exclusion = current.clone();
    let mut hooks_sanitize_payloads = hooks_sanitize_payloads;

    match toggle {
        Toggle::Enabled => {
            let next_enabled = !exclusion.enabled;
            exclusion.enabled = next_enabled;
            if next_enabled {
                apply_enabled_defaults(&mut exclusion, &mut hooks_sanitize_payloads);
            } else {
                disable_all_layers(&mut exclusion, &mut hooks_sanitize_payloads);
                exclusion.paranoid_mode = false;
            }
        }
        Toggle::ParanoidMode => {
            if !exclusion.enabled {
                return NextSettings {
                    exclusion,
                    hooks_sanitize_payloads,
                };
            }
            let next_paranoid = !exclusion.paranoid_mode;
            exclusion.paranoid_mode = next_paranoid;
            if next_paranoid {
                apply_paranoid_defaults(&mut exclusion, &mut hooks_sanitize_payloads);
            } else {
                exclusion.layer_output_sanitization = Some(false);
                exclusion.layer_request_interceptor = Some(false);
            }
        }
        Toggle::PathMatching => {
            if !exclusion.enabled {
                return NextSettings {
                    exclusion,
                    hooks_sanitize_payloads,
                };
            }
            exclusion.path_matching = !exclusion.path_matching;
            exclusion.preflight_shell_paths = exclusion.path_matching;
        }
        Toggle::LayerOutput | Toggle::LayerSend | Toggle::LayerRequest | Toggle::HooksPayloads => {
            if !exclusion.enabled {
                return NextSettings {
                    exclusion,
                    hooks_sanitize_payloads,
                };
            }
            let next = toggle.apply(&exclusion, hooks_sanitize_payloads);
            exclusion = next.exclusion;
            hooks_sanitize_payloads = next.hooks_sanitize_payloads;
        }
        Toggle::PreflightShellPaths => {
            if !exclusion.enabled || !exclusion.path_matching {
                return NextSettings {
                    exclusion,
                    hooks_sanitize_payloads,
                };
            }
            exclusion.preflight_shell_paths = !exclusion.preflight_shell_paths;
        }
        _ => {
            let next = toggle.apply(&exclusion, hooks_sanitize_payloads);
            exclusion = next.exclusion;
            hooks_sanitize_payloads = next.hooks_sanitize_payloads;
        }
    }

    if exclusion.enabled {
        let all_layers_on = layers_enabled(&exclusion, hooks_sanitize_payloads);
        if all_layers_on {
            exclusion.paranoid_mode = true;
        } else if exclusion.paranoid_mode {
            exclusion.paranoid_mode = false;
        }
    }

    NextSettings {
        exclusion,
        hooks_sanitize_payloads,
    }
}

fn apply_enabled_defaults(exclusion: &mut ExclusionConfig, hooks_sanitize_payloads: &mut bool) {
    exclusion.paranoid_mode = false;
    exclusion.path_matching = true;
    exclusion.layer_send_firewall = Some(true);
    exclusion.layer_output_sanitization = Some(false);
    exclusion.layer_request_interceptor = Some(false);
    exclusion.preflight_shell_paths = true;
    *hooks_sanitize_payloads = true;
}

fn apply_paranoid_defaults(exclusion: &mut ExclusionConfig, hooks_sanitize_payloads: &mut bool) {
    exclusion.path_matching = true;
    exclusion.layer_send_firewall = Some(true);
    exclusion.layer_output_sanitization = Some(true);
    exclusion.layer_request_interceptor = Some(true);
    exclusion.preflight_shell_paths = true;
    *hooks_sanitize_payloads = true;
}

fn disable_all_layers(exclusion: &mut ExclusionConfig, hooks_sanitize_payloads: &mut bool) {
    exclusion.path_matching = false;
    exclusion.layer_send_firewall = Some(false);
    exclusion.layer_output_sanitization = Some(false);
    exclusion.layer_request_interceptor = Some(false);
    exclusion.preflight_shell_paths = false;
    *hooks_sanitize_payloads = false;
}

fn layers_enabled(exclusion: &ExclusionConfig, hooks_sanitize_payloads: bool) -> bool {
    exclusion.path_matching
        && exclusion.layer_send_firewall_enabled()
        && exclusion.layer_output_sanitization_enabled()
        && exclusion.layer_request_interceptor_enabled()
        && hooks_sanitize_payloads
}

#[derive(Clone)]
struct NextSettings {
    exclusion: ExclusionConfig,
    hooks_sanitize_payloads: bool,
}

#[derive(Clone, Copy)]
enum Preset {
    AllowAll,
    BlockAll,
    AskAndAllow,
}

impl Preset {
    fn apply(self, current: &ExclusionConfig, _hooks_sanitize_payloads: bool) -> NextSettings {
        let mut exclusion = current.clone();
        let hooks_sanitize_payloads = match self {
            Preset::AllowAll => {
                exclusion.enabled = false;
                exclusion.prompt_on_blocked = false;
                exclusion.on_match = ExclusionOnMatch::Warn;
                exclusion.layer_output_sanitization = Some(false);
                exclusion.layer_send_firewall = Some(false);
                exclusion.layer_request_interceptor = Some(false);
                false
            }
            Preset::BlockAll => {
                exclusion.enabled = true;
                exclusion.on_match = ExclusionOnMatch::Block;
                exclusion.prompt_on_blocked = false;
                exclusion.layer_output_sanitization = Some(true);
                exclusion.layer_send_firewall = Some(true);
                exclusion.layer_request_interceptor = Some(true);
                true
            }
            Preset::AskAndAllow => {
                exclusion.enabled = true;
                exclusion.on_match = ExclusionOnMatch::Warn;
                exclusion.prompt_on_blocked = true;
                exclusion.layer_output_sanitization = Some(true);
                exclusion.layer_send_firewall = Some(true);
                exclusion.layer_request_interceptor = Some(true);
                true
            }
        };

        NextSettings {
            exclusion,
            hooks_sanitize_payloads,
        }
    }

    fn matches(self, current: &ExclusionConfig, hooks_sanitize_payloads: bool) -> bool {
        let next = self.apply(current, hooks_sanitize_payloads);
        next.exclusion.enabled == current.enabled
            && next.exclusion.prompt_on_blocked == current.prompt_on_blocked
            && next.exclusion.on_match == current.on_match
            && next.exclusion.layer_output_sanitization == current.layer_output_sanitization
            && next.exclusion.layer_send_firewall == current.layer_send_firewall
            && next.exclusion.layer_request_interceptor == current.layer_request_interceptor
            && next.hooks_sanitize_payloads == hooks_sanitize_payloads
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn preset_allow_all_sets_expected_values() {
        let current = ExclusionConfig::default();
        let next = Preset::AllowAll.apply(&current, true);

        assert_eq!(next.exclusion.enabled, false);
        assert_eq!(next.exclusion.prompt_on_blocked, false);
        assert_eq!(next.exclusion.on_match, ExclusionOnMatch::Warn);
        assert_eq!(next.exclusion.layer_output_sanitization, Some(false));
        assert_eq!(next.exclusion.layer_send_firewall, Some(false));
        assert_eq!(next.exclusion.layer_request_interceptor, Some(false));
        assert_eq!(next.hooks_sanitize_payloads, false);
    }

    #[test]
    fn preset_block_all_sets_expected_values() {
        let current = ExclusionConfig::default();
        let next = Preset::BlockAll.apply(&current, false);

        assert_eq!(next.exclusion.enabled, true);
        assert_eq!(next.exclusion.prompt_on_blocked, false);
        assert_eq!(next.exclusion.on_match, ExclusionOnMatch::Block);
        assert_eq!(next.exclusion.layer_output_sanitization, Some(true));
        assert_eq!(next.exclusion.layer_send_firewall, Some(true));
        assert_eq!(next.exclusion.layer_request_interceptor, Some(true));
        assert_eq!(next.hooks_sanitize_payloads, true);
    }

    #[test]
    fn preset_ask_and_allow_sets_expected_values() {
        let current = ExclusionConfig::default();
        let next = Preset::AskAndAllow.apply(&current, false);

        assert_eq!(next.exclusion.enabled, true);
        assert_eq!(next.exclusion.prompt_on_blocked, true);
        assert_eq!(next.exclusion.on_match, ExclusionOnMatch::Warn);
        assert_eq!(next.exclusion.layer_output_sanitization, Some(true));
        assert_eq!(next.exclusion.layer_send_firewall, Some(true));
        assert_eq!(next.exclusion.layer_request_interceptor, Some(true));
        assert_eq!(next.hooks_sanitize_payloads, true);
    }

    #[test]
    fn preset_matches_returns_true_for_its_own_output() {
        let current = ExclusionConfig::default();
        let allow = Preset::AllowAll.apply(&current, true);
        let block = Preset::BlockAll.apply(&current, false);
        let ask = Preset::AskAndAllow.apply(&current, false);

        assert_eq!(
            Preset::AllowAll.matches(&allow.exclusion, allow.hooks_sanitize_payloads),
            true
        );
        assert_eq!(
            Preset::BlockAll.matches(&block.exclusion, block.hooks_sanitize_payloads),
            true
        );
        assert_eq!(
            Preset::AskAndAllow.matches(&ask.exclusion, ask.hooks_sanitize_payloads),
            true
        );
    }
}
