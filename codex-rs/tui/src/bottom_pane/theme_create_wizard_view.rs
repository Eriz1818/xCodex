use std::cell::RefCell;

use codex_core::config::Config;
use codex_core::themes::ThemeCatalog;
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
use super::popup_consts::standard_popup_hint_line;
use super::textarea::TextArea;
use super::textarea::TextAreaState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemeCreateSource {
    Current,
    Default,
}

impl ThemeCreateSource {
    fn label(self) -> &'static str {
        match self {
            ThemeCreateSource::Current => "current",
            ThemeCreateSource::Default => "default",
        }
    }

    fn toggle(self) -> Self {
        match self {
            ThemeCreateSource::Current => ThemeCreateSource::Default,
            ThemeCreateSource::Default => ThemeCreateSource::Current,
        }
    }
}

pub(crate) struct ThemeCreateWizardView {
    config: Config,
    app_event_tx: AppEventSender,
    variant: ThemeVariant,
    source: ThemeCreateSource,
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    error_message: Option<String>,
    complete: bool,
}

impl ThemeCreateWizardView {
    pub(crate) fn new(config: Config, variant: ThemeVariant, app_event_tx: AppEventSender) -> Self {
        Self {
            config,
            app_event_tx,
            variant,
            source: ThemeCreateSource::Current,
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            error_message: None,
            complete: false,
        }
    }

    pub(crate) fn set_initial_name(&mut self, name: &str) {
        self.textarea.set_text(name);
    }

    fn toggle_variant(&mut self) {
        self.variant = match self.variant {
            ThemeVariant::Light => ThemeVariant::Dark,
            ThemeVariant::Dark => ThemeVariant::Light,
        };
    }

    fn toggle_source(&mut self) {
        self.source = self.source.toggle();
    }

    fn submit(&mut self) {
        let name = self
            .textarea
            .text()
            .lines()
            .next()
            .unwrap_or_default()
            .trim();
        if name.is_empty() {
            self.error_message = Some("Theme name cannot be empty.".to_string());
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

        let catalog = match ThemeCatalog::load(&self.config) {
            Ok(catalog) => catalog,
            Err(err) => {
                self.error_message = Some(format!("Failed to load themes: {err}"));
                return;
            }
        };

        if catalog.get(name).is_some() {
            self.error_message = Some(format!(
                "Theme `{name}` already exists. Pick a different name."
            ));
            return;
        }

        let base_theme = match self.source {
            ThemeCreateSource::Default => ThemeCatalog::built_in_default(),
            ThemeCreateSource::Current => {
                let current = match self.variant {
                    ThemeVariant::Light => self.config.themes.light.as_deref(),
                    ThemeVariant::Dark => self.config.themes.dark.as_deref(),
                }
                .unwrap_or("default");
                catalog
                    .get(current)
                    .cloned()
                    .unwrap_or_else(ThemeCatalog::built_in_default)
            }
        };

        let mut theme = base_theme;
        theme.name = name.to_string();
        theme.variant = self.variant;

        let yaml = match theme.to_yaml() {
            Ok(yaml) => yaml,
            Err(err) => {
                self.error_message = Some(format!("Failed to serialize theme YAML: {err}"));
                return;
            }
        };

        let path = unique_theme_path(&dir, name);
        if let Err(err) = std::fs::write(&path, yaml) {
            self.error_message = Some(format!("Failed to write `{}`: {err}", path.display()));
            return;
        }

        let variant_label = match self.variant {
            ThemeVariant::Light => "Light",
            ThemeVariant::Dark => "Dark",
        };
        let source_label = self.source.label();
        let lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                "Created theme ".into(),
                format!("`{name}`").cyan(),
                " (".into(),
                variant_label.into(),
                ", from ".into(),
                source_label.into(),
                ")".into(),
            ]),
            Line::from(vec![
                "File: ".dim(),
                path.display().to_string().cyan().underlined(),
            ]),
            "".into(),
            Line::from(
                "Tip: re-run `/theme` any time to switch, and edit the YAML to tweak colors.".dim(),
            ),
        ];
        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
            PlainHistoryCell::new(lines),
        )));
        self.app_event_tx.send(AppEvent::PersistThemeSelection {
            variant: self.variant,
            theme: name.to_string(),
        });
        self.complete = true;
    }
}

impl BottomPaneView for ThemeCreateWizardView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.on_ctrl_c();
            }
            KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                self.toggle_variant();
            }
            KeyEvent {
                code: KeyCode::Char('s'),
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                self.toggle_source();
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                self.submit();
            }
            other => {
                self.textarea.input(other);
            }
        }
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        if pasted.is_empty() {
            return false;
        }
        self.textarea.insert_str(&pasted);
        true
    }
}

impl Renderable for ThemeCreateWizardView {
    fn desired_height(&self, width: u16) -> u16 {
        let input_width = width.saturating_sub(2).max(1);
        let input_height = self.textarea.desired_height(input_width).clamp(1, 3);
        let error_height = if self.error_message.is_some() { 1 } else { 0 };
        1 + 1 + error_height + 1 + input_height + 1 + 1
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.height < 5 || area.width <= 2 {
            return None;
        }

        let input_width = area.width.saturating_sub(2).max(1);
        let input_height = self.textarea.desired_height(input_width).clamp(1, 3);
        let error_height = if self.error_message.is_some() { 1 } else { 0 };

        let textarea_rect = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1 + 1 + error_height + 1),
            width: input_width,
            height: input_height,
        };
        let state = *self.textarea_state.borrow();
        self.textarea.cursor_pos_with_state(textarea_rect, state)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut y = area.y;

        Paragraph::new(Line::from(vec![
            Span::from("Theme create").bold(),
            " — create a new YAML theme and select it".dim(),
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

        let variant_label = match self.variant {
            ThemeVariant::Light => "Light",
            ThemeVariant::Dark => "Dark",
        };
        let source_label = self.source.label();
        Paragraph::new(Line::from(vec![
            "Variant: ".dim(),
            variant_label.into(),
            "  ".into(),
            "Source: ".dim(),
            source_label.into(),
            "  ".into(),
            "(Tab toggles variant, s toggles source)".dim(),
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

        let input_width = area.width.saturating_sub(2).max(1);
        let input_height = self.textarea.desired_height(input_width).clamp(1, 3);
        let textarea_rect = Rect {
            x: area.x.saturating_add(2),
            y,
            width: input_width,
            height: input_height,
        };
        let mut state = self.textarea_state.borrow_mut();
        Clear.render(textarea_rect, buf);
        StatefulWidgetRef::render_ref(&(&self.textarea), textarea_rect, buf, &mut state);
        if self.textarea.text().is_empty() {
            Paragraph::new(Line::from("Theme name (used for selection)".dim()))
                .render(textarea_rect, buf);
        }
        y = y.saturating_add(input_height);

        y = y.saturating_add(1);

        Paragraph::new(standard_popup_hint_line()).render(
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
