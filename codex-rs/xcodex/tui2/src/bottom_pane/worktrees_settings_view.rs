use std::cell::RefCell;
use std::path::Path;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::measure_rows_height;
use super::selection_popup_common::render_rows;
use super::textarea::TextArea;
use super::textarea::TextAreaState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveList {
    SharedDirs,
    PinnedPaths,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    List,
    AddEntry,
}

pub(crate) struct WorktreesSettingsView {
    step: Step,
    active_list: ActiveList,
    complete: bool,
    shared_dirs: Vec<String>,
    pinned_paths: Vec<String>,
    state: ScrollState,
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    app_event_tx: AppEventSender,
}

impl WorktreesSettingsView {
    pub(crate) fn new(
        shared_dirs: Vec<String>,
        pinned_paths: Vec<String>,
        app_event_tx: AppEventSender,
    ) -> Self {
        let mut view = Self {
            step: Step::List,
            active_list: ActiveList::SharedDirs,
            complete: false,
            shared_dirs,
            pinned_paths,
            state: ScrollState::new(),
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            app_event_tx,
        };
        view.state.selected_idx = Some(0);
        view
    }

    fn footer_line(&self) -> Line<'static> {
        match self.step {
            Step::List => vec![
                "Tab".cyan(),
                " = switch list, ".dim(),
                "Enter".cyan(),
                " = remove/add, ".dim(),
                "Esc".cyan(),
                " = close".dim(),
            ]
            .into(),
            Step::AddEntry => vec![
                "Enter".cyan(),
                " = add, ".dim(),
                "Esc".cyan(),
                " = cancel".dim(),
            ]
            .into(),
        }
    }

    fn tab_line(&self) -> Line<'static> {
        let shared = match self.active_list {
            ActiveList::SharedDirs => "[ Shared dirs ]".bold().cyan(),
            ActiveList::PinnedPaths => "[ Shared dirs ]".dim(),
        };
        let pinned = match self.active_list {
            ActiveList::PinnedPaths => "[ Pinned paths ]".bold().cyan(),
            ActiveList::SharedDirs => "[ Pinned paths ]".dim(),
        };

        vec![shared, "  ".into(), pinned].into()
    }

    fn header(&self) -> ColumnRenderable<'_> {
        let mut header = ColumnRenderable::new();
        header.push(Line::from("Settings — Worktrees".bold()));
        header.push(self.tab_line());
        header.push(Line::from(""));
        match self.active_list {
            ActiveList::SharedDirs => {
                header.push(Line::from(vec![
                    "Shared dirs: ".dim(),
                    "linked into the workspace root (writes persist across worktrees)".into(),
                ]));
            }
            ActiveList::PinnedPaths => {
                header.push(Line::from(vec![
                    "Pinned paths: ".dim(),
                    "file tools resolve these under workspace root".into(),
                ]));
            }
        }
        header
    }

    fn active_items(&self) -> &Vec<String> {
        match self.active_list {
            ActiveList::SharedDirs => &self.shared_dirs,
            ActiveList::PinnedPaths => &self.pinned_paths,
        }
    }

    fn active_items_mut(&mut self) -> &mut Vec<String> {
        match self.active_list {
            ActiveList::SharedDirs => &mut self.shared_dirs,
            ActiveList::PinnedPaths => &mut self.pinned_paths,
        }
    }

    fn list_visible_len(&self) -> usize {
        self.active_items().len() + 1
    }

    fn move_up(&mut self) {
        let len = self.list_visible_len();
        if len == 0 {
            return;
        }
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    fn move_down(&mut self) {
        let len = self.list_visible_len();
        if len == 0 {
            return;
        }
        self.state.move_down_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    fn switch_list(&mut self) {
        self.active_list = match self.active_list {
            ActiveList::SharedDirs => ActiveList::PinnedPaths,
            ActiveList::PinnedPaths => ActiveList::SharedDirs,
        };
        let len = self.list_visible_len();
        if len == 0 {
            self.state.selected_idx = None;
        } else if self.state.selected_idx.is_none() {
            self.state.selected_idx = Some(0);
        } else if let Some(idx) = self.state.selected_idx {
            self.state.selected_idx = Some(idx.min(len.saturating_sub(1)));
        }
    }

    fn validate_entry(active_list: ActiveList, raw: &str) -> Result<String, String> {
        use std::path::Component;

        let mut value = raw.trim().trim_end_matches(['/', '\\']).to_string();
        while value.starts_with("./") {
            value = value.trim_start_matches("./").to_string();
        }

        if value.is_empty() {
            return Err(String::from("value is empty"));
        }
        if value.starts_with('~') {
            return Err(String::from("paths must be repo-relative (no '~')"));
        }

        let path = Path::new(&value);
        if path.is_absolute() {
            return Err(String::from(
                "paths must be repo-relative (no absolute paths)",
            ));
        }

        for component in path.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(String::from(
                        "paths must not contain parent/root components",
                    ));
                }
                Component::CurDir => {}
                Component::Normal(_) => {}
            }
        }

        if matches!(active_list, ActiveList::SharedDirs) && value.contains(['*', '?', '[', ']']) {
            return Err(String::from(
                "shared dirs must be plain directories (no globs)",
            ));
        }

        Ok(value)
    }

    fn persist_active_list(&self) {
        match self.active_list {
            ActiveList::SharedDirs => {
                let shared_dirs = self.shared_dirs.clone();
                self.app_event_tx.send(AppEvent::UpdateWorktreesSharedDirs {
                    shared_dirs: shared_dirs.clone(),
                });
                self.app_event_tx
                    .send(AppEvent::PersistWorktreesSharedDirs { shared_dirs });
            }
            ActiveList::PinnedPaths => {
                let pinned_paths = self.pinned_paths.clone();
                self.app_event_tx
                    .send(AppEvent::UpdateWorktreesPinnedPaths {
                        pinned_paths: pinned_paths.clone(),
                    });
                self.app_event_tx
                    .send(AppEvent::PersistWorktreesPinnedPaths { pinned_paths });
            }
        }
    }

    fn start_add_entry(&mut self) {
        self.step = Step::AddEntry;
        self.textarea.set_text("");
    }

    fn apply_list_action(&mut self) {
        let Some(idx) = self.state.selected_idx else {
            return;
        };

        let len = self.active_items().len();
        if idx >= len {
            self.start_add_entry();
            return;
        }

        self.active_items_mut().remove(idx);
        self.persist_active_list();

        let len = self.list_visible_len();
        self.state.selected_idx = Some(idx.min(len.saturating_sub(1)));
    }

    fn commit_add_entry(&mut self) {
        let raw = self.textarea.text().trim().to_string();
        if raw.is_empty() {
            self.step = Step::List;
            return;
        }

        let entry = match Self::validate_entry(self.active_list, &raw) {
            Ok(entry) => entry,
            Err(_err) => {
                self.step = Step::List;
                return;
            }
        };

        {
            let items = self.active_items_mut();
            if items.iter().any(|existing| existing == &entry) {
                self.step = Step::List;
                return;
            }
            items.push(entry.clone());
            items.sort();
        }
        self.persist_active_list();

        self.step = Step::List;
        let selected_idx = self
            .active_items()
            .iter()
            .position(|existing| existing == &entry)
            .unwrap_or(0);
        self.state.selected_idx = Some(selected_idx);
    }

    fn build_rows(&self) -> Vec<GenericDisplayRow> {
        let selected_idx = self.state.selected_idx;
        let mut rows: Vec<GenericDisplayRow> = Vec::new();
        let items = self.active_items();
        if items.is_empty() {
            rows.push(GenericDisplayRow {
                name: String::from("  (none)"),
                display_shortcut: None,
                match_indices: None,
                description: None,
                disabled_reason: None,
                is_dimmed: false,
                wrap_indent: None,
            });
        } else {
            for (idx, item) in items.iter().enumerate() {
                let prefix = if selected_idx == Some(idx) {
                    '›'
                } else {
                    ' '
                };
                rows.push(GenericDisplayRow {
                    name: format!("{prefix} {item}"),
                    description: Some(String::from("Enter to remove")),
                    display_shortcut: None,
                    match_indices: None,
                    disabled_reason: None,
                    is_dimmed: false,
                    wrap_indent: None,
                });
            }
        }

        let add_idx = items.len();
        let prefix = if selected_idx == Some(add_idx) {
            '›'
        } else {
            ' '
        };
        rows.push(GenericDisplayRow {
            name: format!("{prefix} [+] Add…"),
            description: Some(String::from("Enter to add a new entry")),
            display_shortcut: None,
            match_indices: None,
            disabled_reason: None,
            is_dimmed: false,
            wrap_indent: None,
        });

        rows
    }
}

impl BottomPaneView for WorktreesSettingsView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match self.step {
            Step::List => match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.complete = true,
                KeyEvent {
                    code: KeyCode::Tab,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.switch_list(),
                KeyEvent {
                    code: KeyCode::Up, ..
                } => self.move_up(),
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                } => self.move_down(),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.apply_list_action(),
                _ => {}
            },
            Step::AddEntry => match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.step = Step::List,
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.commit_add_entry(),
                other => self.textarea.input(other),
            },
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

impl Renderable for WorktreesSettingsView {
    fn desired_height(&self, width: u16) -> u16 {
        let content_width = width.saturating_sub(4);
        let header_height = self.header().desired_height(content_width);
        let body_height = match self.step {
            Step::List => {
                let rows = self.build_rows();
                measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, content_width)
            }
            Step::AddEntry => 6,
        };

        header_height
            .saturating_add(body_height)
            .saturating_add(4)
            .min(24)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        Clear.render(area, buf);
        let [content_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        let base_style = user_message_style().patch(crate::theme::composer_style());
        Block::default().style(base_style).render(content_area, buf);
        Block::default().style(base_style).render(footer_area, buf);

        let inner = content_area.inset(Insets::vh(1, 2));
        let header = self.header();
        let header_height = header.desired_height(inner.width);
        let [header_area, body_area] =
            Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)]).areas(inner);
        header.render(header_area, buf);

        match self.step {
            Step::List => {
                let rows = self.build_rows();
                render_rows(
                    body_area,
                    buf,
                    &rows,
                    &self.state,
                    MAX_POPUP_ROWS,
                    base_style,
                    "  (no entries)",
                );
            }
            Step::AddEntry => {
                let block = Block::default().style(base_style);
                let [title_area, input_area] =
                    Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(body_area);
                block.render(body_area, buf);
                Paragraph::new(Line::from(vec![
                    "Add ".dim(),
                    match self.active_list {
                        ActiveList::SharedDirs => "shared dir".into(),
                        ActiveList::PinnedPaths => "pinned path".into(),
                    },
                    ":".dim(),
                ]))
                .render(title_area, buf);

                StatefulWidgetRef::render_ref(
                    &(&self.textarea),
                    input_area.inset(Insets::vh(0, 0)),
                    buf,
                    &mut self.textarea_state.borrow_mut(),
                );
            }
        }

        self.footer_line().dim().style(base_style).render(
            Rect {
                x: footer_area.x + 2,
                y: footer_area.y,
                width: footer_area.width.saturating_sub(2),
                height: 1,
            },
            buf,
        );
    }
}
