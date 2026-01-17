use std::cell::RefCell;

use codex_core::config::Config;
use codex_core::themes::ThemeCatalog;
use codex_core::themes::ThemeColor;
use codex_core::themes::ThemeDefinition;
use codex_core::themes::ThemeVariant;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Styled as _;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget as _;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell::PlainHistoryCell;
use crate::render::renderable::Renderable;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::textarea::TextArea;
use super::textarea::TextAreaState;

const LIST_HEIGHT: u16 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tab {
    Roles,
    Palette,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Roles => "roles",
            Tab::Palette => "palette",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Tab::Roles => Tab::Palette,
            Tab::Palette => Tab::Roles,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    List,
    Name,
    Value,
}

pub(crate) struct ThemeEditorView {
    config: Config,
    app_event_tx: AppEventSender,
    variant: ThemeVariant,
    base_theme_name: String,
    tab: Tab,
    focus: Focus,

    theme: ThemeDefinition,
    selected: usize,
    scroll_top: usize,

    save_name: TextArea,
    save_name_state: RefCell<TextAreaState>,

    value_editor: TextArea,
    value_editor_state: RefCell<TextAreaState>,
    editing_key: Option<&'static str>,

    error_message: Option<String>,
    complete: bool,
}

impl ThemeEditorView {
    pub(crate) fn new(
        config: Config,
        variant: ThemeVariant,
        base_theme_name: String,
        base_theme: ThemeDefinition,
        suggested_name: String,
        app_event_tx: AppEventSender,
    ) -> Self {
        let mut save_name = TextArea::new();
        save_name.set_text(&suggested_name);

        let mut theme = base_theme;
        theme.variant = variant;
        theme.name = suggested_name;

        let view = Self {
            config,
            app_event_tx,
            variant,
            base_theme_name,
            tab: Tab::Roles,
            focus: Focus::List,
            theme,
            selected: 0,
            scroll_top: 0,
            save_name,
            save_name_state: RefCell::new(TextAreaState::default()),
            value_editor: TextArea::new(),
            value_editor_state: RefCell::new(TextAreaState::default()),
            editing_key: None,
            error_message: None,
            complete: false,
        };
        view.apply_preview();
        view
    }

    fn apply_preview(&self) {
        crate::theme::preview_definition(&self.theme);
    }

    fn cancel(&mut self) {
        self.app_event_tx.send(AppEvent::CancelThemePreview);
        self.complete = true;
    }

    fn keys(&self) -> &'static [&'static str] {
        match self.tab {
            Tab::Roles => &ROLE_KEYS,
            Tab::Palette => &PALETTE_KEYS,
        }
    }

    fn visible_len(&self) -> usize {
        self.keys().len()
    }

    fn selected_key(&self) -> Option<&'static str> {
        self.keys().get(self.selected).copied()
    }

    fn move_up(&mut self) {
        if self.visible_len() == 0 {
            return;
        }
        if self.selected == 0 {
            self.selected = self.visible_len() - 1;
        } else {
            self.selected -= 1;
        }
        self.ensure_visible();
    }

    fn move_down(&mut self) {
        if self.visible_len() == 0 {
            return;
        }
        self.selected = (self.selected + 1) % self.visible_len();
        self.ensure_visible();
    }

    fn ensure_visible(&mut self) {
        let height = LIST_HEIGHT as usize;
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + height {
            self.scroll_top = self.selected.saturating_sub(height - 1);
        }
    }

    fn toggle_tab(&mut self) {
        self.tab = self.tab.toggle();
        self.selected = 0;
        self.scroll_top = 0;
    }

    fn toggle_focus_name(&mut self) {
        self.focus = match self.focus {
            Focus::Name => Focus::List,
            _ => Focus::Name,
        };
    }

    fn start_value_edit(&mut self) {
        let Some(key) = self.selected_key() else {
            return;
        };
        self.error_message = None;
        let current = match self.tab {
            Tab::Roles => role_value(&self.theme, key).unwrap_or_default(),
            Tab::Palette => palette_value(&self.theme, key),
        };
        self.value_editor.set_text(&current);
        self.editing_key = Some(key);
        self.focus = Focus::Value;
    }

    fn commit_value_edit(&mut self) {
        let Some(key) = self.editing_key else {
            self.focus = Focus::List;
            return;
        };
        let value = self
            .value_editor
            .text()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();

        let set_result = match self.tab {
            Tab::Roles => set_role_value(&mut self.theme, key, value.as_str()),
            Tab::Palette => {
                set_palette_value(&mut self.theme, key, value.as_str());
                Ok(())
            }
        };

        match set_result {
            Ok(()) => {
                self.error_message = None;
                self.apply_preview();
                self.focus = Focus::List;
            }
            Err(err) => {
                self.error_message = Some(err);
            }
        }
    }

    fn cancel_value_edit(&mut self) {
        self.value_editor.set_text("");
        self.editing_key = None;
        self.error_message = None;
        self.focus = Focus::List;
    }

    fn save(&mut self) {
        let save_name = self
            .save_name
            .text()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if save_name.is_empty() {
            self.error_message = Some("Theme name cannot be empty.".to_string());
            return;
        }

        let mut theme = self.theme.clone();
        theme.name = save_name.clone();
        theme.variant = self.variant;

        if let Err(err) = theme.validate() {
            self.error_message = Some(format!("Theme is not valid: {err}"));
            return;
        }

        let catalog = match ThemeCatalog::load(&self.config) {
            Ok(catalog) => catalog,
            Err(err) => {
                self.error_message = Some(format!("Failed to load themes: {err}"));
                return;
            }
        };
        if catalog.get(save_name.as_str()).is_some() {
            self.error_message = Some(format!(
                "Theme `{save_name}` already exists. Pick a different name."
            ));
            return;
        }

        let dir = codex_core::themes::themes_dir(&self.config.codex_home, &self.config.themes);
        if let Err(err) = std::fs::create_dir_all(&dir) {
            self.error_message = Some(format!(
                "Failed to create themes directory `{}`: {err}",
                dir.display()
            ));
            return;
        }

        let yaml = match theme.to_yaml() {
            Ok(yaml) => yaml,
            Err(err) => {
                self.error_message = Some(format!("Failed to serialize theme YAML: {err}"));
                return;
            }
        };

        let path = unique_theme_path(&dir, save_name.as_str());
        if let Err(err) = std::fs::write(&path, yaml) {
            self.error_message = Some(format!("Failed to write `{}`: {err}", path.display()));
            return;
        }

        let variant_label = match self.variant {
            ThemeVariant::Light => "Light",
            ThemeVariant::Dark => "Dark",
        };
        let lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                "Saved theme ".into(),
                format!("`{save_name}`").cyan(),
                " (".into(),
                variant_label.into(),
                ", edited from ".into(),
                format!("`{}`", self.base_theme_name).dim(),
                ")".into(),
            ]),
            Line::from(vec![
                "File: ".dim(),
                path.display().to_string().cyan().underlined(),
            ]),
        ];
        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
            PlainHistoryCell::new(lines),
        )));
        self.app_event_tx.send(AppEvent::PersistThemeSelection {
            variant: self.variant,
            theme: save_name,
        });
        self.complete = true;
    }
}

impl BottomPaneView for ThemeEditorView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        self.error_message = None;
        match (self.focus, key_event) {
            (
                _,
                KeyEvent {
                    code: KeyCode::Esc, ..
                },
            ) => match self.focus {
                Focus::Value => self.cancel_value_edit(),
                Focus::Name => self.focus = Focus::List,
                Focus::List => self.cancel(),
            },
            (
                _,
                KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                },
            ) => self.cancel(),
            (
                _,
                KeyEvent {
                    code: KeyCode::Char('s'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                },
            ) => self.save(),
            (
                Focus::List,
                KeyEvent {
                    code: KeyCode::Tab,
                    modifiers: KeyModifiers::NONE,
                    ..
                },
            ) => self.toggle_tab(),
            (
                Focus::List,
                KeyEvent {
                    code: KeyCode::Up, ..
                },
            ) => self.move_up(),
            (
                Focus::List,
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                },
            ) => self.move_down(),
            (
                Focus::List,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                },
            ) => self.start_value_edit(),
            (
                Focus::List,
                KeyEvent {
                    code: KeyCode::Char('n'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                },
            ) => self.toggle_focus_name(),
            (
                Focus::Name,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                },
            ) => self.focus = Focus::List,
            (Focus::Name, other) => {
                self.save_name.input(other);
            }
            (
                Focus::Value,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                },
            ) => self.commit_value_edit(),
            (Focus::Value, other) => {
                self.value_editor.input(other);
            }
            _ => {}
        }
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.cancel();
        CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
}

impl Renderable for ThemeEditorView {
    fn desired_height(&self, width: u16) -> u16 {
        let input_width = width.saturating_sub(2).max(1);
        let name_height = self.save_name.desired_height(input_width).clamp(1, 1);
        let value_height = if self.focus == Focus::Value {
            self.value_editor.desired_height(input_width).clamp(1, 1)
        } else {
            0
        };
        let error_height = if self.error_message.is_some() { 1 } else { 0 };

        // title + name + error? + blank + list + value? + hint
        1 + name_height + error_height + 1 + LIST_HEIGHT + value_height + 1
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width <= 2 || area.height < 5 {
            return None;
        }
        let input_width = area.width.saturating_sub(2).max(1);
        let name_height = 1u16;
        let error_height = if self.error_message.is_some() {
            1u16
        } else {
            0
        };

        match self.focus {
            Focus::Name => {
                let rect = Rect {
                    x: area.x.saturating_add(2),
                    y: area.y.saturating_add(1),
                    width: input_width,
                    height: name_height,
                };
                let state = *self.save_name_state.borrow();
                self.save_name.cursor_pos_with_state(rect, state)
            }
            Focus::Value => {
                let y = area
                    .y
                    .saturating_add(1 + name_height + error_height + 1 + LIST_HEIGHT);
                let rect = Rect {
                    x: area.x.saturating_add(2),
                    y,
                    width: input_width,
                    height: 1,
                };
                let state = *self.value_editor_state.borrow();
                self.value_editor.cursor_pos_with_state(rect, state)
            }
            Focus::List => None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let input_width = area.width.saturating_sub(2).max(1);
        let variant_label = match self.variant {
            ThemeVariant::Light => "Light",
            ThemeVariant::Dark => "Dark",
        };

        let mut y = area.y;

        Paragraph::new(Line::from(vec![
            Span::from("Theme editor").bold(),
            " — live preview + save as YAML".dim(),
        ]))
        .render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            buf,
        );
        y = y.saturating_add(1);

        let name_prefix = if self.focus == Focus::Name {
            "> "
        } else {
            "  "
        };
        let name_rect = Rect {
            x: area.x.saturating_add(2),
            y,
            width: input_width,
            height: 1,
        };
        Paragraph::new(Line::from(vec![
            Span::from(name_prefix).dim(),
            "Save as: ".dim(),
            format!("({variant_label}) ").dim(),
        ]))
        .render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            buf,
        );
        Clear.render(name_rect, buf);
        {
            let mut state = self.save_name_state.borrow_mut();
            StatefulWidgetRef::render_ref(&(&self.save_name), name_rect, buf, &mut state);
        }
        y = y.saturating_add(1);

        if let Some(msg) = self.error_message.as_deref() {
            Paragraph::new(Line::from(vec![
                Span::from("Error: ").set_style(crate::theme::error_style().bold()),
                msg.into(),
            ]))
            .render(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            y = y.saturating_add(1);
        }

        y = y.saturating_add(1);

        let tab_label = self.tab.label();
        let list_title = Line::from(vec![
            "Editing ".dim(),
            format!("`{}`", self.base_theme_name).dim(),
            " → ".dim(),
            format!("`{}`", self.theme.name).cyan(),
            "  ".into(),
            Span::from("[Tab] ").dim(),
            tab_label.cyan().bold(),
            "  ".into(),
            "(roles.dim is derived)".dim(),
        ]);
        Paragraph::new(list_title).render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            buf,
        );
        y = y.saturating_add(1);

        let list_rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: LIST_HEIGHT,
        };
        Clear.render(list_rect, buf);
        let lines = self.list_lines();
        Paragraph::new(lines).render(list_rect, buf);
        y = y.saturating_add(LIST_HEIGHT);

        if self.focus == Focus::Value {
            let value_rect = Rect {
                x: area.x.saturating_add(2),
                y,
                width: input_width,
                height: 1,
            };
            Paragraph::new(Line::from(vec!["Value: ".dim()])).render(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            Clear.render(value_rect, buf);
            let mut state = self.value_editor_state.borrow_mut();
            StatefulWidgetRef::render_ref(&(&self.value_editor), value_rect, buf, &mut state);
        }
        y = y.saturating_add(1);

        let hint = Line::from(vec![
            "Esc".dim(),
            " cancel  ".dim(),
            "Ctrl+N".dim(),
            " name  ".dim(),
            "↑/↓".dim(),
            " select  ".dim(),
            "Enter".dim(),
            " edit  ".dim(),
            "Ctrl+S".dim(),
            " save".dim(),
        ]);
        Paragraph::new(hint).render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            buf,
        );
    }
}

impl ThemeEditorView {
    fn list_lines(&self) -> Vec<Line<'static>> {
        let keys = self.keys();
        let start = self.scroll_top.min(keys.len().saturating_sub(1));
        let end = keys.len().min(start + LIST_HEIGHT as usize);

        let mut out: Vec<Line<'static>> = Vec::new();
        for (idx, key) in keys[start..end].iter().enumerate() {
            let absolute_idx = start + idx;
            let is_selected = self.focus == Focus::List && absolute_idx == self.selected;
            let marker = if absolute_idx == self.selected {
                "> ".bold()
            } else {
                "  ".into()
            };

            let (value, value_style) = match self.tab {
                Tab::Roles => match role_value(&self.theme, key) {
                    Some(value) if value.is_empty() => {
                        let label = if is_derived_role_key(key) {
                            "(derived)"
                        } else {
                            "(unset)"
                        };
                        (label.to_string(), crate::theme::dim_style())
                    }
                    Some(value) => {
                        let style = display_value_style(value.as_str());
                        (value, style)
                    }
                    None => ("(unset)".to_string(), crate::theme::dim_style()),
                },
                Tab::Palette => {
                    let value = palette_value(&self.theme, key);
                    let style = display_value_style(value.as_str());
                    (value, style)
                }
            };

            let mut line = Line::from(vec![
                marker,
                Span::from(*key).dim(),
                " = ".dim(),
                Span::from(value).set_style(value_style),
            ]);
            if is_selected {
                line = line.set_style(crate::theme::option_style(true, false));
            }
            out.push(line);
        }
        out
    }
}

fn display_value_style(value: &str) -> ratatui::style::Style {
    if value == "inherit" {
        crate::theme::dim_style()
    } else if value.starts_with("palette.") {
        crate::theme::accent_style()
    } else if value.starts_with('#') {
        ratatui::style::Style::default()
    } else {
        crate::theme::warning_style()
    }
}

fn is_derived_role_key(key: &'static str) -> bool {
    matches!(
        key,
        "roles.transcript_bg" | "roles.composer_bg" | "roles.status_bg"
    )
}

fn role_value(theme: &ThemeDefinition, key: &'static str) -> Option<String> {
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
        _ => None,
    }
}

fn set_role_value(
    theme: &mut ThemeDefinition,
    key: &'static str,
    value: &str,
) -> Result<(), String> {
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
        "roles.success" => &mut theme.roles.success,
        "roles.warning" => &mut theme.roles.warning,
        "roles.error" => &mut theme.roles.error,
        "roles.diff_add_fg" => &mut theme.roles.diff_add_fg,
        "roles.diff_add_bg" => &mut theme.roles.diff_add_bg,
        "roles.diff_del_fg" => &mut theme.roles.diff_del_fg,
        "roles.diff_del_bg" => &mut theme.roles.diff_del_bg,
        "roles.diff_hunk_fg" => &mut theme.roles.diff_hunk_fg,
        "roles.diff_hunk_bg" => &mut theme.roles.diff_hunk_bg,
        _ => return Err("Unknown role key.".to_string()),
    };
    dst.set(value.to_string());
    Ok(())
}

fn palette_value(theme: &ThemeDefinition, key: &'static str) -> String {
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

fn set_palette_value(theme: &mut ThemeDefinition, key: &'static str, value: &str) {
    let value = value.trim();
    let dst: &mut ThemeColor = match key {
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

const ROLE_KEYS: [&str; 23] = [
    "roles.fg",
    "roles.bg",
    "roles.transcript_bg",
    "roles.composer_bg",
    "roles.status_bg",
    "roles.selection_fg",
    "roles.selection_bg",
    "roles.cursor_fg",
    "roles.cursor_bg",
    "roles.border",
    "roles.accent",
    "roles.brand",
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
];

const PALETTE_KEYS: [&str; 16] = [
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
