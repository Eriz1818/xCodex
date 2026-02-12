use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidgetRef;
use ratatui::widgets::Widget;

use crate::bottom_pane::selection_popup_common::GenericDisplayRow;
use crate::bottom_pane::selection_popup_common::render_rows;
use crate::key_hint;
use crate::render::renderable::Renderable;

use super::RequestUserInputOverlay;

impl Renderable for RequestUserInputOverlay {
    fn desired_height(&self, width: u16) -> u16 {
        if self.confirm_unanswered_active() {
            return self.unanswered_confirmation_height(width);
        }
        if self.confirm_review_active() {
            return self.review_confirmation_height(width);
        }

        let sections = self.layout_sections(Rect::new(0, 0, width, u16::MAX));
        let mut height = sections
            .question_lines
            .len()
            .saturating_add(5)
            .saturating_add(self.notes_input_height(width) as usize)
            .saturating_add(sections.footer_lines as usize);
        if self.has_options() {
            height = height.saturating_add(2);
        }
        height = height.max(8);
        height as u16
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ui(area, buf);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.cursor_pos_impl(area)
    }
}

impl RequestUserInputOverlay {
    fn unanswered_confirmation_height(&self, width: u16) -> u16 {
        let content_width = width.max(1) as usize;
        let title_lines = textwrap::wrap(super::UNANSWERED_CONFIRM_TITLE, content_width).len();
        let subtitle_lines = textwrap::wrap(
            &format!(
                "{} unanswered question{}",
                self.unanswered_question_count(),
                if self.unanswered_question_count() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            content_width,
        )
        .len();
        let rows = self.unanswered_confirmation_rows();
        let rows_height = rows.len().max(1);
        let hint_lines = 1usize;

        let height = title_lines
            .saturating_add(subtitle_lines)
            .saturating_add(1)
            .saturating_add(rows_height)
            .saturating_add(1)
            .saturating_add(hint_lines)
            .max(8);
        height as u16
    }

    fn review_confirmation_height(&self, width: u16) -> u16 {
        let content_width = width.max(1) as usize;
        let title_lines = textwrap::wrap(super::REVIEW_CONFIRM_TITLE, content_width).len();
        let subtitle_lines = textwrap::wrap(
            &format!("{} question(s)", self.question_count()),
            content_width,
        )
        .len();
        let summary_lines = self
            .review_answer_summaries()
            .into_iter()
            .map(|line| textwrap::wrap(&line, content_width).len())
            .sum::<usize>();
        let rows = self.review_confirmation_rows();
        let rows_height = rows.len().max(1);
        let hint_lines = 1usize;

        let height = title_lines
            .saturating_add(subtitle_lines)
            .saturating_add(1)
            .saturating_add(summary_lines)
            .saturating_add(1)
            .saturating_add(rows_height)
            .saturating_add(1)
            .saturating_add(hint_lines)
            .max(10);
        height as u16
    }

    fn render_unanswered_confirmation(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let base_style = crate::theme::transcript_style();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(base_style);
            }
        }

        let mut cursor_y = area.y;
        let title = textwrap::wrap(super::UNANSWERED_CONFIRM_TITLE, area.width.max(1) as usize)
            .into_iter()
            .map(std::borrow::Cow::into_owned)
            .collect::<Vec<_>>();
        for line in title {
            if cursor_y >= area.bottom() {
                return;
            }
            Paragraph::new(Line::from(line.bold())).render(
                Rect {
                    x: area.x,
                    y: cursor_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            cursor_y = cursor_y.saturating_add(1);
        }

        let subtitle_text = format!(
            "{} unanswered question{}",
            self.unanswered_question_count(),
            if self.unanswered_question_count() == 1 {
                ""
            } else {
                "s"
            }
        );
        for line in textwrap::wrap(&subtitle_text, area.width.max(1) as usize) {
            if cursor_y >= area.bottom() {
                return;
            }
            Paragraph::new(Line::from(line.into_owned().dim())).render(
                Rect {
                    x: area.x,
                    y: cursor_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            cursor_y = cursor_y.saturating_add(1);
        }

        if cursor_y < area.bottom() {
            cursor_y = cursor_y.saturating_add(1);
        }

        let hint_height = 1u16;
        let rows = self.unanswered_confirmation_rows();
        let rows_area = Rect {
            x: area.x,
            y: cursor_y,
            width: area.width,
            height: area
                .bottom()
                .saturating_sub(cursor_y)
                .saturating_sub(hint_height)
                .max(1),
        };
        render_rows(
            rows_area,
            buf,
            &rows,
            &self.confirm_unanswered.unwrap_or_default(),
            rows.len().max(1),
            base_style,
            "No choices",
        );

        let hint_line = Line::from(vec![
            key_hint::plain(KeyCode::Up).into(),
            "/".into(),
            key_hint::plain(KeyCode::Down).into(),
            " move | ".into(),
            key_hint::plain(KeyCode::Enter).into(),
            " choose | ".into(),
            key_hint::plain(KeyCode::Esc).into(),
            " go back".into(),
        ])
        .dim();

        Paragraph::new(hint_line).render(
            Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            },
            buf,
        );
    }

    fn render_review_confirmation(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let base_style = crate::theme::transcript_style();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(base_style);
            }
        }

        let mut cursor_y = area.y;
        for line in textwrap::wrap(super::REVIEW_CONFIRM_TITLE, area.width.max(1) as usize) {
            if cursor_y >= area.bottom() {
                return;
            }
            Paragraph::new(Line::from(line.into_owned().bold())).render(
                Rect {
                    x: area.x,
                    y: cursor_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            cursor_y = cursor_y.saturating_add(1);
        }

        let subtitle = format!("{} question(s)", self.question_count());
        for line in textwrap::wrap(&subtitle, area.width.max(1) as usize) {
            if cursor_y >= area.bottom() {
                return;
            }
            Paragraph::new(Line::from(line.into_owned().dim())).render(
                Rect {
                    x: area.x,
                    y: cursor_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            cursor_y = cursor_y.saturating_add(1);
        }

        if cursor_y < area.bottom() {
            cursor_y = cursor_y.saturating_add(1);
        }

        for summary in self.review_answer_summaries() {
            for line in textwrap::wrap(&format!("- {summary}"), area.width.max(1) as usize) {
                if cursor_y >= area.bottom() {
                    return;
                }
                Paragraph::new(Line::from(line.into_owned().dim())).render(
                    Rect {
                        x: area.x,
                        y: cursor_y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );
                cursor_y = cursor_y.saturating_add(1);
            }
        }

        if cursor_y < area.bottom() {
            cursor_y = cursor_y.saturating_add(1);
        }

        let hint_height = 1u16;
        let rows = self.review_confirmation_rows();
        let rows_area = Rect {
            x: area.x,
            y: cursor_y,
            width: area.width,
            height: area
                .bottom()
                .saturating_sub(cursor_y)
                .saturating_sub(hint_height)
                .max(1),
        };
        render_rows(
            rows_area,
            buf,
            &rows,
            &self.confirm_review.unwrap_or_default(),
            rows.len().max(1),
            base_style,
            "No choices",
        );

        let hint_line = Line::from(vec![
            key_hint::plain(KeyCode::Up).into(),
            "/".into(),
            key_hint::plain(KeyCode::Down).into(),
            " move | ".into(),
            key_hint::plain(KeyCode::Enter).into(),
            " choose | ".into(),
            key_hint::plain(KeyCode::Esc).into(),
            " go back".into(),
        ])
        .dim();

        Paragraph::new(hint_line).render(
            Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            },
            buf,
        );
    }

    /// Render the full request-user-input overlay.
    pub(super) fn render_ui(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.confirm_unanswered_active() {
            self.render_unanswered_confirmation(area, buf);
            return;
        }
        if self.confirm_review_active() {
            self.render_review_confirmation(area, buf);
            return;
        }

        let sections = self.layout_sections(area);

        // Progress header keeps the user oriented across multiple questions.
        let progress_line = if self.question_count() > 0 {
            let idx = self.current_index() + 1;
            let total = self.question_count();
            let unanswered = self.unanswered_count();
            if unanswered > 0 {
                Line::from(format!("Question {idx}/{total} ({unanswered} unanswered)").dim())
            } else {
                Line::from(format!("Question {idx}/{total}").dim())
            }
        } else {
            Line::from("No questions".dim())
        };
        Paragraph::new(progress_line).render(sections.progress_area, buf);

        // Question title and wrapped prompt text.
        let question_header = self.current_question().map(|q| q.header.clone());
        let header_line = if let Some(header) = question_header {
            Line::from(header.bold())
        } else {
            Line::from("No questions".dim())
        };
        Paragraph::new(header_line).render(sections.header_area, buf);

        let question_y = sections.question_area.y;
        for (offset, line) in sections.question_lines.iter().enumerate() {
            if question_y.saturating_add(offset as u16)
                >= sections.question_area.y + sections.question_area.height
            {
                break;
            }
            Paragraph::new(Line::from(line.clone())).render(
                Rect {
                    x: sections.question_area.x,
                    y: question_y.saturating_add(offset as u16),
                    width: sections.question_area.width,
                    height: 1,
                },
                buf,
            );
        }

        if sections.answer_title_area.height > 0 {
            let answer_label = "Answer";
            let answer_title = if self.focus_is_options() || self.focus_is_notes_without_options() {
                Span::styled(
                    answer_label,
                    crate::theme::accent_style().add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(answer_label, crate::theme::dim_style())
            };
            Paragraph::new(Line::from(answer_title)).render(sections.answer_title_area, buf);
        }

        // Build rows with selection markers for the shared selection renderer.
        let option_rows = self
            .current_question()
            .and_then(|question| question.options.as_ref())
            .map(|options| {
                let focused_idx = self.selected_option_index();
                let selected = self
                    .current_answer()
                    .map(|answer| answer.selected_option_indices.clone())
                    .unwrap_or_default();
                options
                    .iter()
                    .enumerate()
                    .map(|(idx, opt)| {
                        let focused = focused_idx.is_some_and(|focused| focused == idx);
                        let checked = selected.contains(&idx);
                        let cursor = if focused { '›' } else { ' ' };
                        let check = if checked { 'x' } else { ' ' };
                        GenericDisplayRow {
                            name: format!("{cursor} {}. [{check}] {}", idx + 1, opt.label),
                            display_shortcut: None,
                            match_indices: None,
                            description: Some(opt.description.clone()),
                            disabled_reason: None,
                            is_dimmed: false,
                            wrap_indent: None,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if self.has_options() {
            let mut option_state = self
                .current_answer()
                .map(|answer| answer.option_state)
                .unwrap_or_default();
            if sections.options_area.height > 0 {
                // Ensure the selected option is visible in the scroll window.
                option_state
                    .ensure_visible(option_rows.len(), sections.options_area.height as usize);
                let base_style = crate::theme::transcript_style();
                render_rows(
                    sections.options_area,
                    buf,
                    &option_rows,
                    &option_state,
                    option_rows.len().max(1),
                    base_style,
                    "No options",
                );
            }
        }

        if sections.notes_title_area.height > 0 {
            let notes_label = "Notes (optional)";
            let notes_title = if self.focus_is_notes() {
                Span::styled(
                    notes_label,
                    crate::theme::accent_style().add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(notes_label, crate::theme::dim_style())
            };
            Paragraph::new(Line::from(notes_title)).render(sections.notes_title_area, buf);
        }

        if sections.notes_area.height > 0 {
            self.render_notes_input(sections.notes_area, buf);
        }

        let footer_y = sections
            .notes_area
            .y
            .saturating_add(sections.notes_area.height);
        if sections.footer_lines == 2 {
            let warning = format!(
                "Unanswered: {} | Will submit as skipped",
                self.unanswered_count()
            );
            Paragraph::new(Line::from(warning.dim())).render(
                Rect {
                    x: area.x,
                    y: footer_y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
        }

        let hint_y = footer_y.saturating_add(sections.footer_lines.saturating_sub(1));
        let mut hint_spans = Vec::new();
        if self.has_options() {
            let options_len = self.options_len();
            let option_index = self.selected_option_index().map_or(0, |idx| idx + 1);
            hint_spans.extend(vec![
                format!("Option {option_index}/{options_len}").into(),
                " | ".into(),
                "space select".into(),
                " | ".into(),
            ]);
        }
        hint_spans.extend(vec![
            key_hint::plain(KeyCode::Up).into(),
            "/".into(),
            key_hint::plain(KeyCode::Down).into(),
            " scroll | ".into(),
            key_hint::plain(KeyCode::Enter).into(),
            " next".into(),
        ]);
        if self.question_count() > 1 {
            hint_spans.extend(vec![
                " | ".into(),
                key_hint::plain(KeyCode::PageUp).into(),
                "/".into(),
                key_hint::plain(KeyCode::PageDown).into(),
                " question".into(),
            ]);
        }
        hint_spans.extend(vec![
            " | ".into(),
            key_hint::plain(KeyCode::Esc).into(),
            " interrupt".into(),
        ]);
        Paragraph::new(Line::from(hint_spans).dim()).render(
            Rect {
                x: area.x,
                y: hint_y,
                width: area.width,
                height: 1,
            },
            buf,
        );
    }

    /// Return the cursor position when editing notes, if visible.
    pub(super) fn cursor_pos_impl(&self, area: Rect) -> Option<(u16, u16)> {
        if self.confirm_unanswered_active()
            || self.confirm_review_active()
            || !self.focus_is_notes()
        {
            return None;
        }
        let sections = self.layout_sections(area);
        let entry = self.current_notes_entry()?;
        let input_area = sections.notes_area;
        if input_area.width <= 2 || input_area.height == 0 {
            return None;
        }
        if input_area.height < 3 {
            // Inline notes layout uses a prefix and a single-line text area.
            let prefix = notes_prefix();
            let prefix_width = prefix.len() as u16;
            if input_area.width <= prefix_width {
                return None;
            }
            let textarea_rect = Rect {
                x: input_area.x.saturating_add(prefix_width),
                y: input_area.y,
                width: input_area.width.saturating_sub(prefix_width),
                height: 1,
            };
            let state = *entry.state.borrow();
            return entry.text.cursor_pos_with_state(textarea_rect, state);
        }
        let text_area_height = input_area.height.saturating_sub(2);
        let textarea_rect = Rect {
            x: input_area.x.saturating_add(1),
            y: input_area.y.saturating_add(1),
            width: input_area.width.saturating_sub(2),
            height: text_area_height,
        };
        let state = *entry.state.borrow();
        entry.text.cursor_pos_with_state(textarea_rect, state)
    }

    fn render_notes_input(&self, area: Rect, buf: &mut Buffer) {
        let entry = self.current_notes_entry();
        let Some(entry) = entry else {
            return;
        };
        if area.height == 0 || area.width == 0 {
            return;
        }

        if area.height < 3 {
            let prefix = notes_prefix();
            let prefix_width = prefix.len() as u16;
            Paragraph::new(Line::from(prefix)).render(
                Rect {
                    x: area.x,
                    y: area.y,
                    width: prefix_width,
                    height: 1,
                },
                buf,
            );
            let textarea_rect = Rect {
                x: area.x.saturating_add(prefix_width),
                y: area.y,
                width: area.width.saturating_sub(prefix_width),
                height: 1,
            };
            if textarea_rect.width == 0 {
                return;
            }
            Clear.render(textarea_rect, buf);
            let mut state = entry.state.borrow_mut();
            StatefulWidgetRef::render_ref(&(&entry.text), textarea_rect, buf, &mut state);
            if entry.text.text().is_empty() {
                Paragraph::new(Line::from(self.notes_placeholder().dim()))
                    .render(textarea_rect, buf);
            }
            return;
        }

        Paragraph::new(Line::from(notes_prefix())).render(Rect { height: 1, ..area }, buf);
        let text_area_height = area.height.saturating_sub(2);
        let textarea_rect = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: text_area_height,
        };
        if textarea_rect.width == 0 || textarea_rect.height == 0 {
            return;
        }
        Clear.render(textarea_rect, buf);
        let mut state = entry.state.borrow_mut();
        StatefulWidgetRef::render_ref(&(&entry.text), textarea_rect, buf, &mut state);
        if entry.text.text().is_empty() {
            Paragraph::new(Line::from(self.notes_placeholder().dim())).render(textarea_rect, buf);
        }
    }
}

fn notes_prefix() -> &'static str {
    "› "
}
