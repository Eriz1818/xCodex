use diffy::Hunk;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line as RtLine;
use ratatui::text::Span as RtSpan;
use ratatui::widgets::Paragraph;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use unicode_width::UnicodeWidthChar;

use crate::exec_command::relativize_to_home;
use crate::render::Insets;
use crate::render::highlight::highlight_code_block_to_lines;
use crate::render::highlight::supports_highlighting;
use crate::render::highlight::syntax_highlighting_enabled;
use crate::render::line_utils::merge_span_style;
use crate::render::line_utils::prefix_lines;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::InsetRenderable;
use crate::render::renderable::Renderable;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use codex_core::git_info::get_git_repo_root;
use codex_core::protocol::FileChange;

// Internal representation for diff line rendering
#[derive(Copy, Clone)]
enum DiffLineType {
    Insert,
    Delete,
    Context,
}

#[derive(Copy, Clone, Debug)]
enum DiffSurface {
    Transcript,
    Popup,
}

pub struct DiffSummary {
    changes: HashMap<PathBuf, FileChange>,
    cwd: PathBuf,
    diff_highlight: bool,
    side_by_side: bool,
    surface: DiffSurface,
}

impl DiffSummary {
    pub fn new(
        changes: HashMap<PathBuf, FileChange>,
        cwd: PathBuf,
        diff_highlight: bool,
        side_by_side: bool,
    ) -> Self {
        Self {
            changes,
            cwd,
            diff_highlight,
            side_by_side,
            surface: DiffSurface::Transcript,
        }
    }

    pub fn new_popup(
        changes: HashMap<PathBuf, FileChange>,
        cwd: PathBuf,
        diff_highlight: bool,
        side_by_side: bool,
    ) -> Self {
        Self {
            changes,
            cwd,
            diff_highlight,
            side_by_side,
            surface: DiffSurface::Popup,
        }
    }
}

impl Renderable for FileChange {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![];
        render_change(
            self,
            &mut lines,
            area.width as usize,
            false,
            false,
            None,
            DiffSurface::Transcript,
        );
        Paragraph::new(lines).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut lines = vec![];
        render_change(
            self,
            &mut lines,
            width as usize,
            false,
            false,
            None,
            DiffSurface::Transcript,
        );
        lines.len() as u16
    }
}

impl From<DiffSummary> for Box<dyn Renderable> {
    fn from(val: DiffSummary) -> Self {
        let mut rows: Vec<Box<dyn Renderable>> = vec![];

        for (i, row) in collect_rows(&val.changes).into_iter().enumerate() {
            if i > 0 {
                rows.push(Box::new(RtLine::from("")));
            }
            let mut path = RtLine::from(display_path_for(&row.path, &val.cwd));
            path.push_span(" ");
            path.extend(render_line_count_summary(
                row.added,
                row.removed,
                val.diff_highlight,
                val.surface,
            ));
            rows.push(Box::new(path));
            rows.push(Box::new(RtLine::from("")));
            rows.push(Box::new(InsetRenderable::new(
                Box::new(ChangeRenderable::new(
                    row.change,
                    val.diff_highlight,
                    val.side_by_side,
                    row.lang.clone(),
                    val.surface,
                )) as Box<dyn Renderable>,
                Insets::tlbr(0, 2, 0, 0),
            )));
        }

        Box::new(ColumnRenderable::with(rows))
    }
}

pub(crate) fn create_diff_summary(
    changes: &HashMap<PathBuf, FileChange>,
    cwd: &Path,
    wrap_cols: usize,
    diff_highlight: bool,
    side_by_side: bool,
) -> Vec<RtLine<'static>> {
    let rows = collect_rows(changes);
    render_changes_block(rows, wrap_cols, cwd, diff_highlight, side_by_side)
}

// Shared row for per-file presentation
#[derive(Clone)]
struct Row {
    #[allow(dead_code)]
    path: PathBuf,
    move_path: Option<PathBuf>,
    added: usize,
    removed: usize,
    change: FileChange,
    lang: Option<String>,
}

struct ChangeRenderable {
    change: FileChange,
    diff_highlight: bool,
    side_by_side: bool,
    lang: Option<String>,
    surface: DiffSurface,
}

impl ChangeRenderable {
    fn new(
        change: FileChange,
        diff_highlight: bool,
        side_by_side: bool,
        lang: Option<String>,
        surface: DiffSurface,
    ) -> Self {
        Self {
            change,
            diff_highlight,
            side_by_side,
            lang,
            surface,
        }
    }
}

impl Renderable for ChangeRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![];
        render_change(
            &self.change,
            &mut lines,
            area.width as usize,
            self.diff_highlight,
            self.side_by_side,
            self.lang.as_deref(),
            self.surface,
        );
        Paragraph::new(lines).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut lines = vec![];
        render_change(
            &self.change,
            &mut lines,
            width as usize,
            self.diff_highlight,
            self.side_by_side,
            self.lang.as_deref(),
            self.surface,
        );
        lines.len() as u16
    }
}

fn collect_rows(changes: &HashMap<PathBuf, FileChange>) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (path, change) in changes.iter() {
        let (added, removed) = match change {
            FileChange::Add { content } => (content.lines().count(), 0),
            FileChange::Delete { content } => (0, content.lines().count()),
            FileChange::Update { unified_diff, .. } => calculate_add_remove_from_diff(unified_diff),
        };
        let move_path = match change {
            FileChange::Update {
                move_path: Some(new),
                ..
            } => Some(new.clone()),
            _ => None,
        };
        rows.push(Row {
            path: path.clone(),
            move_path,
            added,
            removed,
            change: change.clone(),
            lang: guess_lang_for_path(path),
        });
    }
    rows.sort_by_key(|r| r.path.clone());
    rows
}

fn guess_lang_for_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy();
    let lang = match ext.as_ref() {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "rb" => "ruby",
        "sh" | "bash" => "bash",
        _ => return None,
    };
    Some(lang.to_string())
}

fn render_line_count_summary(
    added: usize,
    removed: usize,
    diff_highlight: bool,
    surface: DiffSurface,
) -> Vec<RtSpan<'static>> {
    let mut spans = Vec::new();
    let base = surface_style(surface);
    spans.push(RtSpan::styled("(", base));
    spans.push(RtSpan::styled(
        format!("+{added}"),
        base.patch(style_add(diff_highlight)),
    ));
    spans.push(RtSpan::styled(" ", base));
    spans.push(RtSpan::styled(
        format!("-{removed}"),
        base.patch(style_del(diff_highlight)),
    ));
    spans.push(RtSpan::styled(")", base));
    spans
}

fn render_changes_block(
    rows: Vec<Row>,
    wrap_cols: usize,
    cwd: &Path,
    diff_highlight: bool,
    side_by_side: bool,
) -> Vec<RtLine<'static>> {
    let mut out: Vec<RtLine<'static>> = Vec::new();

    let render_path = |row: &Row| -> Vec<RtSpan<'static>> {
        let mut spans = Vec::new();
        spans.push(display_path_for(&row.path, cwd).into());
        if let Some(move_path) = &row.move_path {
            spans.push(format!(" → {}", display_path_for(move_path, cwd)).into());
        }
        spans
    };

    // Header
    let total_added: usize = rows.iter().map(|r| r.added).sum();
    let total_removed: usize = rows.iter().map(|r| r.removed).sum();
    let file_count = rows.len();
    let noun = if file_count == 1 { "file" } else { "files" };
    let mut header_spans: Vec<RtSpan<'static>> =
        vec![RtSpan::styled("• ", crate::theme::border_style())];
    if let [row] = &rows[..] {
        let verb = match &row.change {
            FileChange::Add { .. } => "Added",
            FileChange::Delete { .. } => "Deleted",
            _ => "Edited",
        };
        header_spans.push(verb.bold());
        header_spans.push(" ".into());
        header_spans.extend(render_path(row));
        header_spans.push(" ".into());
        header_spans.extend(render_line_count_summary(
            row.added,
            row.removed,
            diff_highlight,
            DiffSurface::Transcript,
        ));
    } else {
        header_spans.push("Edited".bold());
        header_spans.push(format!(" {file_count} {noun} ").into());
        header_spans.extend(render_line_count_summary(
            total_added,
            total_removed,
            diff_highlight,
            DiffSurface::Transcript,
        ));
    }
    out.push(RtLine::from(header_spans));

    for (idx, r) in rows.into_iter().enumerate() {
        // Insert a blank separator between file chunks (except before the first)
        if idx > 0 {
            out.push(RtLine::from(""));
        }
        // File header line (skip when single-file header already shows the name)
        let skip_file_header = file_count == 1;
        if !skip_file_header {
            let mut header: Vec<RtSpan<'static>> = Vec::new();
            header.push(RtSpan::styled("  └ ", crate::theme::border_style()));
            header.extend(render_path(&r));
            header.push(" ".into());
            header.extend(render_line_count_summary(
                r.added,
                r.removed,
                diff_highlight,
                DiffSurface::Transcript,
            ));
            out.push(RtLine::from(header));
        }

        let mut lines = vec![];
        render_change(
            &r.change,
            &mut lines,
            wrap_cols - 4,
            diff_highlight,
            side_by_side,
            r.lang.as_deref(),
            DiffSurface::Transcript,
        );
        out.extend(prefix_lines(lines, "    ".into(), "    ".into()));
    }

    out
}

fn render_change(
    change: &FileChange,
    out: &mut Vec<RtLine<'static>>,
    width: usize,
    diff_highlight: bool,
    side_by_side: bool,
    lang: Option<&str>,
    surface: DiffSurface,
) {
    let use_diff_background = diff_highlight;
    let highlight_lang = lang.filter(|value| supports_highlighting(value));
    let use_syntax = syntax_highlighting_enabled() && highlight_lang.is_some();
    if side_by_side {
        render_change_side_by_side(
            change,
            out,
            width,
            use_diff_background,
            use_syntax,
            highlight_lang,
            surface,
        );
        return;
    }
    match change {
        FileChange::Add { content } => {
            let line_number_width = line_number_width(content.lines().count());
            for (i, raw) in content.lines().enumerate() {
                out.extend(push_wrapped_diff_line(
                    i + 1,
                    DiffLineType::Insert,
                    raw,
                    width,
                    line_number_width,
                    use_diff_background,
                    use_syntax,
                    highlight_lang,
                    surface,
                ));
            }
        }
        FileChange::Delete { content } => {
            let line_number_width = line_number_width(content.lines().count());
            for (i, raw) in content.lines().enumerate() {
                out.extend(push_wrapped_diff_line(
                    i + 1,
                    DiffLineType::Delete,
                    raw,
                    width,
                    line_number_width,
                    use_diff_background,
                    use_syntax,
                    highlight_lang,
                    surface,
                ));
            }
        }
        FileChange::Update { unified_diff, .. } => {
            if let Ok(patch) = diffy::Patch::from_str(unified_diff) {
                let mut max_line_number = 0;
                for h in patch.hunks() {
                    let mut old_ln = h.old_range().start();
                    let mut new_ln = h.new_range().start();
                    for l in h.lines() {
                        match l {
                            diffy::Line::Insert(_) => {
                                max_line_number = max_line_number.max(new_ln);
                                new_ln += 1;
                            }
                            diffy::Line::Delete(_) => {
                                max_line_number = max_line_number.max(old_ln);
                                old_ln += 1;
                            }
                            diffy::Line::Context(_) => {
                                max_line_number = max_line_number.max(new_ln);
                                old_ln += 1;
                                new_ln += 1;
                            }
                        }
                    }
                }
                let line_number_width = line_number_width(max_line_number);
                let mut is_first_hunk = true;
                for h in patch.hunks() {
                    if !is_first_hunk {
                        let spacer = format!("{:width$} ", "", width = line_number_width.max(1));
                        let spacer_span = RtSpan::styled(spacer, style_gutter(surface));
                        out.push(RtLine::from(vec![
                            spacer_span,
                            RtSpan::styled("⋮", style_hunk(diff_highlight)),
                        ]));
                    }
                    is_first_hunk = false;

                    let mut old_ln = h.old_range().start();
                    let mut new_ln = h.new_range().start();
                    for l in h.lines() {
                        match l {
                            diffy::Line::Insert(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line(
                                    new_ln,
                                    DiffLineType::Insert,
                                    s,
                                    width,
                                    line_number_width,
                                    use_diff_background,
                                    use_syntax,
                                    highlight_lang,
                                    surface,
                                ));
                                new_ln += 1;
                            }
                            diffy::Line::Delete(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line(
                                    old_ln,
                                    DiffLineType::Delete,
                                    s,
                                    width,
                                    line_number_width,
                                    use_diff_background,
                                    use_syntax,
                                    highlight_lang,
                                    surface,
                                ));
                                old_ln += 1;
                            }
                            diffy::Line::Context(text) => {
                                let s = text.trim_end_matches('\n');
                                out.extend(push_wrapped_diff_line(
                                    new_ln,
                                    DiffLineType::Context,
                                    s,
                                    width,
                                    line_number_width,
                                    use_diff_background,
                                    use_syntax,
                                    highlight_lang,
                                    surface,
                                ));
                                old_ln += 1;
                                new_ln += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
enum SideBySideCell {
    Empty,
    Break,
    Diff {
        line_number: usize,
        kind: DiffLineType,
        text: String,
    },
}

#[derive(Clone)]
struct SideBySideRow {
    left: SideBySideCell,
    right: SideBySideCell,
}

fn flush_side_by_side_pending_rows(
    rows: &mut Vec<SideBySideRow>,
    pending_deletes: &mut Vec<(usize, String)>,
    pending_inserts: &mut Vec<(usize, String)>,
) {
    let mut deletes = pending_deletes.drain(..);
    let mut inserts = pending_inserts.drain(..);

    loop {
        let left = match deletes.next() {
            Some((line_number, text)) => SideBySideCell::Diff {
                line_number,
                kind: DiffLineType::Delete,
                text,
            },
            None => SideBySideCell::Empty,
        };
        let right = match inserts.next() {
            Some((line_number, text)) => SideBySideCell::Diff {
                line_number,
                kind: DiffLineType::Insert,
                text,
            },
            None => SideBySideCell::Empty,
        };

        if matches!(left, SideBySideCell::Empty) && matches!(right, SideBySideCell::Empty) {
            break;
        }

        rows.push(SideBySideRow { left, right });
    }
}

fn render_change_side_by_side(
    change: &FileChange,
    out: &mut Vec<RtLine<'static>>,
    width: usize,
    diff_highlight: bool,
    syntax_highlight: bool,
    lang: Option<&str>,
    surface: DiffSurface,
) {
    let mut rows: Vec<SideBySideRow> = Vec::new();
    let mut max_left_line_number = 0usize;
    let mut max_right_line_number = 0usize;

    match change {
        FileChange::Add { content } => {
            for (i, raw) in content.lines().enumerate() {
                let line_number = i + 1;
                max_right_line_number = max_right_line_number.max(line_number);
                rows.push(SideBySideRow {
                    left: SideBySideCell::Empty,
                    right: SideBySideCell::Diff {
                        line_number,
                        kind: DiffLineType::Insert,
                        text: raw.to_string(),
                    },
                });
            }
        }
        FileChange::Delete { content } => {
            for (i, raw) in content.lines().enumerate() {
                let line_number = i + 1;
                max_left_line_number = max_left_line_number.max(line_number);
                rows.push(SideBySideRow {
                    left: SideBySideCell::Diff {
                        line_number,
                        kind: DiffLineType::Delete,
                        text: raw.to_string(),
                    },
                    right: SideBySideCell::Empty,
                });
            }
        }
        FileChange::Update { unified_diff, .. } => {
            let Ok(patch) = diffy::Patch::from_str(unified_diff) else {
                return;
            };
            let mut is_first_hunk = true;
            for h in patch.hunks() {
                if !is_first_hunk {
                    rows.push(SideBySideRow {
                        left: SideBySideCell::Break,
                        right: SideBySideCell::Break,
                    });
                }
                is_first_hunk = false;

                let mut old_ln = h.old_range().start();
                let mut new_ln = h.new_range().start();
                let mut pending_deletes: Vec<(usize, String)> = Vec::new();
                let mut pending_inserts: Vec<(usize, String)> = Vec::new();
                for l in h.lines() {
                    match l {
                        diffy::Line::Insert(text) => {
                            let s = text.trim_end_matches('\n');
                            max_right_line_number = max_right_line_number.max(new_ln);
                            pending_inserts.push((new_ln, s.to_string()));
                            new_ln += 1;
                        }
                        diffy::Line::Delete(text) => {
                            let s = text.trim_end_matches('\n');
                            max_left_line_number = max_left_line_number.max(old_ln);
                            pending_deletes.push((old_ln, s.to_string()));
                            old_ln += 1;
                        }
                        diffy::Line::Context(text) => {
                            flush_side_by_side_pending_rows(
                                &mut rows,
                                &mut pending_deletes,
                                &mut pending_inserts,
                            );
                            let s = text.trim_end_matches('\n');
                            max_left_line_number = max_left_line_number.max(old_ln);
                            max_right_line_number = max_right_line_number.max(new_ln);
                            rows.push(SideBySideRow {
                                left: SideBySideCell::Diff {
                                    line_number: old_ln,
                                    kind: DiffLineType::Context,
                                    text: s.to_string(),
                                },
                                right: SideBySideCell::Diff {
                                    line_number: new_ln,
                                    kind: DiffLineType::Context,
                                    text: s.to_string(),
                                },
                            });
                            old_ln += 1;
                            new_ln += 1;
                        }
                    }
                }
                flush_side_by_side_pending_rows(
                    &mut rows,
                    &mut pending_deletes,
                    &mut pending_inserts,
                );
            }
        }
    }

    let left_line_number_width = line_number_width(max_left_line_number);
    let right_line_number_width = line_number_width(max_right_line_number);
    let (left_width, right_width) = side_by_side_column_widths(width);
    let separator_style = style_gutter(surface);
    let separator = RtSpan::styled(" │ ", separator_style);

    for row in rows {
        let left_lines = render_side_by_side_cell(
            &row.left,
            left_width,
            left_line_number_width,
            diff_highlight,
            syntax_highlight,
            lang,
            surface,
        );
        let right_lines = render_side_by_side_cell(
            &row.right,
            right_width,
            right_line_number_width,
            diff_highlight,
            syntax_highlight,
            lang,
            surface,
        );
        let row_count = left_lines.len().max(right_lines.len());
        for row_index in 0..row_count {
            let left_line = left_lines
                .get(row_index)
                .cloned()
                .unwrap_or_else(|| blank_side_line(left_width, surface));
            let right_line = right_lines
                .get(row_index)
                .cloned()
                .unwrap_or_else(|| blank_side_line(right_width, surface));
            let left_line = truncate_line_to_width(left_line, left_width);
            let right_line = truncate_line_to_width(right_line, right_width);
            let mut left_line = pad_line_to_width(left_line, left_width, style_context(surface));
            let right_line = pad_line_to_width(right_line, right_width, style_context(surface));
            left_line.spans.push(separator.clone());
            left_line.spans.extend(right_line.spans);
            out.push(truncate_line_to_width(left_line, width));
        }
    }
}

fn side_by_side_column_widths(width: usize) -> (usize, usize) {
    let separator_width = " │ ".chars().count();
    if width <= separator_width {
        return (1, 1);
    }

    let available_width = width.saturating_sub(separator_width);
    let mut left_width = available_width / 2;
    let mut right_width = available_width.saturating_sub(left_width);
    if left_width == 0 {
        left_width = 1;
    }
    if right_width == 0 {
        right_width = 1;
    }
    (left_width, right_width)
}

fn render_side_by_side_cell(
    cell: &SideBySideCell,
    width: usize,
    line_number_width: usize,
    diff_highlight: bool,
    syntax_highlight: bool,
    lang: Option<&str>,
    surface: DiffSurface,
) -> Vec<RtLine<'static>> {
    match cell {
        SideBySideCell::Empty => vec![blank_side_line(width, surface)],
        SideBySideCell::Break => vec![side_by_side_break_line(
            width,
            line_number_width,
            diff_highlight,
            surface,
        )],
        SideBySideCell::Diff {
            line_number,
            kind,
            text,
        } => push_wrapped_diff_line(
            *line_number,
            *kind,
            text,
            width,
            line_number_width,
            diff_highlight,
            syntax_highlight,
            lang,
            surface,
        ),
    }
}

fn side_by_side_break_line(
    width: usize,
    line_number_width: usize,
    diff_highlight: bool,
    surface: DiffSurface,
) -> RtLine<'static> {
    let spacer = format!("{:width$} ", "", width = line_number_width.max(1));
    let line = RtLine::from(vec![
        RtSpan::styled(spacer, style_gutter(surface)),
        RtSpan::styled("⋮", style_hunk(diff_highlight)),
    ]);
    pad_line_to_width(line, width, style_context(surface))
}

fn blank_side_line(width: usize, surface: DiffSurface) -> RtLine<'static> {
    RtLine::from(RtSpan::styled(" ".repeat(width), style_context(surface)))
}

fn pad_line_to_width(mut line: RtLine<'static>, width: usize, style: Style) -> RtLine<'static> {
    let line_width = line.width();
    if line_width < width {
        line.spans
            .push(RtSpan::styled(" ".repeat(width - line_width), style));
    }
    line
}

fn truncate_line_to_width(mut line: RtLine<'static>, max_width: usize) -> RtLine<'static> {
    if line.width() <= max_width {
        return line;
    }

    let mut used = 0usize;
    let mut spans: Vec<RtSpan<'static>> = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        if used >= max_width {
            break;
        }

        let mut kept = String::new();
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used.saturating_add(ch_width) > max_width {
                break;
            }
            kept.push(ch);
            used = used.saturating_add(ch_width);
        }

        if !kept.is_empty() {
            spans.push(RtSpan::styled(kept, span.style));
        }
    }

    line.spans = spans;
    line
}

/// Format a path for display relative to the current working directory when
/// possible, keeping output stable in jj/no-`.git` workspaces (e.g. image
/// tool calls should show `example.png` instead of an absolute path).
pub(crate) fn display_path_for(path: &Path, cwd: &Path) -> String {
    if path.is_relative() {
        return path.display().to_string();
    }

    if let Ok(stripped) = path.strip_prefix(cwd) {
        return stripped.display().to_string();
    }

    let path_in_same_repo = match (get_git_repo_root(cwd), get_git_repo_root(path)) {
        (Some(cwd_repo), Some(path_repo)) => cwd_repo == path_repo,
        _ => false,
    };
    let chosen = if path_in_same_repo {
        pathdiff::diff_paths(path, cwd).unwrap_or_else(|| path.to_path_buf())
    } else {
        relativize_to_home(path)
            .map(|p| PathBuf::from_iter([Path::new("~"), p.as_path()]))
            .unwrap_or_else(|| path.to_path_buf())
    };
    chosen.display().to_string()
}

pub(crate) fn calculate_add_remove_from_diff(diff: &str) -> (usize, usize) {
    if let Ok(patch) = diffy::Patch::from_str(diff) {
        patch
            .hunks()
            .iter()
            .flat_map(Hunk::lines)
            .fold((0, 0), |(a, d), l| match l {
                diffy::Line::Insert(_) => (a + 1, d),
                diffy::Line::Delete(_) => (a, d + 1),
                diffy::Line::Context(_) => (a, d),
            })
    } else {
        // For unparsable diffs, return 0 for both counts.
        (0, 0)
    }
}

fn push_wrapped_diff_line(
    line_number: usize,
    kind: DiffLineType,
    text: &str,
    width: usize,
    line_number_width: usize,
    diff_highlight: bool,
    syntax_highlight: bool,
    lang: Option<&str>,
    surface: DiffSurface,
) -> Vec<RtLine<'static>> {
    let ln_str = line_number.to_string();

    // Reserve a fixed number of spaces (equal to the widest line number plus a
    // trailing spacer) so the sign column stays aligned across the diff block.
    let gutter_width = line_number_width.max(1);
    let prefix_cols = gutter_width + 1;

    let (sign_char, line_style) = match kind {
        DiffLineType::Insert => (
            '+',
            diff_sign_style(style_add(diff_highlight), syntax_highlight, diff_highlight),
        ),
        DiffLineType::Delete => (
            '-',
            diff_sign_style(style_del(diff_highlight), syntax_highlight, diff_highlight),
        ),
        DiffLineType::Context => (' ', style_context(surface)),
    };
    let content_style = match kind {
        DiffLineType::Insert => diff_content_style(style_add(diff_highlight), syntax_highlight),
        DiffLineType::Delete => diff_content_style(style_del(diff_highlight), syntax_highlight),
        DiffLineType::Context => style_context(surface),
    };

    if !syntax_highlight {
        return push_wrapped_diff_text_line(
            text,
            ln_str,
            gutter_width,
            width,
            sign_char,
            line_style,
            surface,
        );
    }

    let content_width = width.saturating_sub(prefix_cols + 1).max(1);
    let highlighted = highlight_code_block_to_lines(lang, text);
    let mut out: Vec<RtLine<'static>> = Vec::new();

    for (line_idx, line) in highlighted.iter().enumerate() {
        let wrapped = word_wrap_line(line, RtOptions::new(content_width));
        for (wrapped_idx, chunk) in wrapped.iter().enumerate() {
            let is_first = line_idx == 0 && wrapped_idx == 0;
            let gutter = if is_first {
                format!("{ln_str:>gutter_width$} ")
            } else {
                format!("{:gutter_width$} ", "")
            };
            let sign = if is_first { sign_char } else { ' ' };
            let mut spans: Vec<RtSpan<'static>> = Vec::with_capacity(chunk.spans.len() + 2);
            spans.push(RtSpan::styled(
                gutter,
                diff_gutter_style(kind, syntax_highlight, diff_highlight, surface),
            ));
            spans.push(RtSpan::styled(format!("{sign}"), line_style));
            if chunk.spans.is_empty() {
                spans.push(RtSpan::styled("".to_string(), content_style));
            } else {
                for span in &chunk.spans {
                    let merged = merge_span_style(span.style, content_style);
                    spans.push(RtSpan::styled(span.content.to_string(), merged));
                }
            }
            out.push(RtLine::from(spans));
        }
    }

    if out.is_empty() {
        let gutter = format!("{ln_str:>gutter_width$} ");
        out.push(RtLine::from(vec![
            RtSpan::styled(
                gutter,
                diff_gutter_style(kind, syntax_highlight, diff_highlight, surface),
            ),
            RtSpan::styled(format!("{sign_char}"), line_style),
        ]));
    }

    out
}

fn push_wrapped_diff_text_line(
    text: &str,
    ln_str: String,
    gutter_width: usize,
    width: usize,
    sign_char: char,
    line_style: Style,
    surface: DiffSurface,
) -> Vec<RtLine<'static>> {
    let prefix_cols = gutter_width + 1;
    let mut remaining_text: &str = text;
    let mut first = true;
    let mut lines: Vec<RtLine<'static>> = Vec::new();

    loop {
        // Fit the content for the current terminal row:
        // compute how many columns are available after the prefix, then split
        // at a UTF-8 character boundary so this row's chunk fits exactly.
        let available_content_cols = width.saturating_sub(prefix_cols + 1).max(1);
        let split_at_byte_index = remaining_text
            .char_indices()
            .nth(available_content_cols)
            .map(|(i, _)| i)
            .unwrap_or_else(|| remaining_text.len());
        let (chunk, rest) = remaining_text.split_at(split_at_byte_index);
        remaining_text = rest;

        if first {
            // Build gutter (right-aligned line number plus spacer) as a dimmed span
            let gutter = format!("{ln_str:>gutter_width$} ");
            // Content with a sign ('+'/'-'/' ') styled per diff kind
            let content = format!("{sign_char}{chunk}");
            lines.push(RtLine::from(vec![
                RtSpan::styled(gutter, style_gutter(surface)),
                RtSpan::styled(content, line_style),
            ]));
            first = false;
        } else {
            // Continuation lines keep a space for the sign column so content aligns
            let gutter = format!("{:gutter_width$}  ", "");
            lines.push(RtLine::from(vec![
                RtSpan::styled(gutter, style_gutter(surface)),
                RtSpan::styled(chunk.to_string(), line_style),
            ]));
        }
        if remaining_text.is_empty() {
            break;
        }
    }
    lines
}

fn line_number_width(max_line_number: usize) -> usize {
    if max_line_number == 0 {
        1
    } else {
        max_line_number.to_string().len()
    }
}

fn surface_style(surface: DiffSurface) -> Style {
    match surface {
        DiffSurface::Transcript => crate::theme::transcript_style(),
        DiffSurface::Popup => crate::theme::composer_style(),
    }
}

fn style_gutter(surface: DiffSurface) -> Style {
    surface_style(surface).patch(crate::theme::dim_style())
}

fn style_context(surface: DiffSurface) -> Style {
    surface_style(surface)
}

fn style_add(diff_highlight: bool) -> Style {
    if diff_highlight {
        crate::theme::diff_add_highlight_style()
    } else {
        crate::theme::diff_add_text_style()
    }
}

fn style_del(diff_highlight: bool) -> Style {
    if diff_highlight {
        crate::theme::diff_del_highlight_style()
    } else {
        crate::theme::diff_del_text_style()
    }
}

fn style_hunk(diff_highlight: bool) -> Style {
    if diff_highlight {
        crate::theme::diff_hunk_highlight_style()
    } else {
        crate::theme::diff_hunk_text_style()
    }
}

fn diff_content_style(style: Style, syntax_highlight: bool) -> Style {
    if syntax_highlight {
        Style { fg: None, ..style }
    } else {
        style
    }
}

fn diff_sign_style(style: Style, syntax_highlight: bool, diff_highlight: bool) -> Style {
    if syntax_highlight && !diff_highlight {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn diff_gutter_style(
    kind: DiffLineType,
    syntax_highlight: bool,
    diff_highlight: bool,
    surface: DiffSurface,
) -> Style {
    if syntax_highlight && !diff_highlight {
        match kind {
            DiffLineType::Insert => style_add(false).add_modifier(Modifier::BOLD),
            DiffLineType::Delete => style_del(false).add_modifier(Modifier::BOLD),
            DiffLineType::Context => style_gutter(surface),
        }
    } else {
        style_gutter(surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Text;
    use ratatui::widgets::Paragraph;
    use ratatui::widgets::WidgetRef;
    use ratatui::widgets::Wrap;
    fn diff_summary_for_tests(changes: &HashMap<PathBuf, FileChange>) -> Vec<RtLine<'static>> {
        create_diff_summary(changes, &PathBuf::from("/"), 80, false, false)
    }

    fn snapshot_lines(name: &str, lines: Vec<RtLine<'static>>, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| {
                Paragraph::new(Text::from(lines))
                    .wrap(Wrap { trim: false })
                    .render_ref(f.area(), f.buffer_mut())
            })
            .expect("draw");
        assert_snapshot!(name, terminal.backend());
    }

    fn snapshot_lines_text(name: &str, lines: &[RtLine<'static>]) {
        // Convert Lines to plain text rows and trim trailing spaces so it's
        // easier to validate indentation visually in snapshots.
        let text = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .map(|s| s.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_snapshot!(name, text);
    }

    #[test]
    fn display_path_prefers_cwd_without_git_repo() {
        let cwd = if cfg!(windows) {
            PathBuf::from(r"C:\workspace\codex")
        } else {
            PathBuf::from("/workspace/codex")
        };
        let path = cwd.join("tui").join("example.png");

        let rendered = display_path_for(&path, &cwd);

        assert_eq!(
            rendered,
            PathBuf::from("tui")
                .join("example.png")
                .display()
                .to_string()
        );
    }

    #[test]
    fn ui_snapshot_wrap_behavior_insert() {
        // Narrow width to force wrapping within our diff line rendering
        let long_line = "this is a very long line that should wrap across multiple terminal columns and continue";

        // Call the wrapping function directly so we can precisely control the width
        let lines = push_wrapped_diff_line(
            1,
            DiffLineType::Insert,
            long_line,
            80,
            line_number_width(1),
            false,
            false,
            None,
            DiffSurface::Transcript,
        );

        // Render into a small terminal to capture the visual layout
        snapshot_lines("wrap_behavior_insert", lines, 90, 8);
    }

    #[test]
    fn ui_snapshot_apply_update_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "line one\nline two\nline three\n";
        let modified = "line one\nline two changed\nline three\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("example.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_update_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_with_rename_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "A\nB\nC\n";
        let modified = "A\nB changed\nC\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("old_name.rs"),
            FileChange::Update {
                unified_diff: patch,
                move_path: Some(PathBuf::from("new_name.rs")),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_update_with_rename_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_multiple_files_block() {
        // Two files: one update and one add, to exercise combined header and per-file rows
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();

        // File a.txt: single-line replacement (one delete, one insert)
        let patch_a = diffy::create_patch("one\n", "one changed\n").to_string();
        changes.insert(
            PathBuf::from("a.txt"),
            FileChange::Update {
                unified_diff: patch_a,
                move_path: None,
            },
        );

        // File b.txt: newly added with one line
        changes.insert(
            PathBuf::from("b.txt"),
            FileChange::Add {
                content: "new\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_multiple_files_block", lines, 80, 14);
    }

    #[test]
    fn ui_snapshot_apply_add_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("new_file.txt"),
            FileChange::Add {
                content: "alpha\nbeta\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);

        snapshot_lines("apply_add_block", lines, 80, 10);
    }

    #[test]
    fn ui_snapshot_apply_delete_block() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("tmp_delete_example.txt"),
            FileChange::Delete {
                content: "first\nsecond\nthird\n".to_string(),
            },
        );

        let lines = diff_summary_for_tests(&changes);
        snapshot_lines("apply_delete_block", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_block_wraps_long_lines() {
        // Create a patch with a long modified line to force wrapping
        let original = "line 1\nshort\nline 3\n";
        let modified = "line 1\nshort this_is_a_very_long_modified_line_that_should_wrap_across_multiple_terminal_columns_and_continue_even_further_beyond_eighty_columns_to_force_multiple_wraps\nline 3\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("long_example.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 72, false, false);

        // Render with backend width wider than wrap width to avoid Paragraph auto-wrap.
        snapshot_lines("apply_update_block_wraps_long_lines", lines, 80, 12);
    }

    #[test]
    fn ui_snapshot_apply_update_block_wraps_long_lines_text() {
        // This mirrors the desired layout example: sign only on first inserted line,
        // subsequent wrapped pieces start aligned under the line number gutter.
        let original = "1\n2\n3\n4\n";
        let modified = "1\nadded long line which wraps and_if_there_is_a_long_token_it_will_be_broken\n3\n4 context line which also wraps across\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("wrap_demo.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 28, false, false);
        snapshot_lines_text("apply_update_block_wraps_long_lines_text", &lines);
    }

    #[test]
    fn ui_snapshot_apply_update_block_line_numbers_three_digits_text() {
        let original = (1..=110).map(|i| format!("line {i}\n")).collect::<String>();
        let modified = (1..=110)
            .map(|i| {
                if i == 100 {
                    format!("line {i} changed\n")
                } else {
                    format!("line {i}\n")
                }
            })
            .collect::<String>();
        let patch = diffy::create_patch(&original, &modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            PathBuf::from("hundreds.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 80, false, false);
        snapshot_lines_text("apply_update_block_line_numbers_three_digits_text", &lines);
    }

    #[test]
    fn ui_snapshot_apply_update_block_relativizes_path() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let abs_old = cwd.join("abs_old.rs");
        let abs_new = cwd.join("abs_new.rs");

        let original = "X\nY\n";
        let modified = "X changed\nY\n";
        let patch = diffy::create_patch(original, modified).to_string();

        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        changes.insert(
            abs_old,
            FileChange::Update {
                unified_diff: patch,
                move_path: Some(abs_new),
            },
        );

        let lines = create_diff_summary(&changes, &cwd, 80, false, false);

        snapshot_lines("apply_update_block_relativizes_path", lines, 80, 10);
    }

    #[test]
    fn ui_snapshot_apply_update_block_side_by_side_text() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "line one\nline two\nline three\n";
        let modified = "line one\nline two changed\nline three\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("example.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 80, false, true);
        snapshot_lines_text("apply_update_block_side_by_side_text", &lines);
    }

    #[test]
    fn ui_snapshot_apply_update_block_side_by_side_narrow_text() {
        let mut changes: HashMap<PathBuf, FileChange> = HashMap::new();
        let original = "1\n2\n3\n";
        let modified = "1\n2 changed and wrapped\n3\n";
        let patch = diffy::create_patch(original, modified).to_string();

        changes.insert(
            PathBuf::from("narrow.txt"),
            FileChange::Update {
                unified_diff: patch,
                move_path: None,
            },
        );

        let lines = create_diff_summary(&changes, &PathBuf::from("/"), 20, false, true);
        snapshot_lines_text("apply_update_block_side_by_side_narrow_text", &lines);
    }

    #[test]
    fn side_by_side_syntax_rows_stay_within_width() {
        let original = "fn a() {}\n";
        let modified = "fn a() { let x = crate::theme::preview_definition(&ThemeCatalog::built_in_default()); }\n";
        let patch = diffy::create_patch(original, modified).to_string();
        let change = FileChange::Update {
            unified_diff: patch,
            move_path: None,
        };

        let width = 40usize;
        let mut lines = Vec::new();
        render_change_side_by_side(
            &change,
            &mut lines,
            width,
            false,
            true,
            Some("rust"),
            DiffSurface::Transcript,
        );

        for (idx, line) in lines.iter().enumerate() {
            assert!(
                line.width() <= width,
                "line {idx} exceeded width {} > {width}",
                line.width()
            );
        }
    }
}
