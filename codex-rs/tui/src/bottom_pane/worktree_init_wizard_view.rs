use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;

use codex_common::fuzzy_match::fuzzy_match;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::popup_consts::MAX_POPUP_ROWS;
use super::popup_consts::standard_popup_hint_line;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::measure_rows_height;
use super::selection_popup_common::render_rows;
use super::textarea::TextArea;
use super::textarea::TextAreaState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BranchMode {
    Existing,
    CreateNew,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Name,
    BranchMode,
    BranchPicker,
    BranchName,
    BaseRef,
    Path,
    SharedDirs,
    AddSharedDir,
    Confirm,
}

#[derive(Clone, Debug)]
struct SharedDirChoice {
    dir: String,
    selected: bool,
    is_new: bool,
}

#[derive(Clone, Debug)]
struct Draft {
    name: String,
    branch_mode: BranchMode,
    branch: String,
    base_ref: String,
    path: String,
    shared_dirs: Vec<SharedDirChoice>,
}

pub(crate) struct WorktreeInitWizardView {
    worktree_root: PathBuf,
    workspace_root: PathBuf,
    invoked_from: &'static str,
    current_branch: Option<String>,
    complete: bool,
    step: Step,
    draft: Draft,
    branches: Vec<String>,
    branch_query: String,
    selection_state: ScrollState,
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    app_event_tx: AppEventSender,
}

impl WorktreeInitWizardView {
    pub(crate) fn new(
        worktree_root: PathBuf,
        workspace_root: PathBuf,
        current_branch: Option<String>,
        shared_dirs: Vec<String>,
        branches: Vec<String>,
        app_event_tx: AppEventSender,
    ) -> Self {
        let default_name = String::new();
        let default_branch = current_branch
            .clone()
            .unwrap_or_else(|| String::from("main"));
        let shared_dirs = shared_dirs
            .into_iter()
            .map(|dir| SharedDirChoice {
                dir,
                selected: true,
                is_new: false,
            })
            .collect();

        let mut view = Self {
            worktree_root,
            workspace_root,
            invoked_from: "/worktree init",
            current_branch,
            complete: false,
            step: Step::Name,
            draft: Draft {
                name: default_name,
                branch_mode: BranchMode::Existing,
                branch: default_branch,
                base_ref: String::from("HEAD"),
                path: String::new(),
                shared_dirs,
            },
            branches,
            branch_query: String::new(),
            selection_state: ScrollState::new(),
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            app_event_tx,
        };
        view.enter_step(Step::Name);
        view
    }

    fn enter_step(&mut self, step: Step) {
        self.step = step;
        match self.step {
            Step::Name => {
                self.textarea.set_text(self.draft.name.as_str());
            }
            Step::BranchMode => {
                self.selection_state.selected_idx = Some(0);
            }
            Step::BranchPicker => {
                self.branch_query.clear();
                let selected = self
                    .branches
                    .iter()
                    .position(|branch| branch == &self.draft.branch)
                    .unwrap_or(0);
                self.selection_state.selected_idx = Some(selected);
                self.selection_state.scroll_top = 0;
            }
            Step::BranchName => {
                self.textarea.set_text(self.draft.branch.as_str());
            }
            Step::BaseRef => {
                self.textarea.set_text(self.draft.base_ref.as_str());
            }
            Step::Path => {
                self.textarea.set_text(self.draft.path.as_str());
            }
            Step::SharedDirs => {
                if self.shared_dirs_visible_len() == 0 {
                    self.selection_state.selected_idx = None;
                } else if self.selection_state.selected_idx.is_none() {
                    self.selection_state.selected_idx = Some(0);
                }
            }
            Step::AddSharedDir => {
                self.textarea.set_text("");
            }
            Step::Confirm => {}
        }
    }

    fn shared_dirs_visible_len(&self) -> usize {
        // Extra row at end for "Add…"
        self.draft.shared_dirs.len() + 1
    }

    fn move_up(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        self.selection_state.move_up_wrap(len);
        self.selection_state
            .ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    fn move_down(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        self.selection_state.move_down_wrap(len);
        self.selection_state
            .ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    fn go_back(&mut self) {
        let prev = match self.step {
            Step::Name => {
                self.complete = true;
                return;
            }
            Step::BranchMode => Step::Name,
            Step::BranchPicker => Step::BranchMode,
            Step::BranchName => Step::BranchMode,
            Step::BaseRef => Step::BranchName,
            Step::Path => {
                if self.draft.branch_mode == BranchMode::CreateNew {
                    Step::BaseRef
                } else {
                    Step::BranchPicker
                }
            }
            Step::SharedDirs => Step::Path,
            Step::AddSharedDir => Step::SharedDirs,
            Step::Confirm => Step::SharedDirs,
        };
        self.enter_step(prev);
    }

    fn sanitize_worktree_path_slug(name: &str) -> String {
        let mut out = String::new();
        for ch in name.trim().chars() {
            match ch {
                '/' | '\\' => out.push('-'),
                ' ' | '\t' | '\n' | '\r' => out.push('-'),
                other => out.push(other),
            }
        }
        while out.contains("--") {
            out = out.replace("--", "-");
        }
        out.trim_matches('-').to_string()
    }

    fn default_worktree_path(&self) -> PathBuf {
        let slug = Self::sanitize_worktree_path_slug(&self.draft.name);
        self.workspace_root
            .join(".worktrees")
            .join(if slug.is_empty() {
                String::from("worktree")
            } else {
                slug
            })
    }

    fn resolve_worktree_path(&self) -> PathBuf {
        let raw = self.draft.path.trim();
        if raw.is_empty() {
            return self.default_worktree_path();
        }
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root.join(candidate)
        }
    }

    fn validate_shared_dir(raw: &str) -> Result<String, String> {
        use std::path::Component;

        let mut value = raw.trim().trim_end_matches(['/', '\\']).to_string();
        while value.starts_with("./") {
            value = value.trim_start_matches("./").to_string();
        }
        if value.is_empty() {
            return Err(String::from("shared dir is empty"));
        }
        if value.starts_with('~') {
            return Err(String::from("shared dirs must be repo-relative (no '~')"));
        }

        let path = Path::new(&value);
        if path.is_absolute() {
            return Err(String::from("shared dirs must be repo-relative"));
        }

        for component in path.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(String::from(
                        "shared dirs must not contain parent/root components",
                    ));
                }
                Component::CurDir => {}
                Component::Normal(_) => {}
            }
        }

        Ok(value)
    }

    fn toggle_shared_dir_selection(&mut self) {
        let Some(idx) = self.selection_state.selected_idx else {
            return;
        };

        if idx >= self.draft.shared_dirs.len() {
            // Add… row
            self.enter_step(Step::AddSharedDir);
            return;
        }

        if let Some(item) = self.draft.shared_dirs.get_mut(idx) {
            item.selected = !item.selected;
        }
    }

    fn commit_add_shared_dir(&mut self) {
        let raw = self.textarea.text().trim().to_string();
        if raw.is_empty() {
            self.enter_step(Step::SharedDirs);
            return;
        }

        let dir = match Self::validate_shared_dir(&raw) {
            Ok(dir) => dir,
            Err(err) => {
                self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!("`/worktree init` — {err}")),
                )));
                self.enter_step(Step::SharedDirs);
                return;
            }
        };

        if self.draft.shared_dirs.iter().any(|d| d.dir == dir) {
            self.enter_step(Step::SharedDirs);
            return;
        }

        self.draft.shared_dirs.push(SharedDirChoice {
            dir,
            selected: true,
            is_new: true,
        });
        self.enter_step(Step::SharedDirs);
    }

    fn step_title(&self) -> String {
        match self.step {
            Step::Name => String::from("Worktree init — name"),
            Step::BranchMode => String::from("Worktree init — branch mode"),
            Step::BranchPicker => String::from("Worktree init — branch (existing)"),
            Step::BranchName => String::from("Worktree init — branch name"),
            Step::BaseRef => String::from("Worktree init — base ref"),
            Step::Path => String::from("Worktree init — path"),
            Step::SharedDirs => String::from("Worktree init — shared dirs"),
            Step::AddSharedDir => String::from("Worktree init — add shared dir"),
            Step::Confirm => String::from("Worktree init — confirm"),
        }
    }

    fn header(&self) -> ColumnRenderable<'_> {
        let mut header = ColumnRenderable::new();
        header.push(Line::from(self.step_title().bold()));
        header.push(Line::from(vec![
            "Workspace root: ".dim(),
            self.workspace_root.display().to_string().into(),
        ]));
        header.push(Line::from(vec![
            "Active worktree: ".dim(),
            self.worktree_root.display().to_string().into(),
        ]));
        header
    }

    fn build_branch_mode_rows(&self) -> Vec<GenericDisplayRow> {
        let selected = self.selection_state.selected_idx.unwrap_or(0);
        let mut rows = Vec::with_capacity(2);
        let existing_prefix = if selected == 0 { '›' } else { ' ' };
        let new_prefix = if selected == 1 { '›' } else { ' ' };

        rows.push(GenericDisplayRow {
            name: format!("{existing_prefix} Use existing branch"),
            description: self
                .current_branch
                .as_ref()
                .map(|branch| format!("default: {branch}")),
            ..Default::default()
        });
        rows.push(GenericDisplayRow {
            name: format!("{new_prefix} Create new branch"),
            description: Some(String::from("choose a base ref on the next step")),
            ..Default::default()
        });
        rows
    }

    fn build_branch_picker_rows(&self) -> Vec<GenericDisplayRow> {
        let selected_idx = self.selection_state.selected_idx;
        self.filtered_branch_matches()
            .into_iter()
            .enumerate()
            .map(|(idx, (branch, indices, _score))| {
                let prefix = if selected_idx == Some(idx) {
                    '›'
                } else {
                    ' '
                };
                let description = self
                    .current_branch
                    .as_deref()
                    .filter(|current| *current == branch)
                    .map(|_| String::from("current"));
                GenericDisplayRow {
                    name: format!("{prefix} {branch}"),
                    match_indices: indices.map(|v| v.into_iter().map(|i| i + 2).collect()),
                    description,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn filtered_branch_matches(&self) -> Vec<(&str, Option<Vec<usize>>, i32)> {
        let query = self.branch_query.trim();
        let mut matches: Vec<(&str, Option<Vec<usize>>, i32)> = Vec::new();

        if query.is_empty() {
            for branch in &self.branches {
                matches.push((branch.as_str(), None, 0));
            }
            return matches;
        }

        for branch in &self.branches {
            if let Some((indices, score)) = fuzzy_match(branch.as_str(), query) {
                matches.push((branch.as_str(), Some(indices), score));
            }
        }
        matches.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(b.0)));
        matches
    }

    fn branch_picker_visible_len(&self) -> usize {
        self.build_branch_picker_rows().len()
    }

    fn commit_branch_picker_selection(&mut self) {
        let Some(selected_idx) = self.selection_state.selected_idx else {
            return;
        };
        let branches = self.filtered_branch_matches();
        let Some((branch, _, _)) = branches.get(selected_idx) else {
            return;
        };
        self.draft.branch = (*branch).to_string();
        self.enter_step(Step::Path);
    }

    fn build_shared_dirs_rows(&self) -> Vec<GenericDisplayRow> {
        let mut rows = Vec::new();
        let selected_idx = self.selection_state.selected_idx;

        for (idx, item) in self.draft.shared_dirs.iter().enumerate() {
            let prefix = if selected_idx == Some(idx) {
                '›'
            } else {
                ' '
            };
            let marker = if item.selected { 'x' } else { ' ' };
            let mut name = format!("{prefix} [{marker}] {}", item.dir);
            if item.is_new {
                name.push_str(" (new)");
            }
            rows.push(GenericDisplayRow {
                name,
                description: Some(String::from("will link into the new worktree")),
                ..Default::default()
            });
        }

        let add_idx = self.draft.shared_dirs.len();
        let prefix = if selected_idx == Some(add_idx) {
            '›'
        } else {
            ' '
        };
        rows.push(GenericDisplayRow {
            name: format!("{prefix} [+] Add shared dir…"),
            description: Some(String::from("adds to `worktrees.shared_dirs` on success")),
            ..Default::default()
        });
        rows
    }

    fn accept_default_value_for_step(&mut self) -> bool {
        match self.step {
            Step::Path => {
                if !self.textarea.text().trim().is_empty() {
                    return false;
                }
                let default = self.default_worktree_path();
                let text = default.display().to_string();
                self.textarea.set_text(&text);
                true
            }
            Step::BranchName => {
                if !self.textarea.text().trim().is_empty() {
                    return false;
                }
                let default = self
                    .current_branch
                    .clone()
                    .unwrap_or_else(|| String::from("main"));
                self.textarea.set_text(&default);
                true
            }
            _ => false,
        }
    }

    fn apply_init(&mut self) {
        let name = self.draft.name.trim().to_string();
        if name.is_empty() {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(String::from(
                    "`/worktree init` — worktree name is empty",
                )),
            )));
            self.enter_step(Step::Name);
            return;
        }

        let branch = self.draft.branch.trim().to_string();
        if branch.is_empty() {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(String::from(
                    "`/worktree init` — branch name is empty",
                )),
            )));
            self.enter_step(Step::BranchName);
            return;
        }

        let create_branch = self.draft.branch_mode == BranchMode::CreateNew;
        let base_ref = self.draft.base_ref.trim().to_string();
        if create_branch && base_ref.is_empty() {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(String::from("`/worktree init` — base ref is empty")),
            )));
            self.enter_step(Step::BaseRef);
            return;
        }

        let worktree_path = self.resolve_worktree_path();
        if std::fs::symlink_metadata(&worktree_path).is_ok() {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(format!(
                    "`/worktree init` — worktree path already exists: {}",
                    worktree_path.display()
                )),
            )));
            self.enter_step(Step::Path);
            return;
        }

        let selected_shared_dirs: Vec<String> = self
            .draft
            .shared_dirs
            .iter()
            .filter(|d| d.selected)
            .map(|d| d.dir.clone())
            .collect();
        let next_shared_dirs: Vec<String> = self
            .draft
            .shared_dirs
            .iter()
            .map(|d| d.dir.clone())
            .collect();
        let added_any_shared_dirs = self.draft.shared_dirs.iter().any(|d| d.is_new);

        let workspace_root = self.workspace_root.clone();
        let app_event_tx = self.app_event_tx.clone();
        let invoked_from = self.invoked_from;

        tokio::spawn(async move {
            let result = codex_core::git_info::init_git_worktree_with_mode(
                &workspace_root,
                &name,
                &branch,
                Some(&worktree_path),
                create_branch,
                create_branch.then_some(base_ref.as_str()),
            )
            .await;

            let path = match result {
                Ok(path) => path,
                Err(err) => {
                    let mut lines: Vec<Line<'static>> = Vec::new();
                    lines.push(Line::from(vec![invoked_from.magenta()]));
                    lines.push(Line::from(format!("error: {err}")));
                    lines.push(Line::from(""));
                    lines.push(Line::from("Try running this outside xcodex:"));
                    if create_branch {
                        lines.push(Line::from(format!(
                            "  git -C {} worktree add -b {} {} {}",
                            workspace_root.display(),
                            branch,
                            worktree_path.display(),
                            base_ref
                        )));
                    } else {
                        lines.push(Line::from(format!(
                            "  git -C {} worktree add {} {}",
                            workspace_root.display(),
                            worktree_path.display(),
                            branch
                        )));
                    }
                    app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                        PlainHistoryCell::new(lines),
                    )));
                    return;
                }
            };

            if added_any_shared_dirs {
                app_event_tx.send(AppEvent::UpdateWorktreesSharedDirs {
                    shared_dirs: next_shared_dirs.clone(),
                });
                app_event_tx.send(AppEvent::PersistWorktreesSharedDirs {
                    shared_dirs: next_shared_dirs,
                });
            }

            let mut body: Vec<Line<'static>> = Vec::new();
            body.push(Line::from(vec!["worktree init".into()]));
            body.push(Line::from(vec![
                "created: ".dim(),
                path.display().to_string().into(),
            ]));
            body.push(Line::from(vec![
                "branch: ".dim(),
                branch.clone().into(),
                if create_branch {
                    " (new)".dim()
                } else {
                    "".into()
                },
            ]));
            if create_branch {
                body.push(Line::from(vec!["base: ".dim(), base_ref.clone().into()]));
            }
            body.push(Line::from(vec![
                "workspace root: ".dim(),
                workspace_root.display().to_string().into(),
            ]));

            if !selected_shared_dirs.is_empty() {
                let actions = codex_core::git_info::link_worktree_shared_dirs(
                    &path,
                    &workspace_root,
                    &selected_shared_dirs,
                )
                .await;

                let mut linked = 0usize;
                let mut skipped = 0usize;
                let mut failed = 0usize;
                for action in &actions {
                    match action.outcome {
                        codex_core::git_info::SharedDirLinkOutcome::Linked
                        | codex_core::git_info::SharedDirLinkOutcome::AlreadyLinked => {
                            linked += 1;
                        }
                        codex_core::git_info::SharedDirLinkOutcome::Skipped(_) => skipped += 1,
                        codex_core::git_info::SharedDirLinkOutcome::Failed(_) => failed += 1,
                    }
                }

                body.push(Line::from(""));
                body.push(Line::from(vec![
                    "shared dirs: ".dim(),
                    format!("linked={linked}, skipped={skipped}, failed={failed}").into(),
                ]));
            }

            let command = PlainHistoryCell::new(vec![Line::from(vec![invoked_from.magenta()])]);
            let output = PlainHistoryCell::new(body);
            app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                CompositeHistoryCell::new(vec![Box::new(command), Box::new(output)]),
            )));

            app_event_tx.send(AppEvent::WorktreeSwitched(path.clone()));
            app_event_tx.send(AppEvent::CodexOp(
                codex_core::protocol::Op::OverrideTurnContext {
                    cwd: Some(path.clone()),
                    approval_policy: None,
                    sandbox_policy: None,
                    model: None,
                    effort: None,
                    summary: None,
                },
            ));
            app_event_tx.send(AppEvent::CodexOp(codex_core::protocol::Op::ListSkills {
                cwds: vec![path],
                force_reload: true,
            }));
        });

        self.complete = true;
    }
}

impl BottomPaneView for WorktreeInitWizardView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match self.step {
            Step::Name | Step::BranchName | Step::BaseRef | Step::Path | Step::AddSharedDir => {
                match key_event {
                    KeyEvent {
                        code: KeyCode::Esc, ..
                    } => self.go_back(),
                    KeyEvent {
                        code: KeyCode::Tab, ..
                    } => if self.accept_default_value_for_step() {},
                    KeyEvent {
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => match self.step {
                        Step::Name => {
                            self.draft.name = self.textarea.text().trim().to_string();
                            self.enter_step(Step::BranchMode);
                        }
                        Step::BranchName => {
                            self.draft.branch = self.textarea.text().trim().to_string();
                            if self.draft.branch_mode == BranchMode::CreateNew {
                                self.enter_step(Step::BaseRef);
                            } else {
                                self.enter_step(Step::Path);
                            }
                        }
                        Step::BaseRef => {
                            self.draft.base_ref = self.textarea.text().trim().to_string();
                            self.enter_step(Step::Path);
                        }
                        Step::Path => {
                            self.draft.path = self.textarea.text().trim().to_string();
                            self.enter_step(Step::SharedDirs);
                        }
                        Step::AddSharedDir => self.commit_add_shared_dir(),
                        _ => {}
                    },
                    other => self.textarea.input(other),
                }
            }
            Step::BranchMode => match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.go_back(),
                KeyEvent {
                    code: KeyCode::Up, ..
                } => self.move_up(2),
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                } => self.move_down(2),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => match self.selection_state.selected_idx {
                    Some(1) => {
                        self.draft.branch_mode = BranchMode::CreateNew;
                        let suggested = self.draft.name.trim();
                        if !suggested.is_empty() {
                            self.draft.branch = suggested.to_string();
                        }
                        self.enter_step(Step::BranchName);
                    }
                    _ => {
                        self.draft.branch_mode = BranchMode::Existing;
                        self.draft.branch = self
                            .current_branch
                            .clone()
                            .unwrap_or_else(|| String::from("main"));
                        self.enter_step(Step::BranchPicker);
                    }
                },
                _ => {}
            },
            Step::BranchPicker => match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.go_back(),
                KeyEvent {
                    code: KeyCode::Up, ..
                } => self.move_up(self.branch_picker_visible_len()),
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                } => self.move_down(self.branch_picker_visible_len()),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Tab, ..
                } => self.commit_branch_picker_selection(),
                KeyEvent {
                    code: KeyCode::Backspace,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => {
                    self.branch_query.pop();
                    let len = self.branch_picker_visible_len();
                    self.selection_state.clamp_selection(len);
                    self.selection_state
                        .ensure_visible(len, MAX_POPUP_ROWS.min(len));
                }
                KeyEvent {
                    code: KeyCode::Char(ch),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => {
                    self.branch_query.push(ch);
                    let len = self.branch_picker_visible_len();
                    self.selection_state.clamp_selection(len);
                    self.selection_state
                        .ensure_visible(len, MAX_POPUP_ROWS.min(len));
                }
                _ => {}
            },
            Step::SharedDirs => {
                let len = self.shared_dirs_visible_len();
                match key_event {
                    KeyEvent {
                        code: KeyCode::Esc, ..
                    } => self.go_back(),
                    KeyEvent {
                        code: KeyCode::Up, ..
                    } => self.move_up(len),
                    KeyEvent {
                        code: KeyCode::Down,
                        ..
                    } => self.move_down(len),
                    KeyEvent {
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => self.toggle_shared_dir_selection(),
                    KeyEvent {
                        code: KeyCode::Char('a'),
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => self.enter_step(Step::AddSharedDir),
                    KeyEvent {
                        code: KeyCode::Tab, ..
                    }
                    | KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => {
                        self.enter_step(Step::Confirm);
                    }
                    _ => {}
                }
            }
            Step::Confirm => match key_event {
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.go_back(),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.apply_init(),
                _ => {}
            },
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
        match self.step {
            Step::Name | Step::BranchName | Step::BaseRef | Step::Path | Step::AddSharedDir => {
                if pasted.is_empty() {
                    return false;
                }
                self.textarea.insert_str(&pasted);
                true
            }
            _ => false,
        }
    }
}

impl Renderable for WorktreeInitWizardView {
    fn desired_height(&self, width: u16) -> u16 {
        let content_width = width.saturating_sub(4);
        let header_height = self.header().desired_height(content_width);
        let body_height = match self.step {
            Step::Name | Step::BranchName | Step::BaseRef | Step::Path | Step::AddSharedDir => 4,
            Step::BranchMode => {
                let rows = self.build_branch_mode_rows();
                measure_rows_height(&rows, &self.selection_state, MAX_POPUP_ROWS, content_width)
                    .saturating_add(2)
            }
            Step::BranchPicker => {
                let rows = self.build_branch_picker_rows();
                // Label + search line + list + padding.
                measure_rows_height(&rows, &self.selection_state, MAX_POPUP_ROWS, content_width)
                    .saturating_add(4)
            }
            Step::SharedDirs => {
                let rows = self.build_shared_dirs_rows();
                measure_rows_height(&rows, &self.selection_state, MAX_POPUP_ROWS, content_width)
                    .saturating_add(2)
            }
            Step::Confirm => 10,
        };
        header_height
            .saturating_add(body_height)
            .saturating_add(2)
            .min(24)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let [content_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        Block::default()
            .style(user_message_style())
            .render(content_area, buf);

        let inner = content_area.inset(Insets::vh(1, 2));
        let header = self.header();
        let header_height = header.desired_height(inner.width);
        let [header_area, body_area] =
            Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)]).areas(inner);
        header.render(header_area, buf);

        match self.step {
            Step::Name => {
                self.render_input_step(
                    body_area,
                    buf,
                    "Worktree name",
                    String::from("e.g. fix/worktree"),
                );
            }
            Step::BranchPicker => {
                self.render_branch_picker(body_area, buf);
            }
            Step::BranchName => {
                self.render_input_step(body_area, buf, "Branch name", String::from("e.g. main"));
            }
            Step::BaseRef => {
                self.render_input_step(
                    body_area,
                    buf,
                    "Base ref (for new branch)",
                    String::from("HEAD"),
                );
            }
            Step::Path => {
                let default = self.default_worktree_path();
                self.render_input_step(
                    body_area,
                    buf,
                    "Worktree path (optional)",
                    default.display().to_string(),
                );
            }
            Step::AddSharedDir => {
                self.render_input_step(
                    body_area,
                    buf,
                    "Add shared dir (repo-relative)",
                    String::from("e.g. docs/impl-plans"),
                );
            }
            Step::BranchMode => {
                let rows = self.build_branch_mode_rows();
                self.render_rows(body_area, buf, &rows, "  No options");
            }
            Step::SharedDirs => {
                let rows = self.build_shared_dirs_rows();
                self.render_rows(body_area, buf, &rows, "  No shared dirs configured");
            }
            Step::Confirm => {
                self.render_confirm(body_area, buf);
            }
        }

        let hint = match self.step {
            Step::Path => Line::from(vec![
                "Enter".cyan(),
                " = Next, ".dim(),
                "Tab".cyan(),
                " = Use default, ".dim(),
                "Esc".cyan(),
                " = Back".dim(),
            ]),
            Step::BranchPicker => Line::from(vec![
                "Enter".cyan(),
                " = Select, ".dim(),
                "Tab".cyan(),
                " = Select, ".dim(),
                "Esc".cyan(),
                " = Back".dim(),
            ]),
            Step::SharedDirs => Line::from(vec![
                "Enter".cyan(),
                " = Toggle, ".dim(),
                "a".cyan(),
                " = Add, ".dim(),
                "Tab".cyan(),
                " = Next, ".dim(),
                "Esc".cyan(),
                " = Back".dim(),
            ]),
            Step::Confirm => Line::from(vec![
                "Enter".cyan(),
                " = Create + switch, ".dim(),
                "Esc".cyan(),
                " = Back, ".dim(),
                "Ctrl+C".cyan(),
                " = Cancel".dim(),
            ]),
            _ => standard_popup_hint_line(),
        };

        let hint_area = Rect {
            x: footer_area.x + 2,
            y: footer_area.y,
            width: footer_area.width.saturating_sub(2),
            height: 1,
        };
        Paragraph::new(hint).render(hint_area, buf);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        match self.step {
            Step::Name | Step::BranchName | Step::BaseRef | Step::Path | Step::AddSharedDir => {
                if area.height == 0 || area.width == 0 {
                    return None;
                }
                let content_area = Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: area.height.saturating_sub(1),
                };
                let inner = content_area.inset(Insets::vh(1, 2));
                let header_height = self.header().desired_height(inner.width);
                let [_, body_area] =
                    Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)])
                        .areas(inner);
                self.input_cursor_pos(body_area)
            }
            _ => None,
        }
    }
}

impl WorktreeInitWizardView {
    fn render_rows(&self, area: Rect, buf: &mut Buffer, rows: &[GenericDisplayRow], empty: &str) {
        let rows_width = area.width.saturating_sub(2).max(1);
        let height = measure_rows_height(rows, &self.selection_state, MAX_POPUP_ROWS, rows_width);
        let render_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: height.min(area.height),
        };
        render_rows(
            render_area,
            buf,
            rows,
            &self.selection_state,
            MAX_POPUP_ROWS,
            empty,
        );
    }

    fn render_input_step(&self, area: Rect, buf: &mut Buffer, label: &str, placeholder: String) {
        let [label_area, input_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

        Paragraph::new(Line::from(vec![label.bold()])).render(label_area, buf);

        if input_area.width == 0 || input_area.height == 0 {
            return;
        }
        Clear.render(input_area, buf);

        let text_area = Rect {
            x: input_area.x,
            y: input_area.y,
            width: input_area.width,
            height: 1,
        };
        let mut state = self.textarea_state.borrow_mut();
        StatefulWidgetRef::render_ref(&(&self.textarea), text_area, buf, &mut state);
        if self.textarea.text().is_empty() {
            Paragraph::new(Line::from(placeholder.dim())).render(text_area, buf);
        }
    }

    fn render_branch_picker(&self, area: Rect, buf: &mut Buffer) {
        let [label_area, search_area, list_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);

        Paragraph::new(Line::from(vec!["Branch".bold(), " (existing)".dim()]))
            .render(label_area, buf);

        let query = self.branch_query.clone();
        let query_span: Span<'static> = if query.is_empty() {
            "Type to search branches".dim()
        } else {
            query.into()
        };
        Paragraph::new(Line::from(vec!["Search: ".dim(), query_span])).render(search_area, buf);

        let rows = self.build_branch_picker_rows();
        self.render_rows(list_area, buf, &rows, "  No matching branches");
    }

    fn input_cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width == 0 || area.height < 2 {
            return None;
        }
        let input_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: 1,
        };
        let state = *self.textarea_state.borrow();
        self.textarea.cursor_pos_with_state(input_area, state)
    }

    fn render_confirm(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line<'static>> = Vec::new();

        let name = self.draft.name.trim();
        let branch = self.draft.branch.trim();
        let create_branch = self.draft.branch_mode == BranchMode::CreateNew;
        let base_ref = self.draft.base_ref.trim();
        let path = self.resolve_worktree_path();

        lines.push(Line::from(vec![
            "Name: ".dim(),
            if name.is_empty() {
                "(missing)".red()
            } else {
                name.to_string().into()
            },
        ]));
        lines.push(Line::from(vec![
            "Branch: ".dim(),
            if branch.is_empty() {
                "(missing)".red()
            } else {
                branch.to_string().into()
            },
            if create_branch {
                " (new)".dim()
            } else {
                "".into()
            },
        ]));
        if create_branch {
            lines.push(Line::from(vec![
                "Base: ".dim(),
                if base_ref.is_empty() {
                    "(missing)".red()
                } else {
                    base_ref.to_string().into()
                },
            ]));
        }
        lines.push(Line::from(vec![
            "Worktree path: ".dim(),
            path.display().to_string().into(),
        ]));
        lines.push(Line::from(""));

        let selected: Vec<&str> = self
            .draft
            .shared_dirs
            .iter()
            .filter(|d| d.selected)
            .map(|d| d.dir.as_str())
            .collect();
        lines.push(Line::from(vec![
            "Shared dirs: ".dim(),
            if selected.is_empty() {
                "(none)".dim()
            } else {
                selected.join(", ").into()
            },
        ]));

        Paragraph::new(lines).render(area, buf);
    }
}
