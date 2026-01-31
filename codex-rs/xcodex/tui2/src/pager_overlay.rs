//! Overlay UIs rendered in an alternate screen.
//!
//! This module implements the pager-style overlays used by the TUI, including the transcript
//! overlay (`Ctrl+T`) that renders a full history view separate from the main viewport.
//!
//! The transcript overlay renders committed transcript cells plus an optional render-only live tail
//! derived from the current in-flight active cell. Because rebuilding wrapped `Line`s on every draw
//! can be expensive, that live tail is cached and only recomputed when its cache key changes, which
//! is derived from the terminal width (wrapping), an active-cell revision (in-place mutations), the
//! stream-continuation flag (spacing), and an animation tick (time-based spinner/shimmer output).
//!
//! The transcript overlay live tail is kept in sync by `App` during draws: `App` supplies an
//! `ActiveCellTranscriptKey` and a function to compute the active cell transcript lines, and
//! `TranscriptOverlay::sync_live_tail` uses the key to decide when the cached tail must be
//! recomputed. `ChatWidget` is responsible for producing a key that changes when the active cell
//! mutates in place or when its transcript output is time-dependent.

use std::collections::HashMap;
use std::io::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::BottomPaneParams;
use crate::chatwidget::ActiveCellTranscriptKey;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::Insets;
use crate::render::RectExt;
use crate::render::renderable::InsetRenderable;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;
use crate::tui;
use crate::tui::TuiEvent;
use codex_core::protocol::FileChange;
use codex_protocol::ThreadId;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
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
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;
use serde_json::json;

pub(crate) enum Overlay {
    Transcript(TranscriptOverlay),
    Static(StaticOverlay),
    #[allow(dead_code)]
    ThemePreview(ThemePreviewOverlay),
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

    #[allow(dead_code)]
    pub(crate) fn new_theme_preview(
        app_event_tx: AppEventSender,
        config: codex_core::config::Config,
        terminal_bg: Option<(u8, u8, u8)>,
    ) -> Self {
        Self::ThemePreview(ThemePreviewOverlay::new(app_event_tx, config, terminal_bg))
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match self {
            Overlay::Transcript(o) => o.handle_event(tui, event),
            Overlay::Static(o) => o.handle_event(tui, event),
            Overlay::ThemePreview(o) => o.handle_event(tui, event),
            Overlay::ThemeSelector(o) => o.handle_event(tui, event),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        match self {
            Overlay::Transcript(o) => o.is_done(),
            Overlay::Static(o) => o.is_done(),
            Overlay::ThemePreview(o) => o.is_done(),
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
const KEY_LEFT: KeyBinding = key_hint::plain(KeyCode::Left);
const KEY_RIGHT: KeyBinding = key_hint::plain(KeyCode::Right);
const KEY_CTRL_F: KeyBinding = key_hint::ctrl(KeyCode::Char('f'));
const KEY_CTRL_D: KeyBinding = key_hint::ctrl(KeyCode::Char('d'));
const KEY_CTRL_B: KeyBinding = key_hint::ctrl(KeyCode::Char('b'));
const KEY_CTRL_U: KeyBinding = key_hint::ctrl(KeyCode::Char('u'));
const KEY_TAB: KeyBinding = key_hint::plain(KeyCode::Tab);
const KEY_Q: KeyBinding = key_hint::plain(KeyCode::Char('q'));
const KEY_ESC: KeyBinding = key_hint::plain(KeyCode::Esc);
const KEY_ENTER: KeyBinding = key_hint::plain(KeyCode::Enter);
const KEY_CTRL_T: KeyBinding = key_hint::ctrl(KeyCode::Char('t'));
const KEY_CTRL_S: KeyBinding = key_hint::ctrl(KeyCode::Char('s'));
const KEY_CTRL_C: KeyBinding = key_hint::ctrl(KeyCode::Char('c'));
const KEY_CTRL_G: KeyBinding = key_hint::ctrl(KeyCode::Char('g'));
const KEY_CTRL_P: KeyBinding = key_hint::ctrl(KeyCode::Char('p'));
const KEY_CTRL_M: KeyBinding = key_hint::ctrl(KeyCode::Char('m'));
const KEY_QUESTION: KeyBinding = key_hint::plain(KeyCode::Char('?'));

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

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../theme-ui/src/theme_selector_overlay.rs"
));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemePreviewMode {
    Preview,
    Edit,
}

pub(crate) struct ThemePreviewOverlay {
    preview: ThemeSelectorOverlay,
    terminal_bg: Option<(u8, u8, u8)>,
    mode: ThemePreviewMode,

    // Preview scroll state.
    preview_scroll: u16,
    max_preview_scroll: u16,

    // Edit mode state.
    edit_tab: ThemeEditTab,
    edit_selected_idx: usize,
    edit_scroll_top: usize,
    last_editor_area: Option<Rect>,

    working_theme: Option<codex_core::themes::ThemeDefinition>,
    base_theme_name: Option<String>,
    variant: Option<codex_core::themes::ThemeVariant>,

    color_picker: Option<ColorPickerState>,
    save_modal: Option<SaveThemeState>,

    is_done: bool,
}

impl ThemePreviewOverlay {
    #[allow(dead_code)]
    fn new(
        app_event_tx: AppEventSender,
        config: codex_core::config::Config,
        terminal_bg: Option<(u8, u8, u8)>,
    ) -> Self {
        Self {
            preview: ThemeSelectorOverlay::new(app_event_tx, config, terminal_bg),
            terminal_bg,
            mode: ThemePreviewMode::Preview,
            preview_scroll: 0,
            max_preview_scroll: 0,
            edit_tab: ThemeEditTab::Palette,
            edit_selected_idx: 0,
            edit_scroll_top: 0,
            last_editor_area: None,
            working_theme: None,
            base_theme_name: None,
            variant: None,
            color_picker: None,
            save_modal: None,
            is_done: false,
        }
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        if self.preview.frame_requester.is_none() {
            self.preview.frame_requester = Some(tui.frame_requester());
        }

        if matches!(event, TuiEvent::Draw) {
            return self.handle_draw(tui);
        }

        if self.handle_modal_event(tui, &event)? {
            return Ok(());
        }

        match self.mode {
            ThemePreviewMode::Preview => self.handle_preview_event(tui, event),
            ThemePreviewMode::Edit => self.handle_edit_event(tui, event),
        }
    }

    fn handle_preview_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('u' | 'U'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(3);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('d' | 'D'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_add(3);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('g' | 'G'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let next = !self.preview.config.tui_transcript_diff_highlight;
                self.preview.config.tui_transcript_diff_highlight = next;
                self.preview
                    .app_event_tx
                    .send(AppEvent::UpdateTranscriptDiffHighlight(next));
                self.preview
                    .app_event_tx
                    .send(AppEvent::PersistTranscriptDiffHighlight(next));
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('p' | 'P'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let next = !self.preview.config.tui_transcript_user_prompt_highlight;
                self.preview.config.tui_transcript_user_prompt_highlight = next;
                self.preview
                    .app_event_tx
                    .send(AppEvent::UpdateTranscriptUserPromptHighlight(next));
                self.preview
                    .app_event_tx
                    .send(AppEvent::PersistTranscriptUserPromptHighlight(next));
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('m' | 'M'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                let next = !self.preview.config.tui_minimal_composer;
                self.preview.config.tui_minimal_composer = next;
                self.preview
                    .app_event_tx
                    .send(AppEvent::UpdateMinimalComposer(next));
                self.preview
                    .app_event_tx
                    .send(AppEvent::PersistMinimalComposer(next));
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('t' | 'T'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.enter_edit_mode();
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('q'),
                kind: KeyEventKind::Press,
                ..
            })
            | TuiEvent::Key(KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            })
            | TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('c' | 'C'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.is_done = true;
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(1);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
                tui.frame_requester().schedule_frame();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_edit_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.exit_edit_mode_revert();
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('q'),
                kind: KeyEventKind::Press,
                ..
            }) => {
                self.exit_edit_mode_revert();
                self.is_done = true;
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Tab,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.edit_tab = self.edit_tab.toggle();
                self.edit_selected_idx = 0;
                self.edit_scroll_top = 0;
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.move_selection(-1);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Down,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.move_selection(1);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if let Some(key) = self.selected_key() {
                    self.open_color_picker(key);
                    tui.frame_requester().schedule_frame();
                }
            }
            TuiEvent::Key(KeyEvent {
                code: KeyCode::Char('s' | 'S'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => {
                self.open_save_modal();
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                ..
            }) => {
                if let Some(editor_area) = self.last_editor_area
                    && column >= editor_area.left()
                    && column < editor_area.right()
                    && row >= editor_area.top()
                    && row < editor_area.bottom()
                    && let Some(key) = self.editor_hit_test(editor_area, column, row)
                {
                    self.open_color_picker(key);
                    tui.frame_requester().schedule_frame();
                }
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(1);
                tui.frame_requester().schedule_frame();
            }
            TuiEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            }) => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
                tui.frame_requester().schedule_frame();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_modal_event(&mut self, tui: &mut tui::Tui, event: &TuiEvent) -> Result<bool> {
        if let Some(save) = self.save_modal.as_mut() {
            match event {
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    self.save_modal = None;
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    if save.stage == SaveStage::Editing {
                        self.commit_save();
                    }
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Char('y' | 'Y'),
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    if save.stage == SaveStage::ConfirmOverwrite {
                        self.commit_overwrite(true);
                    }
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Char('n' | 'N'),
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    if save.stage == SaveStage::ConfirmOverwrite {
                        self.commit_overwrite(false);
                    }
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(key) => {
                    if save.stage == SaveStage::Editing {
                        apply_text_edit(&mut save.name, &mut save.cursor, key);
                        tui.frame_requester().schedule_frame();
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if let Some(picker) = self.color_picker.as_mut() {
            let mut apply_live = false;
            match event {
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    self.cancel_color_picker();
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    self.color_picker = None;
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Char('i' | 'I'),
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    picker.inherit = !picker.inherit;
                    if picker.inherit {
                        picker.derived = false;
                    }
                    apply_live = true;
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Char('d' | 'D'),
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    if is_optional_role_key(picker.key) {
                        picker.derived = !picker.derived;
                        if picker.derived {
                            picker.inherit = false;
                        }
                        apply_live = true;
                        tui.frame_requester().schedule_frame();
                    }
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Up,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    picker.focus = match picker.focus {
                        ColorPickerFocus::Hex => ColorPickerFocus::R,
                        ColorPickerFocus::R => ColorPickerFocus::R,
                        ColorPickerFocus::G => ColorPickerFocus::R,
                        ColorPickerFocus::B => ColorPickerFocus::G,
                    };
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Down,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    picker.focus = match picker.focus {
                        ColorPickerFocus::Hex => ColorPickerFocus::R,
                        ColorPickerFocus::R => ColorPickerFocus::G,
                        ColorPickerFocus::G => ColorPickerFocus::B,
                        ColorPickerFocus::B => ColorPickerFocus::B,
                    };
                    tui.frame_requester().schedule_frame();
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Left,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    if !picker.inherit && !picker.derived {
                        match picker.focus {
                            ColorPickerFocus::R => picker.r = picker.r.saturating_sub(1),
                            ColorPickerFocus::G => picker.g = picker.g.saturating_sub(1),
                            ColorPickerFocus::B => picker.b = picker.b.saturating_sub(1),
                            ColorPickerFocus::Hex => {}
                        }
                        Self::sync_picker_rgb_text_from_rgb(picker);
                        Self::sync_picker_hex_from_rgb(picker);
                        apply_live = true;
                        tui.frame_requester().schedule_frame();
                    }
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Right,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    if !picker.inherit && !picker.derived {
                        match picker.focus {
                            ColorPickerFocus::R => picker.r = picker.r.saturating_add(1),
                            ColorPickerFocus::G => picker.g = picker.g.saturating_add(1),
                            ColorPickerFocus::B => picker.b = picker.b.saturating_add(1),
                            ColorPickerFocus::Hex => {}
                        }
                        Self::sync_picker_rgb_text_from_rgb(picker);
                        Self::sync_picker_hex_from_rgb(picker);
                        apply_live = true;
                        tui.frame_requester().schedule_frame();
                    }
                }
                TuiEvent::Key(key) => {
                    if picker.focus == ColorPickerFocus::Hex
                        && !picker.inherit
                        && !picker.derived
                        && apply_hex_edit(&mut picker.hex, &mut picker.cursor, key)
                    {
                        if picker.hex.len() == 6
                            && let Ok((r, g, b)) = parse_hex_rgb(picker.hex.as_str())
                        {
                            picker.r = r;
                            picker.g = g;
                            picker.b = b;
                            Self::sync_picker_rgb_text_from_rgb(picker);
                            apply_live = true;
                        }
                        tui.frame_requester().schedule_frame();
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
                                    } else if let Some(value) =
                                        parse_rgb_text(picker.r_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.r = clamped;
                                        picker.r_text = clamped.to_string();
                                        picker.r_cursor = picker.r_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                    tui.frame_requester().schedule_frame();
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
                                    } else if let Some(value) =
                                        parse_rgb_text(picker.g_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.g = clamped;
                                        picker.g_text = clamped.to_string();
                                        picker.g_cursor = picker.g_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                    tui.frame_requester().schedule_frame();
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
                                    } else if let Some(value) =
                                        parse_rgb_text(picker.b_text.as_str())
                                    {
                                        let clamped = value.min(255) as u8;
                                        picker.b = clamped;
                                        picker.b_text = clamped.to_string();
                                        picker.b_cursor = picker.b_text.len();
                                        Self::sync_picker_hex_from_rgb(picker);
                                        apply_live = true;
                                    }
                                    tui.frame_requester().schedule_frame();
                                }
                            }
                            ColorPickerFocus::Hex => {}
                        }
                    }
                }
                _ => {}
            }
            if apply_live {
                self.apply_color_picker_live();
            }
            return Ok(true);
        }

        Ok(false)
    }

    fn handle_draw(&mut self, tui: &mut tui::Tui) -> Result<()> {
        if self.preview.frame_requester.is_none() {
            return Ok(());
        };

        let requested_scroll = self.preview_scroll;
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
            let body = parts[0];
            let footer = parts[1];

            let (editor_area, preview_area) = match self.mode {
                ThemePreviewMode::Preview => (Rect::new(body.x, body.y, body.width, 0), body),
                ThemePreviewMode::Edit => {
                    let editor_h = body.height.min(5);
                    let parts = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(editor_h), Constraint::Min(0)])
                        .split(body);
                    (parts[0], parts[1])
                }
            };

            self.last_editor_area = if self.mode == ThemePreviewMode::Edit {
                Some(editor_area)
            } else {
                None
            };

            self.max_preview_scroll =
                self.preview
                    .render_preview(preview_area, frame.buffer_mut(), requested_scroll);

            if self.mode == ThemePreviewMode::Edit {
                self.render_editor(editor_area, frame.buffer_mut());
            }

            self.render_footer(footer, frame.buffer_mut());
            if let Some((x, y)) = self.render_modals(area, frame.buffer_mut()) {
                frame.set_cursor_position((x, y));
            }
        })?;

        if self.preview_scroll > self.max_preview_scroll {
            self.preview_scroll = self.max_preview_scroll;
            tui.frame_requester().schedule_frame();
        }

        Ok(())
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let pairs: Vec<(&[KeyBinding], &str)> = match self.mode {
            ThemePreviewMode::Preview => vec![
                (&[KEY_CTRL_U, KEY_CTRL_D], "scroll"),
                (&[KEY_CTRL_G], "diff highlight"),
                (&[KEY_CTRL_P], "prompt highlight"),
                (&[KEY_CTRL_M], "minimal composer"),
                (&[KEY_CTRL_T], "edit"),
                (&[KEY_Q], "quit"),
            ],
            ThemePreviewMode::Edit => vec![
                (&[KEY_TAB], "palette/roles"),
                (&[KEY_ENTER], "pick"),
                (&[KEY_CTRL_S], "save as"),
                (&[KEY_ESC], "exit edit"),
                (&[KEY_Q], "quit"),
            ],
        };
        render_key_hints(area, buf, &pairs);
    }

    fn render_editor(&mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(crate::theme::composer_style());
            }
        }

        let tabs = Line::from(vec![
            match self.edit_tab {
                ThemeEditTab::Palette => "Palette".bold(),
                ThemeEditTab::Roles => "Palette".into(),
            },
            " ".into(),
            match self.edit_tab {
                ThemeEditTab::Roles => "Roles".bold(),
                ThemeEditTab::Palette => "Roles".into(),
            },
            "   ".into(),
            match self.edit_tab {
                ThemeEditTab::Palette => {
                    "(Tab toggle to Roles, Enter pick, Ctrl+S save theme)".dim()
                }
                ThemeEditTab::Roles => {
                    "(Tab toggle to Palette, Enter pick, Ctrl+S save theme)".dim()
                }
            },
        ]);
        tabs.render_ref(Rect::new(area.x, area.y, area.width, 1), buf);

        let Some(theme) = self.working_theme.as_ref() else {
            return;
        };

        let content = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        match self.edit_tab {
            ThemeEditTab::Palette => self.render_palette_swatches(theme, content, buf),
            ThemeEditTab::Roles => self.render_role_list(theme, content, buf),
        }
    }

    fn render_palette_swatches(
        &self,
        theme: &codex_core::themes::ThemeDefinition,
        area: Rect,
        buf: &mut Buffer,
    ) {
        if area.is_empty() {
            return;
        }

        let swatch_w = 6u16;
        let cols = (area.width / swatch_w).max(1) as usize;
        let keys = palette_keys();
        let max = (area.height as usize * cols).min(keys.len());

        for idx in 0..max {
            let row = idx / cols;
            let col = idx % cols;
            let x = area.x + col as u16 * swatch_w;
            let y = area.y + row as u16;
            let rect = Rect::new(x, y, swatch_w.min(area.width.saturating_sub(x - area.x)), 1);
            if rect.width == 0 {
                continue;
            }

            let key = keys[idx];
            let label = format!("{idx:02X}");
            let value = palette_value(theme, key);
            let (inherit, rgb) =
                parse_theme_color_as_rgb(theme, value.as_str()).unwrap_or((false, None));
            let style = if inherit {
                crate::theme::composer_style().patch(crate::theme::dim_style())
            } else if let Some((r, g, b)) = rgb {
                let c = crate::terminal_palette::best_color((r, g, b));
                Style::default().fg(c).bg(c)
            } else {
                crate::theme::warning_style().patch(crate::theme::composer_style())
            };

            let mut line: Line<'static> = vec![label.into(), " ".into(), "██".into()].into();
            line = line.set_style(style);
            if idx == self.edit_selected_idx {
                line = line.set_style(
                    crate::theme::composer_style()
                        .patch(crate::theme::accent_style())
                        .add_modifier(Modifier::BOLD),
                );
            }
            line.render_ref(rect, buf);
        }
    }

    fn render_role_list(
        &self,
        theme: &codex_core::themes::ThemeDefinition,
        area: Rect,
        buf: &mut Buffer,
    ) {
        if area.is_empty() {
            return;
        }

        let keys = role_keys();
        if keys.is_empty() {
            return;
        }

        let visible = usize::from(area.height.max(1)).min(keys.len());
        let start = self.edit_scroll_top;
        let end = (start + visible).min(keys.len());
        for (row, idx) in (start..end).enumerate() {
            let y = area.y + row as u16;
            let key = keys[idx];
            let value = role_value(theme, key).unwrap_or_default();
            let mut line: Line<'static> = vec![key.dim(), " ".into(), value.clone().into()].into();
            let mut style = crate::theme::composer_style();
            if idx == self.edit_selected_idx {
                style = style
                    .patch(crate::theme::accent_style())
                    .add_modifier(Modifier::BOLD);
            }
            line = line.set_style(style);
            Paragraph::new(line).render_ref(Rect::new(area.x, y, area.width, 1), buf);
        }
    }

    fn render_modals(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
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

            "Save theme as"
                .bold()
                .render_ref(Rect::new(rect.x + 2, rect.y, rect.width - 4, 1), buf);

            let input = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1);
            let shown = save.name.clone();
            Paragraph::new(Line::from(shown))
                .style(crate::theme::composer_style())
                .render_ref(input, buf);

            if let Some(err) = save.error.as_ref() {
                err.as_str()
                    .red()
                    .render_ref(Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1), buf);
            } else if save.stage == SaveStage::ConfirmOverwrite {
                "Overwrite? (y/n)"
                    .yellow()
                    .render_ref(Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1), buf);
            } else {
                "Enter to save, Esc to cancel"
                    .dim()
                    .render_ref(Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1), buf);
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

        let r_rect = Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1);
        let g_rect = Rect::new(rect.x + 2, rect.y + 4, rect.width - 4, 1);
        let b_rect = Rect::new(rect.x + 2, rect.y + 5, rect.width - 4, 1);
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
            let cursor_pos = match picker.focus {
                ColorPickerFocus::R => Some((r_rect, picker.r_text.as_str(), picker.r_cursor)),
                ColorPickerFocus::G => Some((g_rect, picker.g_text.as_str(), picker.g_cursor)),
                ColorPickerFocus::B => Some((b_rect, picker.b_text.as_str(), picker.b_cursor)),
                ColorPickerFocus::Hex => None,
            };
            if let Some((row_rect, value_text, cursor)) = cursor_pos {
                let prefix = format!(
                    "{}: ",
                    match picker.focus {
                        ColorPickerFocus::R => "R",
                        ColorPickerFocus::G => "G",
                        ColorPickerFocus::B => "B",
                        ColorPickerFocus::Hex => "",
                    }
                );
                let bar_w = row_rect.width.saturating_sub(prefix.len() as u16 + 3 + 4);
                let value_start = row_rect.x + prefix.len() as u16 + 1 + bar_w + 2;
                let digits_start = value_start + 3u16.saturating_sub(value_text.len() as u16);
                let cursor_x = digits_start + cursor.min(value_text.len()) as u16;
                return Some((cursor_x, row_rect.y));
            }
        }

        None
    }

    fn enter_edit_mode(&mut self) {
        use codex_core::themes::ThemeCatalog;

        let terminal_bg = self.terminal_bg;
        let variant = crate::theme::active_variant(&self.preview.config, terminal_bg);
        let terminal_background_is_light = terminal_bg.is_some_and(crate::color::is_light);

        let Ok(catalog) = ThemeCatalog::load(&self.preview.config) else {
            return;
        };

        let active = catalog.resolve_active(
            &self.preview.config.xcodex.themes,
            Some(variant),
            terminal_background_is_light,
        );

        self.base_theme_name = Some(active.name.clone());
        self.variant = Some(variant);
        self.working_theme = Some(active.clone());
        self.mode = ThemePreviewMode::Edit;
        self.edit_tab = ThemeEditTab::Palette;
        self.edit_selected_idx = 0;
        self.edit_scroll_top = 0;
        self.preview_scroll = 0;
        self.max_preview_scroll = 0;

        if let Some(theme) = self.working_theme.as_ref() {
            crate::theme::preview_definition(theme);
        }
    }

    fn exit_edit_mode_revert(&mut self) {
        self.color_picker = None;
        self.save_modal = None;
        self.working_theme = None;
        self.base_theme_name = None;
        self.variant = None;
        self.mode = ThemePreviewMode::Preview;
        self.preview.app_event_tx.send(AppEvent::CancelThemePreview);
    }

    fn visible_keys(&self) -> &'static [&'static str] {
        match self.edit_tab {
            ThemeEditTab::Palette => palette_keys(),
            ThemeEditTab::Roles => role_keys(),
        }
    }

    fn selected_key(&self) -> Option<&'static str> {
        self.visible_keys().get(self.edit_selected_idx).copied()
    }

    fn move_selection(&mut self, delta: isize) {
        let keys = self.visible_keys();
        if keys.is_empty() {
            return;
        }
        let len = keys.len() as isize;
        let next = (self.edit_selected_idx as isize + delta).rem_euclid(len) as usize;
        self.edit_selected_idx = next;

        if self.edit_tab == ThemeEditTab::Roles {
            let list_height = 4usize;
            if self.edit_selected_idx < self.edit_scroll_top {
                self.edit_scroll_top = self.edit_selected_idx;
            } else if self.edit_selected_idx >= self.edit_scroll_top + list_height {
                self.edit_scroll_top = self.edit_selected_idx + 1 - list_height;
            }
        }
    }

    fn editor_hit_test(&self, editor_area: Rect, column: u16, row: u16) -> Option<&'static str> {
        let content = Rect::new(
            editor_area.x,
            editor_area.y.saturating_add(1),
            editor_area.width,
            editor_area.height.saturating_sub(1),
        );
        if content.is_empty() {
            return None;
        }

        match self.edit_tab {
            ThemeEditTab::Palette => {
                let swatch_w = 6u16;
                let cols = (content.width / swatch_w).max(1) as usize;
                let x = column.saturating_sub(content.x);
                let y = row.saturating_sub(content.y) as usize;
                let col = usize::from(x / swatch_w);
                let idx = y.saturating_mul(cols).saturating_add(col);
                palette_keys().get(idx).copied()
            }
            ThemeEditTab::Roles => {
                let y = row.saturating_sub(content.y) as usize;
                let idx = self.edit_scroll_top.saturating_add(y);
                role_keys().get(idx).copied()
            }
        }
    }

    fn open_color_picker(&mut self, key: &'static str) {
        let Some(theme) = self.working_theme.as_ref() else {
            return;
        };

        let current = if key.starts_with("palette.") {
            palette_value(theme, key)
        } else {
            role_value(theme, key).unwrap_or_default()
        };

        let (inherit, rgb) = match parse_theme_color_as_rgb(theme, current.as_str()) {
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

    fn apply_color_picker_live(&mut self) {
        let Some(picker) = self.color_picker.as_ref() else {
            return;
        };
        let Some(theme) = self.working_theme.as_mut() else {
            return;
        };

        if picker.derived && is_optional_role_key(picker.key) {
            let _ = set_role_value(theme, picker.key, "");
            crate::theme::preview_definition(theme);
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
            set_palette_value(theme, picker.key, value.as_str());
        } else {
            let _ = set_role_value(theme, picker.key, value.as_str());
        }
        crate::theme::preview_definition(theme);
    }

    fn cancel_color_picker(&mut self) {
        let Some(picker) = self.color_picker.take() else {
            return;
        };
        let Some(theme) = self.working_theme.as_mut() else {
            return;
        };

        if picker.key.starts_with("palette.") {
            set_palette_value(theme, picker.key, picker.original_value.as_str());
        } else {
            let _ = set_role_value(theme, picker.key, picker.original_value.as_str());
        }
        crate::theme::preview_definition(theme);
    }

    fn open_save_modal(&mut self) {
        let Some(base) = self.base_theme_name.clone() else {
            return;
        };
        let suggested = if base == "default" {
            "my-theme".to_string()
        } else {
            format!("{base}-custom")
        };
        self.save_modal = Some(SaveThemeState {
            stage: SaveStage::Editing,
            cursor: suggested.len(),
            name: suggested,
            overwrite_path: None,
            error: None,
        });
    }

    fn commit_save(&mut self) {
        use codex_core::themes::ThemeCatalog;

        let Some(save) = self.save_modal.as_mut() else {
            return;
        };
        let Some(theme) = self.working_theme.as_ref() else {
            save.error = Some("No theme loaded.".to_string());
            return;
        };
        let Some(variant) = self.variant else {
            save.error = Some("No active variant.".to_string());
            return;
        };

        let name = save.name.trim().to_string();
        if name.is_empty() {
            save.error = Some("Theme name cannot be empty.".to_string());
            return;
        }

        let mut out = theme.clone();
        out.name = name.clone();
        out.variant = variant;

        if let Err(err) = out.validate() {
            save.error = Some(format!("Theme is not valid: {err}"));
            return;
        }

        let catalog = match ThemeCatalog::load(&self.preview.config) {
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

        let dir = codex_core::themes::themes_dir(
            &self.preview.config.codex_home,
            &self.preview.config.xcodex.themes,
        );
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

        self.preview
            .app_event_tx
            .send(AppEvent::PersistThemeSelection {
                variant,
                theme: name,
            });
        self.is_done = true;
    }

    fn commit_overwrite(&mut self, overwrite: bool) {
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
        let Some(theme) = self.working_theme.as_ref() else {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = Some("No theme loaded.".to_string());
            return;
        };
        let Some(variant) = self.variant else {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = Some("No active variant.".to_string());
            return;
        };

        let name = save.name.trim().to_string();
        if name.is_empty() {
            save.stage = SaveStage::Editing;
            save.overwrite_path = None;
            save.error = Some("Theme name cannot be empty.".to_string());
            return;
        }

        let mut out = theme.clone();
        out.name = name.clone();
        out.variant = variant;

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

        self.preview
            .app_event_tx
            .send(AppEvent::PersistThemeSelection {
                variant,
                theme: name,
            });
        self.is_done = true;
    }

    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
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
        // Request a redraw; the frame scheduler coalesces bursts and clamps to 60fps.
        tui.frame_requester().schedule_frame();
        Ok(())
    }

    fn handle_mouse_scroll(&mut self, tui: &mut tui::Tui, event: MouseEvent) -> Result<()> {
        let step: usize = 3;
        match event.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(step);
            }
            MouseEventKind::ScrollDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(step);
            }
            _ => {
                return Ok(());
            }
        }
        // Request a redraw; the frame scheduler coalesces bursts and clamps to 60fps.
        tui.frame_requester().schedule_frame();
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
    /// Pager UI state and the renderables currently displayed.
    ///
    /// The invariant is that `view.renderables` is `render_cells(cells)` plus an optional trailing
    /// live-tail renderable appended after the committed cells.
    view: PagerView,
    /// Committed transcript cells (does not include the live tail).
    cells: Vec<Arc<dyn HistoryCell>>,
    highlight_cell: Option<usize>,
    /// Cache key for the render-only live tail appended after committed cells.
    live_tail_key: Option<LiveTailKey>,
    is_done: bool,
}

/// Cache key for the active-cell "live tail" appended to the transcript overlay.
///
/// Changing any field implies a different rendered tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveTailKey {
    /// Current terminal width, which affects wrapping.
    width: u16,
    /// Revision that changes on in-place active cell transcript updates.
    revision: u64,
    /// Whether the tail should be treated as a continuation for spacing.
    is_stream_continuation: bool,
    /// Optional animation tick to refresh spinners/progress indicators.
    animation_tick: Option<u64>,
}

impl TranscriptOverlay {
    /// Creates a transcript overlay for a fixed set of committed cells.
    ///
    /// This overlay does not own the "active cell"; callers may optionally append a live tail via
    /// `sync_live_tail` during draws to reflect in-flight activity.
    pub(crate) fn new(transcript_cells: Vec<Arc<dyn HistoryCell>>) -> Self {
        Self {
            view: PagerView::new(
                Self::render_cells(&transcript_cells, None),
                "T R A N S C R I P T".to_string(),
                usize::MAX,
            ),
            cells: transcript_cells,
            highlight_cell: None,
            live_tail_key: None,
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

    /// Insert a committed history cell while keeping any cached live tail.
    ///
    /// The live tail is temporarily removed, the committed cells are rebuilt,
    /// then the tail is reattached. If the tail previously had no leading
    /// spacing because it was the only renderable, we add the missing inset
    /// when the first committed cell arrives.
    ///
    /// This expects `cell` to be a committed transcript cell (not the in-flight active cell). If
    /// the overlay was scrolled to bottom before insertion, it remains pinned to bottom after the
    /// insertion to preserve the "follow along" behavior.
    pub(crate) fn insert_cell(&mut self, cell: Arc<dyn HistoryCell>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        let had_prior_cells = !self.cells.is_empty();
        let tail_renderable = self.take_live_tail_renderable();
        self.cells.push(cell);
        self.view.renderables = Self::render_cells(&self.cells, self.highlight_cell);
        if let Some(tail) = tail_renderable {
            let tail = if !had_prior_cells
                && self
                    .live_tail_key
                    .is_some_and(|key| !key.is_stream_continuation)
            {
                // The tail was rendered as the only entry, so it lacks a top
                // inset; add one now that it follows a committed cell.
                Box::new(InsetRenderable::new(tail, Insets::tlbr(1, 0, 0, 0)))
                    as Box<dyn Renderable>
            } else {
                tail
            };
            self.view.renderables.push(tail);
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    /// Sync the active-cell live tail with the current width and cell state.
    ///
    /// Recomputes the tail only when the cache key changes, preserving scroll
    /// position and dropping the tail if there is nothing to render.
    ///
    /// The overlay owns committed transcript cells while the live tail is derived from the current
    /// active cell, which can mutate in place while streaming. `App` calls this during
    /// `TuiEvent::Draw` for `Overlay::Transcript`, passing a key that changes when the active cell
    /// mutates or animates so the cached tail stays fresh.
    ///
    /// Passing a key that does not change on in-place active-cell mutations will freeze the tail in
    /// `Ctrl+T` while the main viewport continues to update.
    pub(crate) fn sync_live_tail(
        &mut self,
        width: u16,
        active_key: Option<ActiveCellTranscriptKey>,
        compute_lines: impl FnOnce(u16) -> Option<Vec<Line<'static>>>,
    ) {
        let next_key = active_key.map(|key| LiveTailKey {
            width,
            revision: key.revision,
            is_stream_continuation: key.is_stream_continuation,
            animation_tick: key.animation_tick,
        });

        if self.live_tail_key == next_key {
            return;
        }
        let follow_bottom = self.view.is_scrolled_to_bottom();

        self.take_live_tail_renderable();
        self.live_tail_key = next_key;

        if let Some(key) = next_key {
            let lines = compute_lines(width).unwrap_or_default();
            if !lines.is_empty() {
                self.view.renderables.push(Self::live_tail_renderable(
                    lines,
                    !self.cells.is_empty(),
                    key.is_stream_continuation,
                ));
            }
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    pub(crate) fn set_highlight_cell(&mut self, cell: Option<usize>) {
        self.highlight_cell = cell;
        self.rebuild_renderables();
        if let Some(idx) = self.highlight_cell {
            self.view.scroll_chunk_into_view(idx);
        }
    }

    /// Returns whether the underlying pager view is currently pinned to the bottom.
    ///
    /// This is used by the `App` draw loop to decide whether to schedule animation frames for the
    /// live tail (if the user has scrolled up, we avoid driving animation).
    pub(crate) fn is_scrolled_to_bottom(&self) -> bool {
        self.view.is_scrolled_to_bottom()
    }

    fn rebuild_renderables(&mut self) {
        let tail_renderable = self.take_live_tail_renderable();
        self.view.renderables = Self::render_cells(&self.cells, self.highlight_cell);
        if let Some(tail) = tail_renderable {
            self.view.renderables.push(tail);
        }
    }

    /// Removes and returns the cached live-tail renderable, if present.
    ///
    /// The live tail is represented as a single optional renderable appended after the committed
    /// cell renderables, so this relies on the live tail always being the final entry in
    /// `view.renderables` when present.
    fn take_live_tail_renderable(&mut self) -> Option<Box<dyn Renderable>> {
        (self.view.renderables.len() > self.cells.len()).then(|| self.view.renderables.pop())?
    }

    fn live_tail_renderable(
        lines: Vec<Line<'static>>,
        has_prior_cells: bool,
        is_stream_continuation: bool,
    ) -> Box<dyn Renderable> {
        let paragraph = Paragraph::new(Text::from(lines));
        let mut renderable: Box<dyn Renderable> = Box::new(CachedRenderable::new(paragraph));
        if has_prior_cells && !is_stream_continuation {
            renderable = Box::new(InsetRenderable::new(renderable, Insets::tlbr(1, 0, 0, 0)));
        }
        renderable
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_key_hints(line1, buf, PAGER_KEY_HINTS);

        let mut pairs: Vec<(&[KeyBinding], &str)> = vec![(&[KEY_Q], "to quit")];
        if self.highlight_cell.is_some() {
            pairs.push((&[KEY_ESC, KEY_LEFT], "to edit prev"));
            pairs.push((&[KEY_RIGHT], "to edit next"));
            pairs.push((&[KEY_ENTER], "to edit message"));
        } else {
            pairs.push((&[KEY_ESC], "to edit prev"));
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
            TuiEvent::Mouse(mouse_event) => self.view.handle_mouse_scroll(tui, mouse_event),
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
            TuiEvent::Mouse(mouse_event) => self.view.handle_mouse_scroll(tui, mouse_event),
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
    use pretty_assertions::assert_eq;
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

        // Render into a wide buffer so the footer hints aren't truncated.
        let area = Rect::new(0, 0, 120, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let s = buffer_to_text(&buf, area);
        assert!(
            s.contains("edit prev"),
            "expected 'edit prev' hint in overlay footer, got: {s:?}"
        );
    }

    #[test]
    fn edit_next_hint_is_visible_when_highlighted() {
        let mut overlay = TranscriptOverlay::new(vec![Arc::new(TestCell {
            lines: vec![Line::from("hello")],
        })]);
        overlay.set_highlight_cell(Some(0));

        // Render into a wide buffer so the footer hints aren't truncated.
        let area = Rect::new(0, 0, 120, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let s = buffer_to_text(&buf, area);
        assert!(
            s.contains("edit next"),
            "expected 'edit next' hint in overlay footer, got: {s:?}"
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

    #[test]
    fn transcript_overlay_renders_live_tail() {
        let mut overlay = TranscriptOverlay::new(vec![Arc::new(TestCell {
            lines: vec![Line::from("alpha")],
        })]);
        overlay.sync_live_tail(
            40,
            Some(ActiveCellTranscriptKey {
                revision: 1,
                is_stream_continuation: false,
                animation_tick: None,
            }),
            |_| Some(vec![Line::from("tail")]),
        );

        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    #[test]
    fn transcript_overlay_sync_live_tail_is_noop_for_identical_key() {
        let mut overlay = TranscriptOverlay::new(vec![Arc::new(TestCell {
            lines: vec![Line::from("alpha")],
        })]);

        let calls = std::cell::Cell::new(0usize);
        let key = ActiveCellTranscriptKey {
            revision: 1,
            is_stream_continuation: false,
            animation_tick: None,
        };

        overlay.sync_live_tail(40, Some(key), |_| {
            calls.set(calls.get() + 1);
            Some(vec![Line::from("tail")])
        });
        overlay.sync_live_tail(40, Some(key), |_| {
            calls.set(calls.get() + 1);
            Some(vec![Line::from("tail2")])
        });

        assert_eq!(calls.get(), 1);
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
        let approval_cell: Arc<dyn HistoryCell> =
            Arc::new(new_patch_event(approval_changes, &cwd, false));
        cells.push(approval_cell);

        let mut apply_changes = HashMap::new();
        apply_changes.insert(
            PathBuf::from("foo.txt"),
            FileChange::Add {
                content: "hello\nworld\n".to_string(),
            },
        );
        let apply_begin_cell: Arc<dyn HistoryCell> =
            Arc::new(new_patch_event(apply_changes, &cwd, false));
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
