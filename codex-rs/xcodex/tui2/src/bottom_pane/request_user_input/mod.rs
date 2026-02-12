//! Request-user-input overlay state machine.
//!
//! Core behaviors:
//! - Each question can be answered by selecting a single option and/or providing notes.
//! - Notes are stored per question and appended as extra answers.
//! - Enter advances to the next question; the last question opens an answers-review step.
//! - Freeform-only questions submit an empty answer list when empty.
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;

mod layout;
mod render;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::bottom_pane_view::BottomPaneView;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::bottom_pane::selection_popup_common::GenericDisplayRow;
use crate::bottom_pane::textarea::TextArea;
use crate::bottom_pane::textarea::TextAreaState;

use codex_core::protocol::Op;
use codex_protocol::request_user_input::RequestUserInputAnswer;
use codex_protocol::request_user_input::RequestUserInputEvent;
use codex_protocol::request_user_input::RequestUserInputResponse;

const NOTES_PLACEHOLDER: &str = "Add notes (optional)";
const ANSWER_PLACEHOLDER: &str = "Type your answer (optional)";
const SELECT_OPTION_PLACEHOLDER: &str = "Select an option to add notes (optional)";
pub(super) const UNANSWERED_CONFIRM_TITLE: &str = "Submit with unanswered questions?";
const UNANSWERED_CONFIRM_GO_BACK: &str = "Go back";
const UNANSWERED_CONFIRM_GO_BACK_DESC: &str = "Return to the first unanswered question.";
const UNANSWERED_CONFIRM_SUBMIT: &str = "Proceed";
pub(super) const REVIEW_CONFIRM_TITLE: &str = "Review answers before submit";
const REVIEW_CONFIRM_GO_BACK: &str = "Go back";
const REVIEW_CONFIRM_GO_BACK_DESC: &str = "Return to questions to edit answers.";
const REVIEW_CONFIRM_SUBMIT: &str = "Submit";
const REVIEW_CONFIRM_SUBMIT_DESC: &str = "Send these answers to the model.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Options,
    Notes,
}

struct NotesEntry {
    text: TextArea,
    state: RefCell<TextAreaState>,
}

impl NotesEntry {
    fn new() -> Self {
        Self {
            text: TextArea::new(),
            state: RefCell::new(TextAreaState::default()),
        }
    }
}

struct AnswerState {
    // Scrollable cursor state for option navigation/highlight.
    option_state: ScrollState,
    // Selected option for this question. We keep a Vec for payload compatibility.
    selected_option_indices: Vec<usize>,
    // Per-question notes draft.
    notes: NotesEntry,
    // Whether this answer was explicitly committed (Enter on question).
    answer_committed: bool,
}

pub(crate) struct RequestUserInputOverlay {
    app_event_tx: AppEventSender,
    request: RequestUserInputEvent,
    // Queue of incoming requests to process after the current one.
    queue: VecDeque<RequestUserInputEvent>,
    answers: Vec<AnswerState>,
    current_idx: usize,
    focus: Focus,
    done: bool,
    confirm_unanswered: Option<ScrollState>,
    confirm_review: Option<ScrollState>,
}

impl RequestUserInputOverlay {
    pub(crate) fn new(request: RequestUserInputEvent, app_event_tx: AppEventSender) -> Self {
        let mut overlay = Self {
            app_event_tx,
            request,
            queue: VecDeque::new(),
            answers: Vec::new(),
            current_idx: 0,
            focus: Focus::Options,
            done: false,
            confirm_unanswered: None,
            confirm_review: None,
        };
        overlay.reset_for_request();
        overlay.ensure_focus_available();
        overlay
    }

    fn current_index(&self) -> usize {
        self.current_idx
    }

    fn current_question(
        &self,
    ) -> Option<&codex_protocol::request_user_input::RequestUserInputQuestion> {
        self.request.questions.get(self.current_index())
    }

    fn current_answer_mut(&mut self) -> Option<&mut AnswerState> {
        let idx = self.current_index();
        self.answers.get_mut(idx)
    }

    fn current_answer(&self) -> Option<&AnswerState> {
        let idx = self.current_index();
        self.answers.get(idx)
    }

    fn question_count(&self) -> usize {
        self.request.questions.len()
    }

    fn has_options(&self) -> bool {
        self.current_question()
            .and_then(|question| question.options.as_ref())
            .is_some_and(|options| !options.is_empty())
    }

    fn options_len(&self) -> usize {
        self.current_question()
            .and_then(|question| question.options.as_ref())
            .map(std::vec::Vec::len)
            .unwrap_or(0)
    }

    fn option_index_for_digit(&self, ch: char) -> Option<usize> {
        if !self.has_options() {
            return None;
        }
        let digit = ch.to_digit(10)?;
        if digit == 0 {
            return None;
        }
        let idx = (digit - 1) as usize;
        (idx < self.options_len()).then_some(idx)
    }

    fn selected_option_index(&self) -> Option<usize> {
        if !self.has_options() {
            return None;
        }
        self.current_answer()
            .and_then(|answer| answer.option_state.selected_idx)
    }

    fn has_selected_options(&self) -> bool {
        self.current_answer()
            .is_some_and(|answer| !answer.selected_option_indices.is_empty())
    }

    fn current_notes_entry(&self) -> Option<&NotesEntry> {
        self.current_answer().map(|answer| &answer.notes)
    }

    fn current_notes_entry_mut(&mut self) -> Option<&mut NotesEntry> {
        self.current_answer_mut().map(|answer| &mut answer.notes)
    }

    fn notes_placeholder(&self) -> &'static str {
        if self.has_options() && !self.has_selected_options() {
            SELECT_OPTION_PLACEHOLDER
        } else if self.has_options() {
            NOTES_PLACEHOLDER
        } else {
            ANSWER_PLACEHOLDER
        }
    }

    fn confirm_unanswered_active(&self) -> bool {
        self.confirm_unanswered.is_some()
    }

    fn confirm_review_active(&self) -> bool {
        self.confirm_review.is_some()
    }

    fn focus_is_options(&self) -> bool {
        self.focus == Focus::Options
    }

    fn focus_is_notes(&self) -> bool {
        self.focus == Focus::Notes
    }

    fn focus_is_notes_without_options(&self) -> bool {
        self.focus_is_notes() && !self.has_options()
    }

    /// Ensure the focus mode is valid for the current question.
    fn ensure_focus_available(&mut self) {
        if self.question_count() == 0 {
            return;
        }
        if !self.has_options() {
            self.focus = Focus::Notes;
        }
    }

    /// Rebuild local answer state from the current request.
    fn reset_for_request(&mut self) {
        self.answers = self
            .request
            .questions
            .iter()
            .map(|question| {
                let mut option_state = ScrollState::new();
                if let Some(options) = question.options.as_ref()
                    && !options.is_empty()
                {
                    option_state.selected_idx = Some(0);
                }
                AnswerState {
                    option_state,
                    selected_option_indices: Vec::new(),
                    notes: NotesEntry::new(),
                    answer_committed: false,
                }
            })
            .collect();

        self.current_idx = 0;
        self.focus = Focus::Options;
        self.confirm_unanswered = None;
        self.confirm_review = None;
    }

    fn move_question(&mut self, forward: bool) {
        let next = if forward {
            self.current_idx.saturating_add(1)
        } else {
            self.current_idx.saturating_sub(1)
        };
        if next < self.question_count() {
            self.current_idx = next;
            self.ensure_focus_available();
        }
    }

    fn jump_to_question(&mut self, idx: usize) {
        if idx < self.question_count() {
            self.current_idx = idx;
            self.ensure_focus_available();
        }
    }

    fn toggle_option_selection(&mut self, option_idx: usize) {
        if !self.has_options() {
            return;
        }
        let options_len = self.options_len();
        if option_idx >= options_len {
            return;
        }

        if let Some(answer) = self.current_answer_mut() {
            answer.option_state.clamp_selection(options_len);
            answer.option_state.selected_idx = Some(option_idx);
            if answer.selected_option_indices.contains(&option_idx) {
                answer.selected_option_indices.clear();
            } else {
                answer.selected_option_indices = vec![option_idx];
            }
            answer.answer_committed = false;
        }
    }

    fn ensure_default_option_selected(&mut self) {
        if !self.has_options() || self.has_selected_options() {
            return;
        }
        let selected_idx = self
            .current_answer()
            .and_then(|answer| answer.option_state.selected_idx)
            .unwrap_or(0);
        self.toggle_option_selection(selected_idx);
    }

    fn commit_current_question(&mut self) {
        let has_options = self.has_options();
        if has_options {
            self.ensure_default_option_selected();
        }

        if let Some(answer) = self.current_answer_mut() {
            answer.answer_committed = if has_options {
                !answer.selected_option_indices.is_empty()
            } else {
                !answer.notes.text.text().trim().is_empty()
            };
        }
    }

    fn collect_answers(&self) -> HashMap<String, RequestUserInputAnswer> {
        let mut answers = HashMap::new();
        for (idx, question) in self.request.questions.iter().enumerate() {
            let answer_state = &self.answers[idx];
            let options = question.options.as_ref();
            let selected_indices = if options.is_some_and(|opts| !opts.is_empty())
                && !answer_state.selected_option_indices.is_empty()
            {
                answer_state.selected_option_indices.clone()
            } else {
                Vec::new()
            };
            let notes = if !answer_state.notes.text.text().trim().is_empty() {
                answer_state.notes.text.text().trim().to_string()
            } else {
                String::new()
            };
            let mut answer_list = selected_indices
                .iter()
                .filter_map(|selected_idx| {
                    question
                        .options
                        .as_ref()
                        .and_then(|opts| opts.get(*selected_idx))
                        .map(|opt| opt.label.clone())
                })
                .collect::<Vec<_>>();
            if !notes.is_empty() {
                answer_list.push(format!("user_note: {notes}"));
            }
            answers.insert(
                question.id.clone(),
                RequestUserInputAnswer {
                    answers: answer_list,
                },
            );
        }
        answers
    }

    fn review_answer_summaries(&self) -> Vec<String> {
        let answers = self.collect_answers();
        self.request
            .questions
            .iter()
            .map(|question| {
                let answer_values = answers
                    .get(&question.id)
                    .map(|answer| answer.answers.clone())
                    .unwrap_or_default();
                if answer_values.is_empty() {
                    format!("{}: no answer", question.header)
                } else {
                    format!("{}: {}", question.header, answer_values.join(", "))
                }
            })
            .collect()
    }

    fn open_review_confirmation(&mut self) {
        let mut state = ScrollState::new();
        state.selected_idx = Some(0);
        self.confirm_review = Some(state);
    }

    fn close_review_confirmation(&mut self) {
        self.confirm_review = None;
    }

    fn review_confirmation_rows(&self) -> Vec<GenericDisplayRow> {
        let selected = self
            .confirm_review
            .as_ref()
            .and_then(|state| state.selected_idx)
            .unwrap_or(0);
        let entries = [
            (
                REVIEW_CONFIRM_SUBMIT,
                REVIEW_CONFIRM_SUBMIT_DESC.to_string(),
            ),
            (
                REVIEW_CONFIRM_GO_BACK,
                REVIEW_CONFIRM_GO_BACK_DESC.to_string(),
            ),
        ];
        entries
            .iter()
            .enumerate()
            .map(|(idx, (label, description))| {
                let prefix = if idx == selected { '›' } else { ' ' };
                let number = idx + 1;
                GenericDisplayRow {
                    name: format!("{prefix} {number}. {label}"),
                    display_shortcut: None,
                    match_indices: None,
                    description: Some(description.clone()),
                    disabled_reason: None,
                    is_dimmed: false,
                    wrap_indent: None,
                }
            })
            .collect()
    }

    /// Build the response payload and dispatch it to the app.
    fn submit_answers(&mut self) {
        self.confirm_unanswered = None;
        self.confirm_review = None;

        let answers = self.collect_answers();
        self.app_event_tx
            .send(AppEvent::CodexOp(Op::UserInputAnswer {
                id: self.request.turn_id.clone(),
                response: RequestUserInputResponse { answers },
            }));

        if let Some(next) = self.queue.pop_front() {
            self.request = next;
            self.reset_for_request();
            self.ensure_focus_available();
        } else {
            self.done = true;
        }
    }

    fn open_unanswered_confirmation(&mut self) {
        let mut state = ScrollState::new();
        state.selected_idx = Some(0);
        self.confirm_unanswered = Some(state);
    }

    fn close_unanswered_confirmation(&mut self) {
        self.confirm_unanswered = None;
    }

    fn unanswered_question_count(&self) -> usize {
        self.unanswered_count()
    }

    fn unanswered_submit_description(&self) -> String {
        let count = self.unanswered_question_count();
        let suffix = if count == 1 { "question" } else { "questions" };
        format!("Submit with {count} unanswered {suffix}.")
    }

    fn first_unanswered_index(&self) -> Option<usize> {
        self.request
            .questions
            .iter()
            .enumerate()
            .find(|(idx, _)| !self.is_question_answered(*idx))
            .map(|(idx, _)| idx)
    }

    fn unanswered_confirmation_rows(&self) -> Vec<GenericDisplayRow> {
        let selected = self
            .confirm_unanswered
            .as_ref()
            .and_then(|state| state.selected_idx)
            .unwrap_or(0);
        let entries = [
            (
                UNANSWERED_CONFIRM_SUBMIT,
                self.unanswered_submit_description(),
            ),
            (
                UNANSWERED_CONFIRM_GO_BACK,
                UNANSWERED_CONFIRM_GO_BACK_DESC.to_string(),
            ),
        ];

        entries
            .iter()
            .enumerate()
            .map(|(idx, (label, description))| {
                let prefix = if idx == selected { '›' } else { ' ' };
                let number = idx + 1;
                GenericDisplayRow {
                    name: format!("{prefix} {number}. {label}"),
                    display_shortcut: None,
                    match_indices: None,
                    description: Some(description.clone()),
                    disabled_reason: None,
                    is_dimmed: false,
                    wrap_indent: None,
                }
            })
            .collect()
    }

    fn is_question_answered(&self, idx: usize) -> bool {
        let Some(question) = self.request.questions.get(idx) else {
            return false;
        };
        let Some(answer) = self.answers.get(idx) else {
            return false;
        };
        let has_options = question
            .options
            .as_ref()
            .is_some_and(|options| !options.is_empty());
        let has_notes = !answer.notes.text.text().trim().is_empty();

        if has_options {
            !answer.selected_option_indices.is_empty() || has_notes
        } else {
            has_notes
        }
    }

    /// Count questions that would submit an empty answer list.
    fn unanswered_count(&self) -> usize {
        self.request
            .questions
            .iter()
            .enumerate()
            .filter(|(idx, _)| !self.is_question_answered(*idx))
            .count()
    }

    /// Compute the preferred notes input height for the current question.
    fn notes_input_height(&self, width: u16) -> u16 {
        let Some(entry) = self.current_notes_entry() else {
            return 3;
        };
        let usable_width = width.saturating_sub(2);
        let text_height = entry.text.desired_height(usable_width).clamp(1, 6);
        text_height.saturating_add(2).clamp(3, 8)
    }

    fn go_next_or_submit(&mut self) {
        if self.current_index() + 1 >= self.question_count() {
            if self.unanswered_count() > 0 {
                self.open_unanswered_confirmation();
            } else {
                self.open_review_confirmation();
            }
        } else {
            self.move_question(true);
        }
    }

    fn handle_confirm_unanswered_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }
        let Some(state) = self.confirm_unanswered.as_mut() else {
            return;
        };

        match key_event.code {
            KeyCode::Esc | KeyCode::Backspace => {
                self.close_unanswered_confirmation();
                if let Some(idx) = self.first_unanswered_index() {
                    self.jump_to_question(idx);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.move_up_wrap(2);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.move_down_wrap(2);
            }
            KeyCode::Enter => {
                let selected = state.selected_idx.unwrap_or(0);
                self.close_unanswered_confirmation();
                if selected == 0 {
                    self.open_review_confirmation();
                } else if let Some(idx) = self.first_unanswered_index() {
                    self.jump_to_question(idx);
                }
            }
            KeyCode::Char('1') | KeyCode::Char('2') => {
                let idx = if matches!(key_event.code, KeyCode::Char('1')) {
                    0
                } else {
                    1
                };
                state.selected_idx = Some(idx);
            }
            _ => {}
        }
    }

    fn handle_confirm_review_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }
        let Some(state) = self.confirm_review.as_mut() else {
            return;
        };

        match key_event.code {
            KeyCode::Esc | KeyCode::Backspace => {
                self.close_review_confirmation();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.move_up_wrap(2);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.move_down_wrap(2);
            }
            KeyCode::Enter => {
                let selected = state.selected_idx.unwrap_or(0);
                self.close_review_confirmation();
                if selected == 0 {
                    self.submit_answers();
                }
            }
            KeyCode::Char('1') | KeyCode::Char('2') => {
                let idx = if matches!(key_event.code, KeyCode::Char('1')) {
                    0
                } else {
                    1
                };
                state.selected_idx = Some(idx);
            }
            _ => {}
        }
    }
}

impl BottomPaneView for RequestUserInputOverlay {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }

        if self.confirm_unanswered_active() {
            self.handle_confirm_unanswered_key_event(key_event);
            return;
        }
        if self.confirm_review_active() {
            self.handle_confirm_review_key_event(key_event);
            return;
        }

        if matches!(key_event.code, KeyCode::Esc) {
            self.app_event_tx.send(AppEvent::CodexOp(Op::Interrupt));
            self.done = true;
            return;
        }

        // Question navigation is always available.
        match key_event.code {
            KeyCode::PageUp => {
                self.move_question(false);
                return;
            }
            KeyCode::PageDown => {
                self.move_question(true);
                return;
            }
            _ => {}
        }

        match self.focus {
            Focus::Options => {
                let options_len = self.options_len();
                match key_event.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(answer) = self.current_answer_mut() {
                            answer.option_state.move_up_wrap(options_len);
                            answer.answer_committed = false;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(answer) = self.current_answer_mut() {
                            answer.option_state.move_down_wrap(options_len);
                            answer.answer_committed = false;
                        }
                    }
                    KeyCode::Char(' ') => {
                        if let Some(option_idx) = self.selected_option_index() {
                            self.toggle_option_selection(option_idx);
                        }
                    }
                    KeyCode::Enter => {
                        self.commit_current_question();
                        self.go_next_or_submit();
                    }
                    KeyCode::Tab => {
                        if self.has_selected_options() {
                            self.focus = Focus::Notes;
                        }
                    }
                    KeyCode::Char(ch) => {
                        if let Some(option_idx) = self.option_index_for_digit(ch) {
                            self.toggle_option_selection(option_idx);
                        }
                    }
                    KeyCode::Backspace | KeyCode::Delete => {
                        if let Some(answer) = self.current_answer_mut() {
                            answer.selected_option_indices.clear();
                            answer.answer_committed = false;
                        }
                    }
                    _ => {}
                }
            }
            Focus::Notes => {
                if self.has_options() && matches!(key_event.code, KeyCode::Tab) {
                    self.focus = Focus::Options;
                    return;
                }
                if matches!(key_event.code, KeyCode::Enter) {
                    self.commit_current_question();
                    self.go_next_or_submit();
                    return;
                }
                if self.has_options() && matches!(key_event.code, KeyCode::Up | KeyCode::Down) {
                    let options_len = self.options_len();
                    if let Some(answer) = self.current_answer_mut() {
                        match key_event.code {
                            KeyCode::Up => {
                                answer.option_state.move_up_wrap(options_len);
                            }
                            KeyCode::Down => {
                                answer.option_state.move_down_wrap(options_len);
                            }
                            _ => {}
                        }
                        answer.answer_committed = false;
                    }
                    return;
                }
                if matches!(
                    key_event.code,
                    KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
                ) && let Some(answer) = self.current_answer_mut()
                {
                    answer.answer_committed = false;
                }
                if let Some(entry) = self.current_notes_entry_mut() {
                    entry.text.input(key_event);
                }
            }
        }
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        if self.confirm_unanswered_active() {
            self.close_unanswered_confirmation();
            self.app_event_tx.send(AppEvent::CodexOp(Op::Interrupt));
            self.done = true;
            return CancellationEvent::Handled;
        }
        if self.confirm_review_active() {
            self.close_review_confirmation();
            return CancellationEvent::Handled;
        }

        self.app_event_tx.send(AppEvent::CodexOp(Op::Interrupt));
        self.done = true;
        CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.done
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        if pasted.is_empty() {
            return false;
        }
        if matches!(self.focus, Focus::Options) {
            self.focus = Focus::Notes;
        }
        if let Some(answer) = self.current_answer_mut() {
            answer.answer_committed = false;
        }
        if let Some(entry) = self.current_notes_entry_mut() {
            entry.text.insert_str(&pasted);
            return true;
        }
        false
    }

    fn try_consume_user_input_request(
        &mut self,
        request: RequestUserInputEvent,
    ) -> Option<RequestUserInputEvent> {
        if self.done {
            return Some(request);
        }
        self.queue.push_back(request);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn test_sender() -> (
        AppEventSender,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        (AppEventSender::new(tx_raw), rx)
    }

    fn question_with_options(
        id: &str,
        header: &str,
    ) -> codex_protocol::request_user_input::RequestUserInputQuestion {
        codex_protocol::request_user_input::RequestUserInputQuestion {
            id: id.to_string(),
            header: header.to_string(),
            question: "Choose an option.".to_string(),
            is_other: false,
            is_secret: false,
            options: Some(vec![
                codex_protocol::request_user_input::RequestUserInputQuestionOption {
                    label: "Option 1".to_string(),
                    description: "First choice.".to_string(),
                },
                codex_protocol::request_user_input::RequestUserInputQuestionOption {
                    label: "Option 2".to_string(),
                    description: "Second choice.".to_string(),
                },
                codex_protocol::request_user_input::RequestUserInputQuestionOption {
                    label: "Option 3".to_string(),
                    description: "Third choice.".to_string(),
                },
            ]),
        }
    }

    fn request_event(
        turn_id: &str,
        questions: Vec<codex_protocol::request_user_input::RequestUserInputQuestion>,
    ) -> RequestUserInputEvent {
        RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: turn_id.to_string(),
            questions,
        }
    }

    fn question_without_options(
        id: &str,
        header: &str,
    ) -> codex_protocol::request_user_input::RequestUserInputQuestion {
        codex_protocol::request_user_input::RequestUserInputQuestion {
            id: id.to_string(),
            header: header.to_string(),
            question: "Add details.".to_string(),
            is_other: false,
            is_secret: false,
            options: None,
        }
    }

    #[test]
    fn selecting_new_option_replaces_previous_selection() {
        let (tx, mut rx) = test_sender();
        let mut overlay = RequestUserInputOverlay::new(
            request_event("turn-1", vec![question_with_options("q1", "Pick one")]),
            tx,
        );

        overlay.handle_key_event(KeyEvent::from(KeyCode::Char('1')));
        overlay.handle_key_event(KeyEvent::from(KeyCode::Char('3')));
        overlay.handle_key_event(KeyEvent::from(KeyCode::Enter));
        assert!(overlay.confirm_review_active());
        overlay.handle_key_event(KeyEvent::from(KeyCode::Enter));

        let event = rx.try_recv().expect("expected AppEvent");
        let AppEvent::CodexOp(Op::UserInputAnswer { response, .. }) = event else {
            panic!("expected UserInputAnswer");
        };
        let answer = response.answers.get("q1").expect("answer missing");
        assert_eq!(answer.answers, vec!["Option 3".to_string()]);
    }

    #[test]
    fn unanswered_confirmation_precedes_review_confirmation() {
        let (tx, _rx) = test_sender();
        let mut overlay = RequestUserInputOverlay::new(
            request_event(
                "turn-1",
                vec![
                    question_with_options("q1", "Pick one"),
                    question_without_options("q2", "Details"),
                ],
            ),
            tx,
        );

        overlay.handle_key_event(KeyEvent::from(KeyCode::Enter));
        overlay.handle_key_event(KeyEvent::from(KeyCode::Enter));

        assert!(overlay.confirm_unanswered_active());
        assert!(!overlay.confirm_review_active());

        overlay.handle_key_event(KeyEvent::from(KeyCode::Enter));
        assert!(overlay.confirm_review_active());
    }

    #[test]
    fn option_toggle_marks_question_answered_without_enter() {
        let (tx, _rx) = test_sender();
        let mut overlay = RequestUserInputOverlay::new(
            request_event("turn-1", vec![question_with_options("q1", "Pick one")]),
            tx,
        );

        overlay.handle_key_event(KeyEvent::from(KeyCode::Char('2')));

        assert_eq!(overlay.unanswered_count(), 0);
    }

    #[test]
    fn freeform_text_counts_as_answered_without_enter() {
        let (tx, _rx) = test_sender();
        let mut overlay = RequestUserInputOverlay::new(
            request_event("turn-1", vec![question_without_options("q1", "Details")]),
            tx,
        );

        overlay.handle_key_event(KeyEvent::from(KeyCode::Char('a')));

        assert_eq!(overlay.unanswered_count(), 0);
    }
}
