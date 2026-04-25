use codex_feedback::FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME;
use codex_feedback::FeedbackDiagnostics;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use crate::app_event::AppEvent;
use crate::app_event::FeedbackCategory;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;
use crate::render::renderable::Renderable;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::popup_consts::standard_popup_hint_line;
use super::textarea::TextArea;
use super::textarea::TextAreaState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FeedbackAudience {
    OpenAiEmployee,
    External,
}

/// Minimal input overlay to collect an optional feedback note, then save
/// a local report for troubleshooting.
pub(crate) struct FeedbackNoteView {
    category: FeedbackCategory,
    snapshot: codex_feedback::FeedbackSnapshot,
    rollout_path: Option<PathBuf>,
    app_event_tx: AppEventSender,
    include_logs: bool,

    // UI state
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    complete: bool,
}

impl FeedbackNoteView {
    pub(crate) fn new(
        category: FeedbackCategory,
        snapshot: codex_feedback::FeedbackSnapshot,
        rollout_path: Option<PathBuf>,
        app_event_tx: AppEventSender,
        include_logs: bool,
    ) -> Self {
        Self {
            category,
            snapshot,
            rollout_path,
            app_event_tx,
            include_logs,
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            complete: false,
        }
    }

    fn submit(&mut self) {
        let note = self.textarea.text().trim().to_string();
        let classification = feedback_classification(self.category);
        let thread_id = self.snapshot.thread_id.clone();

        let note_path = if note.is_empty() {
            None
        } else {
            match save_feedback_note(&thread_id, classification, &note) {
                Ok(path) => Some(path),
                Err(err) => {
                    self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_error_event(format!(
                            "Failed to save feedback note: {err}"
                        )),
                    )));
                    self.complete = true;
                    return;
                }
            }
        };

        let log_path = if self.include_logs {
            match self.snapshot.save_to_temp_file() {
                Ok(path) => Some(path),
                Err(err) => {
                    self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_error_event(format!("Failed to save logs: {err}")),
                    )));
                    self.complete = true;
                    return;
                }
            }
        } else {
            None
        };

        let diagnostics_path = if self.include_logs {
            match self.snapshot.feedback_diagnostics_attachment_text(true) {
                Some(contents) => match save_feedback_diagnostics(&thread_id, &contents) {
                    Ok(path) => Some(path),
                    Err(err) => {
                        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                            history_cell::new_error_event(format!(
                                "Failed to save diagnostics: {err}"
                            )),
                        )));
                        self.complete = true;
                        return;
                    }
                },
                None => None,
            }
        } else {
            None
        };

        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
            local_feedback_saved_cell(
                self.include_logs,
                &thread_id,
                note_path.as_deref(),
                log_path.as_deref(),
                diagnostics_path.as_deref(),
                self.rollout_path.as_deref(),
            ),
        )));
        self.complete = true;
    }
}

impl BottomPaneView for FeedbackNoteView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.on_ctrl_c();
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                self.submit();
            }
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => {
                self.textarea.input(key_event);
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

impl Renderable for FeedbackNoteView {
    fn desired_height(&self, width: u16) -> u16 {
        self.intro_lines(width).len() as u16 + self.input_height(width) + 2u16
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.height < 2 || area.width <= 2 {
            return None;
        }
        let intro_height = self.intro_lines(area.width).len() as u16;
        let text_area_height = self.input_height(area.width).saturating_sub(1);
        if text_area_height == 0 {
            return None;
        }
        let textarea_rect = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(intro_height).saturating_add(1),
            width: area.width.saturating_sub(2),
            height: text_area_height,
        };
        let state = *self.textarea_state.borrow();
        self.textarea.cursor_pos_with_state(textarea_rect, state)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let intro_lines = self.intro_lines(area.width);
        let (_, placeholder) = feedback_title_and_placeholder(self.category);
        let input_height = self.input_height(area.width);

        for (offset, line) in intro_lines.iter().enumerate() {
            Paragraph::new(line.clone()).render(
                Rect {
                    x: area.x,
                    y: area.y.saturating_add(offset as u16),
                    width: area.width,
                    height: 1,
                },
                buf,
            );
        }

        // Input line
        let input_area = Rect {
            x: area.x,
            y: area.y.saturating_add(intro_lines.len() as u16),
            width: area.width,
            height: input_height,
        };
        if input_area.width >= 2 {
            for row in 0..input_area.height {
                Paragraph::new(Line::from(vec![gutter()])).render(
                    Rect {
                        x: input_area.x,
                        y: input_area.y.saturating_add(row),
                        width: 2,
                        height: 1,
                    },
                    buf,
                );
            }

            let text_area_height = input_area.height.saturating_sub(1);
            if text_area_height > 0 {
                if input_area.width > 2 {
                    let blank_rect = Rect {
                        x: input_area.x.saturating_add(2),
                        y: input_area.y,
                        width: input_area.width.saturating_sub(2),
                        height: 1,
                    };
                    Clear.render(blank_rect, buf);
                }
                let textarea_rect = Rect {
                    x: input_area.x.saturating_add(2),
                    y: input_area.y.saturating_add(1),
                    width: input_area.width.saturating_sub(2),
                    height: text_area_height,
                };
                let mut state = self.textarea_state.borrow_mut();
                StatefulWidgetRef::render_ref(&(&self.textarea), textarea_rect, buf, &mut state);
                if self.textarea.text().is_empty() {
                    Paragraph::new(Line::from(placeholder.dim())).render(textarea_rect, buf);
                }
            }
        }

        let hint_blank_y = input_area.y.saturating_add(input_height);
        if hint_blank_y < area.y.saturating_add(area.height) {
            let blank_area = Rect {
                x: area.x,
                y: hint_blank_y,
                width: area.width,
                height: 1,
            };
            Clear.render(blank_area, buf);
        }

        let hint_y = hint_blank_y.saturating_add(1);
        if hint_y < area.y.saturating_add(area.height) {
            Paragraph::new(standard_popup_hint_line()).render(
                Rect {
                    x: area.x,
                    y: hint_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
        }
    }
}

impl FeedbackNoteView {
    fn input_height(&self, width: u16) -> u16 {
        let usable_width = width.saturating_sub(2);
        let text_height = self.textarea.desired_height(usable_width).clamp(1, 8);
        text_height.saturating_add(1).min(9)
    }

    fn intro_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let (title, _) = feedback_title_and_placeholder(self.category);
        vec![Line::from(vec![gutter(), title.bold()])]
    }
}

pub(crate) fn should_show_feedback_connectivity_details(
    category: FeedbackCategory,
    diagnostics: &FeedbackDiagnostics,
) -> bool {
    category != FeedbackCategory::GoodResult && !diagnostics.is_empty()
}

fn gutter() -> Span<'static> {
    "▌ ".cyan()
}

fn feedback_title_and_placeholder(category: FeedbackCategory) -> (String, String) {
    match category {
        FeedbackCategory::BadResult => (
            "Tell us more (bad result)".to_string(),
            "(optional) Write a short description to help us further".to_string(),
        ),
        FeedbackCategory::GoodResult => (
            "Tell us more (good result)".to_string(),
            "(optional) Write a short description to help us further".to_string(),
        ),
        FeedbackCategory::Bug => (
            "Tell us more (bug)".to_string(),
            "(optional) Write a short description to help us further".to_string(),
        ),
        FeedbackCategory::SafetyCheck => (
            "Tell us more (safety check)".to_string(),
            "(optional) Share what was refused and why it should have been allowed".to_string(),
        ),
        FeedbackCategory::Other => (
            "Tell us more (other)".to_string(),
            "(optional) Write a short description to help us further".to_string(),
        ),
    }
}

pub(crate) fn feedback_classification(category: FeedbackCategory) -> &'static str {
    match category {
        FeedbackCategory::BadResult => "bad_result",
        FeedbackCategory::GoodResult => "good_result",
        FeedbackCategory::Bug => "bug",
        FeedbackCategory::SafetyCheck => "safety_check",
        FeedbackCategory::Other => "other",
    }
}

fn save_feedback_note(
    thread_id: &str,
    classification: &str,
    note: &str,
) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("xcodex-feedback-{thread_id}.txt"));
    let contents = format!("thread_id={thread_id}\nclassification={classification}\n\n{note}\n");
    std::fs::write(&path, contents)?;
    Ok(path)
}

fn save_feedback_diagnostics(thread_id: &str, contents: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "xcodex-feedback-{thread_id}-{FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME}"
    ));
    std::fs::write(&path, contents)?;
    Ok(path)
}

fn local_feedback_saved_cell(
    include_logs: bool,
    thread_id: &str,
    note_path: Option<&Path>,
    log_path: Option<&Path>,
    diagnostics_path: Option<&Path>,
    rollout_path: Option<&Path>,
) -> history_cell::PlainHistoryCell {
    let mut lines = vec![Line::from(if include_logs {
        "• Feedback saved locally (no network upload)."
    } else {
        "• Feedback saved locally (no network upload, no logs)."
    })];

    lines.extend([
        "".into(),
        Line::from(vec!["  Thread ID: ".into(), thread_id.to_string().bold()]),
    ]);

    if let Some(path) = note_path {
        lines.push(Line::from(vec![
            "  Note: ".into(),
            path.display().to_string().cyan().underlined(),
        ]));
    }
    if let Some(path) = log_path {
        lines.push(Line::from(vec![
            "  Logs: ".into(),
            path.display().to_string().cyan().underlined(),
        ]));
    }
    if let Some(path) = diagnostics_path {
        lines.push(Line::from(vec![
            "  Diagnostics: ".into(),
            path.display().to_string().cyan().underlined(),
        ]));
    }
    if let Some(path) = rollout_path {
        lines.push(Line::from(vec![
            "  Rollout: ".into(),
            path.display().to_string().cyan().underlined(),
        ]));
    }

    lines.extend([
        "".into(),
        Line::from("  Attach these files when filing an issue in your fork.".dim()),
    ]);

    history_cell::PlainHistoryCell::new(lines)
}

pub(crate) fn feedback_success_cell(
    _category: FeedbackCategory,
    include_logs: bool,
    thread_id: &str,
    _feedback_audience: FeedbackAudience,
) -> history_cell::PlainHistoryCell {
    let mut lines = vec![Line::from(if include_logs {
        "• Feedback uploaded."
    } else {
        "• Feedback recorded (no logs)."
    })];
    lines.extend([
        "".into(),
        Line::from(vec!["  Thread ID: ".into(), thread_id.to_string().bold()]),
    ]);
    history_cell::PlainHistoryCell::new(lines)
}

// Build the selection popup params for feedback categories.
pub(crate) fn feedback_selection_params(
    app_event_tx: AppEventSender,
) -> super::SelectionViewParams {
    super::SelectionViewParams {
        title: Some("How was this?".to_string()),
        items: vec![
            make_feedback_item(
                app_event_tx.clone(),
                "bug",
                "Crash, error message, hang, or broken UI/behavior.",
                FeedbackCategory::Bug,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "bad result",
                "Output was off-target, incorrect, incomplete, or unhelpful.",
                FeedbackCategory::BadResult,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "good result",
                "Helpful, correct, high‑quality, or delightful result worth celebrating.",
                FeedbackCategory::GoodResult,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "safety check",
                "Benign usage blocked due to safety checks or refusals.",
                FeedbackCategory::SafetyCheck,
            ),
            make_feedback_item(
                app_event_tx,
                "other",
                "Slowness, feature suggestion, UX feedback, or anything else.",
                FeedbackCategory::Other,
            ),
        ],
        ..Default::default()
    }
}

/// Build the selection popup params shown when feedback is disabled.
pub(crate) fn feedback_disabled_params() -> super::SelectionViewParams {
    super::SelectionViewParams {
        title: Some("Sending feedback is disabled".to_string()),
        subtitle: Some("This action is disabled by configuration.".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items: vec![super::SelectionItem {
            name: "Close".to_string(),
            dismiss_on_select: true,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn make_feedback_item(
    app_event_tx: AppEventSender,
    name: &str,
    description: &str,
    category: FeedbackCategory,
) -> super::SelectionItem {
    let action: super::SelectionAction = Box::new(move |_sender: &AppEventSender| {
        app_event_tx.send(AppEvent::OpenFeedbackConsent { category });
    });
    super::SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        actions: vec![action],
        dismiss_on_select: true,
        ..Default::default()
    }
}

/// Build the upload consent popup params for a given feedback category.
pub(crate) fn feedback_upload_consent_params(
    app_event_tx: AppEventSender,
    category: FeedbackCategory,
    rollout_path: Option<std::path::PathBuf>,
    feedback_diagnostics: &FeedbackDiagnostics,
) -> super::SelectionViewParams {
    use super::popup_consts::standard_popup_hint_line;
    let yes_action: super::SelectionAction = Box::new({
        let tx = app_event_tx.clone();
        move |sender: &AppEventSender| {
            let _ = sender;
            tx.send(AppEvent::OpenFeedbackNote {
                category,
                include_logs: true,
            });
        }
    });

    let no_action: super::SelectionAction = Box::new({
        let tx = app_event_tx;
        move |sender: &AppEventSender| {
            let _ = sender;
            tx.send(AppEvent::OpenFeedbackNote {
                category,
                include_logs: false,
            });
        }
    });

    // Build header listing files that would be saved if user consents.
    let mut header_lines: Vec<Box<dyn crate::render::renderable::Renderable>> = vec![
        Line::from("Save logs?".bold()).into(),
        Line::from("").into(),
        Line::from("The following files will be saved locally:".dim()).into(),
        Line::from(vec![
            "  • ".into(),
            "xcodex-feedback-<thread-id>.log".into(),
        ])
        .into(),
    ];
    if let Some(path) = rollout_path.as_deref()
        && let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_string())
    {
        header_lines.push(Line::from(vec!["  • ".into(), name.into()]).into());
    }
    if !feedback_diagnostics.is_empty() {
        header_lines.push(
            Line::from(vec![
                "  • ".into(),
                FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME.into(),
            ])
            .into(),
        );
    }
    if should_show_feedback_connectivity_details(category, feedback_diagnostics) {
        header_lines.push(Line::from("").into());
        header_lines.push(Line::from("Connectivity diagnostics".bold()).into());
        for diagnostic in feedback_diagnostics.diagnostics() {
            header_lines
                .push(Line::from(vec!["  - ".into(), diagnostic.headline.clone().into()]).into());
            for detail in &diagnostic.details {
                header_lines.push(Line::from(vec!["    - ".dim(), detail.clone().into()]).into());
            }
        }
    }

    super::SelectionViewParams {
        footer_hint: Some(standard_popup_hint_line()),
        items: vec![
            super::SelectionItem {
                name: "Yes".to_string(),
                description: Some(
                    "Save the current xcodex session logs for troubleshooting.".to_string(),
                ),
                actions: vec![yes_action],
                dismiss_on_select: true,
                ..Default::default()
            },
            super::SelectionItem {
                name: "No".to_string(),
                actions: vec![no_action],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        header: Box::new(crate::render::renderable::ColumnRenderable::with(
            header_lines,
        )),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use codex_feedback::CodexFeedback;
    use codex_feedback::FeedbackDiagnostic;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;

    fn render(view: &FeedbackNoteView, width: u16) -> String {
        let height = view.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);

        let mut lines: Vec<String> = (0..area.height)
            .map(|row| {
                let mut line = String::new();
                for col in 0..area.width {
                    let symbol = buf[(area.x + col, area.y + row)].symbol();
                    if symbol.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(symbol);
                    }
                }
                line.trim_end().to_string()
            })
            .collect();

        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    fn render_cell(cell: &impl history_cell::HistoryCell, width: u16) -> String {
        cell.display_lines(width)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn make_snapshot() -> codex_feedback::FeedbackSnapshot {
        CodexFeedback::new().snapshot(Some(ThreadId::new()))
    }

    fn make_view(category: FeedbackCategory) -> FeedbackNoteView {
        let (tx_raw, _rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        FeedbackNoteView::new(
            category,
            make_snapshot(),
            /*rollout_path*/ None,
            tx,
            /*include_logs*/ true,
        )
    }

    #[test]
    fn feedback_view_bad_result() {
        let view = make_view(FeedbackCategory::BadResult);
        let rendered = render(&view, /*width*/ 60);
        insta::assert_snapshot!("feedback_view_bad_result", rendered);
    }

    #[test]
    fn feedback_view_good_result() {
        let view = make_view(FeedbackCategory::GoodResult);
        let rendered = render(&view, /*width*/ 60);
        insta::assert_snapshot!("feedback_view_good_result", rendered);
    }

    #[test]
    fn feedback_view_bug() {
        let view = make_view(FeedbackCategory::Bug);
        let rendered = render(&view, /*width*/ 60);
        insta::assert_snapshot!("feedback_view_bug", rendered);
    }

    #[test]
    fn feedback_view_other() {
        let view = make_view(FeedbackCategory::Other);
        let rendered = render(&view, /*width*/ 60);
        insta::assert_snapshot!("feedback_view_other", rendered);
    }

    #[test]
    fn save_feedback_note_writes_file() {
        let path = save_feedback_note("thread-1", "bug", "hello").expect("save note");
        let contents = std::fs::read_to_string(&path).expect("read note");
        assert!(contents.contains("thread_id=thread-1"));
        assert!(contents.contains("classification=bug"));
        assert!(contents.contains("hello"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn feedback_view_safety_check() {
        let view = make_view(FeedbackCategory::SafetyCheck);
        let rendered = render(&view, /*width*/ 60);
        insta::assert_snapshot!("feedback_view_safety_check", rendered);
    }

    #[test]
    fn submit_feedback_inserts_history_cell_with_local_artifacts() {
        let (tx_raw, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut view = FeedbackNoteView::new(
            FeedbackCategory::Bug,
            make_snapshot(),
            Some(PathBuf::from("/tmp/rollout.jsonl")),
            tx,
            /*include_logs*/ true,
        );
        view.textarea.insert_str("  something broke  ");

        view.submit();

        let event = rx.try_recv().expect("history event");
        assert!(matches!(event, AppEvent::InsertHistoryCell(_)));
        assert_eq!(view.is_complete(), true);
    }

    #[test]
    fn should_show_feedback_connectivity_details_only_for_non_good_result_with_diagnostics() {
        let diagnostics = FeedbackDiagnostics::new(vec![FeedbackDiagnostic {
            headline: "Proxy environment variables are set and may affect connectivity."
                .to_string(),
            details: vec!["HTTP_PROXY = http://proxy.example.com:8080".to_string()],
        }]);

        assert_eq!(
            should_show_feedback_connectivity_details(FeedbackCategory::Bug, &diagnostics),
            true
        );
        assert_eq!(
            should_show_feedback_connectivity_details(FeedbackCategory::GoodResult, &diagnostics),
            false
        );
        assert_eq!(
            should_show_feedback_connectivity_details(
                FeedbackCategory::BadResult,
                &FeedbackDiagnostics::default()
            ),
            false
        );
    }

    #[test]
    fn local_feedback_saved_cell_matches_copy() {
        let rendered = render_cell(
            &local_feedback_saved_cell(
                /*include_logs*/ true,
                "thread-1",
                Some(Path::new("/tmp/note.txt")),
                Some(Path::new("/tmp/log.txt")),
                Some(Path::new("/tmp/diag.txt")),
                Some(Path::new("/tmp/rollout.jsonl")),
            ),
            /*width*/ 120,
        );
        assert_eq!(
            rendered,
            "• Feedback saved locally (no network upload).\n\n  Thread ID: thread-1\n  Note: /tmp/note.txt\n  Logs: /tmp/log.txt\n  Diagnostics: /tmp/diag.txt\n  Rollout: /tmp/rollout.jsonl\n\n  Attach these files when filing an issue in your fork."
        );
    }

    #[test]
    fn feedback_success_cell_matches_neutral_bug_copy() {
        let rendered = render_cell(
            &feedback_success_cell(
                FeedbackCategory::Bug,
                /*include_logs*/ true,
                "thread-2",
                FeedbackAudience::OpenAiEmployee,
            ),
            /*width*/ 120,
        );
        assert_eq!(rendered, "• Feedback uploaded.\n\n  Thread ID: thread-2");
    }

    #[test]
    fn feedback_success_cell_matches_neutral_good_result_copy() {
        let rendered = render_cell(
            &feedback_success_cell(
                FeedbackCategory::GoodResult,
                /*include_logs*/ false,
                "thread-3",
                FeedbackAudience::External,
            ),
            /*width*/ 120,
        );
        assert_eq!(
            rendered,
            "• Feedback recorded (no logs).\n\n  Thread ID: thread-3"
        );
    }
}
