use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

/// Read the first markdown H1 title (`# ...`) from a plan file.
pub fn read_title(path: &Path) -> io::Result<Option<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim().to_string()))
}

/// Read the `Status:` metadata field from a plan file.
pub fn read_status(path: &Path) -> io::Result<Option<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .find(|line| line.trim_start().starts_with("Status:"))
        .map(|line| {
            line.trim_start()
                .trim_start_matches("Status:")
                .trim()
                .to_string()
        }))
}

/// Count unchecked markdown checkboxes (`- [ ]`) while ignoring fenced code blocks.
pub fn count_unchecked_todos(path: &Path) -> io::Result<usize> {
    let content = fs::read_to_string(path)?;
    Ok(count_unchecked_todos_in_text(&content))
}

/// Update `Status:` and `Last updated:` metadata using an atomic file replace.
pub fn set_status(path: &Path, status: &str, today: &str) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let updated = update_status_and_last_updated(&content, status, today);
    write_atomic(path, &updated)
}

/// On assistant turn completion, update plan metadata and append a checkpoint line.
///
/// This updates:
/// - `Status:` (`Draft`/`Paused` -> `Active`)
/// - `TODOs remaining:`
/// - `Last updated:`
/// - `## Progress log` section (append `- YYYY-MM-DD: ...`)
///
/// If status is `Done` or `Archived`, this is a no-op.
pub fn sync_turn_end(path: &Path, today: &str, note: &str) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let current_status = read_status_from_text(&content)
        .unwrap_or_else(|| "Draft".to_string())
        .to_ascii_lowercase();
    if matches!(current_status.as_str(), "done" | "archived") {
        return Ok(());
    }

    let status = "Active";
    let todos = count_unchecked_todos_in_text(&content);
    let updated_with_metadata = update_turn_metadata(&content, status, todos, today);
    let note = normalize_progress_note(note);
    let updated = append_progress_log_entry(&updated_with_metadata, today, &note);
    write_atomic(path, &updated)
}

/// ADR-lite turn-end sync with stricter hygiene and context tracking.
///
/// In addition to `sync_turn_end`, this:
/// - ensures key ADR-lite sections exist
/// - ensures `Worktree:` and `Branch:` metadata are present and current
/// - appends one progress-log line that includes hygiene/context notes when relevant
pub fn sync_turn_end_adr_lite(
    path: &Path,
    today: &str,
    note: &str,
    worktree: &str,
    branch: &str,
) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let current_status = read_status_from_text(&content)
        .unwrap_or_else(|| "Draft".to_string())
        .to_ascii_lowercase();
    if matches!(current_status.as_str(), "done" | "archived") {
        return Ok(());
    }

    let status = "Active";
    let mut updated = update_turn_metadata(
        &content,
        status,
        count_unchecked_todos_in_text(&content),
        today,
    );
    let mut inserted_sections: Vec<&'static str> = Vec::new();
    updated = ensure_adr_lite_sections(&updated, &mut inserted_sections);
    let context_changed = upsert_context_fields(&mut updated, worktree, branch);
    let todos = count_unchecked_todos_in_text(&updated);
    updated = update_turn_metadata(&updated, status, todos, today);

    let mut final_note = normalize_progress_note(note);
    if !inserted_sections.is_empty() {
        final_note.push_str(" ADR-lite hygiene: added missing sections: ");
        final_note.push_str(&inserted_sections.join(", "));
        final_note.push('.');
    }
    if context_changed {
        final_note.push_str(&format!(
            " Context updated: Worktree=`{worktree}`, Branch=`{branch}`."
        ));
    }

    let updated = append_progress_log_entry(&updated, today, &final_note);
    write_atomic(path, &updated)
}

/// ADR-lite open/resume sync that preserves status while ensuring structure/context.
pub fn sync_adr_lite_open_or_resume(
    path: &Path,
    today: &str,
    worktree: &str,
    branch: &str,
    reason: &str,
) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let current_status = read_status_from_text(&content)
        .unwrap_or_else(|| "Draft".to_string())
        .to_ascii_lowercase();
    if matches!(current_status.as_str(), "done" | "archived") {
        return Ok(());
    }

    let mut updated = content;
    let mut inserted_sections: Vec<&'static str> = Vec::new();
    updated = ensure_adr_lite_sections(&updated, &mut inserted_sections);
    let context_changed = upsert_context_fields(&mut updated, worktree, branch);
    let status = read_status_from_text(&updated).unwrap_or_else(|| "Draft".to_string());
    let todos = count_unchecked_todos_in_text(&updated);
    updated = update_turn_metadata(&updated, &status, todos, today);

    let mut notes: Vec<String> = Vec::new();
    if !inserted_sections.is_empty() {
        notes.push(format!(
            "ADR-lite hygiene: added missing sections: {}.",
            inserted_sections.join(", ")
        ));
    }
    if context_changed {
        notes.push(format!(
            "Context updated: Worktree=`{worktree}`, Branch=`{branch}`."
        ));
    }
    if !notes.is_empty() {
        let mut entry = format!("ADR-lite {reason}: ");
        entry.push_str(&notes.join(" "));
        updated = append_progress_log_entry(&updated, today, &entry);
    }

    write_atomic(path, &updated)
}

/// Atomically replace `path` with `content` via temp-file + rename in the target directory.
pub fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path)
        .map(|_| ())
        .map_err(|err| io::Error::new(err.error.kind(), err.error))
}

fn read_status_from_text(content: &str) -> Option<String> {
    content
        .lines()
        .find(|line| line.trim_start().starts_with("Status:"))
        .map(|line| {
            line.trim_start()
                .trim_start_matches("Status:")
                .trim()
                .to_string()
        })
}

fn update_status_and_last_updated(content: &str, status: &str, today: &str) -> String {
    let mut replaced_status = false;
    let mut replaced_last_updated = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if !replaced_status && line.trim_start().starts_with("Status:") {
            lines.push(format!("Status: {status}"));
            replaced_status = true;
            continue;
        }
        if !replaced_last_updated && line.trim_start().starts_with("Last updated:") {
            lines.push(format!("Last updated: {today}"));
            replaced_last_updated = true;
            continue;
        }
        lines.push(line.to_string());
    }

    let insert_index = lines
        .iter()
        .position(|line| line.starts_with("# "))
        .map_or(0, |idx| idx + 1);
    if !replaced_status {
        lines.insert(insert_index, format!("Status: {status}"));
    }
    if !replaced_last_updated {
        let after_status_index = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Status:"))
            .map_or(insert_index + 1, |idx| idx + 1);
        lines.insert(after_status_index, format!("Last updated: {today}"));
    }

    ensure_trailing_newline(lines.join("\n"))
}

fn update_turn_metadata(content: &str, status: &str, todos: usize, today: &str) -> String {
    let mut replaced_status = false;
    let mut replaced_todos = false;
    let mut replaced_last_updated = false;
    let mut lines: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !replaced_status && trimmed.starts_with("Status:") {
            lines.push(format!("Status: {status}"));
            replaced_status = true;
            continue;
        }
        if !replaced_todos && trimmed.starts_with("TODOs remaining:") {
            lines.push(format!("TODOs remaining: {todos}"));
            replaced_todos = true;
            continue;
        }
        if !replaced_last_updated && trimmed.starts_with("Last updated:") {
            lines.push(format!("Last updated: {today}"));
            replaced_last_updated = true;
            continue;
        }
        lines.push(line.to_string());
    }

    let insert_index = lines
        .iter()
        .position(|line| line.starts_with("# "))
        .map_or(0, |idx| idx + 1);
    if !replaced_status {
        lines.insert(insert_index, format!("Status: {status}"));
    }
    if !replaced_todos {
        let after_status_index = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Status:"))
            .map_or(insert_index + 1, |idx| idx + 1);
        lines.insert(after_status_index, format!("TODOs remaining: {todos}"));
    }
    if !replaced_last_updated {
        let after_todos_index = lines
            .iter()
            .position(|line| line.trim_start().starts_with("TODOs remaining:"))
            .map_or(insert_index + 1, |idx| idx + 1);
        lines.insert(after_todos_index, format!("Last updated: {today}"));
    }

    ensure_trailing_newline(lines.join("\n"))
}

fn normalize_progress_note(note: &str) -> String {
    let normalized = note
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        "Completed assistant turn.".to_string()
    } else {
        normalized
    }
}

fn append_progress_log_entry(content: &str, today: &str, note: &str) -> String {
    let entry = format!("- {today}: {note}");
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    if let Some(progress_header_index) = lines
        .iter()
        .position(|line| line.trim() == "## Progress log")
    {
        let section_end = lines
            .iter()
            .enumerate()
            .skip(progress_header_index + 1)
            .find(|(_, line)| line.starts_with("## "))
            .map_or(lines.len(), |(idx, _)| idx);

        let last_non_empty = lines
            .iter()
            .take(section_end)
            .skip(progress_header_index + 1)
            .rfind(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string());
        if last_non_empty.as_deref() == Some(entry.as_str()) {
            return ensure_trailing_newline(lines.join("\n"));
        }

        let mut insert_index = section_end;
        while insert_index > progress_header_index + 1 && lines[insert_index - 1].trim().is_empty()
        {
            insert_index = insert_index.saturating_sub(1);
        }
        if insert_index == progress_header_index + 1 {
            lines.insert(insert_index, String::new());
            insert_index += 1;
        }
        lines.insert(insert_index, entry);
        return ensure_trailing_newline(lines.join("\n"));
    }

    let mut out = content.trim_end_matches('\n').to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Progress log\n\n");
    out.push_str(&entry);
    out.push('\n');
    out
}

fn ensure_adr_lite_sections(content: &str, inserted_sections: &mut Vec<&'static str>) -> String {
    let mut out = content.trim_end_matches('\n').to_string();
    let required = [
        ("## Decisions", "## Decisions\n\n- None yet.\n", "Decisions"),
        (
            "## Open questions",
            "## Open questions\n\n- None currently.\n",
            "Open questions",
        ),
        (
            "## Acceptance criteria / verification",
            "## Acceptance criteria / verification\n\n- [ ] Define acceptance criteria.\n",
            "Acceptance criteria / verification",
        ),
        ("## Progress log", "## Progress log\n\n", "Progress log"),
        ("## Learnings", "## Learnings\n\n- None yet.\n", "Learnings"),
        ("## Memories", "## Memories\n\n- None yet.\n", "Memories"),
    ];

    for (header, block, label) in required {
        if out.lines().any(|line| line.trim() == header) {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(block.trim_end_matches('\n'));
        inserted_sections.push(label);
    }

    ensure_trailing_newline(out)
}

fn read_metadata_value(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(key).map(str::trim))
        .map(ToString::to_string)
}

fn upsert_context_fields(content: &mut String, worktree: &str, branch: &str) -> bool {
    let old_worktree = read_metadata_value(content, "Worktree:");
    let old_branch = read_metadata_value(content, "Branch:");

    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    let mut replaced_worktree = false;
    let mut replaced_branch = false;

    for line in &mut lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Worktree:") {
            *line = format!("Worktree: {worktree}");
            replaced_worktree = true;
            continue;
        }
        if trimmed.starts_with("Branch:") {
            *line = format!("Branch: {branch}");
            replaced_branch = true;
        }
    }

    if !replaced_worktree {
        let insert_idx = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Last updated:"))
            .map_or(1, |idx| idx + 1);
        lines.insert(insert_idx, format!("Worktree: {worktree}"));
    }
    if !replaced_branch {
        let insert_idx = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Worktree:"))
            .map_or(1, |idx| idx + 1);
        lines.insert(insert_idx, format!("Branch: {branch}"));
    }

    *content = ensure_trailing_newline(lines.join("\n"));
    old_worktree.as_deref() != Some(worktree) || old_branch.as_deref() != Some(branch)
}

fn ensure_trailing_newline(mut content: String) -> String {
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

fn count_unchecked_todos_in_text(content: &str) -> usize {
    let mut in_fenced_block = false;
    let mut unchecked = 0usize;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if is_fence_delimiter(trimmed) {
            in_fenced_block = !in_fenced_block;
            continue;
        }
        if in_fenced_block {
            continue;
        }
        if trimmed.starts_with("- [ ]") {
            unchecked = unchecked.saturating_add(1);
        }
    }
    unchecked
}

fn is_fence_delimiter(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn count_unchecked_todos_ignores_fenced_blocks() {
        let input = r#"# Plan

Status: Draft

- [ ] top level

```md
- [ ] ignored in code fence
```

  - [ ] nested
~~~text
- [ ] ignored in tilde fence
~~~
"#;
        assert_eq!(count_unchecked_todos_in_text(input), 2);
    }

    #[test]
    fn set_status_updates_existing_metadata() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "# Plan\nStatus: Draft\nLast updated: 2026-01-01\n\n- [ ] item\n",
        )
        .expect("write plan");

        set_status(&path, "Done", "2026-02-08").expect("set status");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert_eq!(
            updated,
            "# Plan\nStatus: Done\nLast updated: 2026-02-08\n\n- [ ] item\n"
        );
    }

    #[test]
    fn set_status_inserts_missing_metadata_near_header() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(&path, "# Plan\n\n## Goal\n- TODO\n").expect("write plan");

        set_status(&path, "Active", "2026-02-08").expect("set status");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert_eq!(
            updated,
            "# Plan\nStatus: Active\nLast updated: 2026-02-08\n\n## Goal\n- TODO\n"
        );
    }

    #[test]
    fn read_title_and_status_extract_expected_values() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(&path, "# Shipping Plan\nStatus: Paused\n").expect("write plan");

        assert_eq!(
            read_title(&path).expect("title"),
            Some("Shipping Plan".to_string())
        );
        assert_eq!(
            read_status(&path).expect("status"),
            Some("Paused".to_string())
        );
    }

    #[test]
    fn sync_turn_end_updates_metadata_and_appends_progress() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "# Plan\nStatus: Draft\nTODOs remaining: 999\nLast updated: 2026-01-01\n\n## Plan (checklist)\n- [ ] A\n- [x] B\n\n## Progress log\n\n- 2026-02-07: Created\n",
        )
        .expect("write plan");

        sync_turn_end(&path, "2026-02-08", "Implemented first slice").expect("sync");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert_eq!(
            updated,
            "# Plan\nStatus: Active\nTODOs remaining: 1\nLast updated: 2026-02-08\n\n## Plan (checklist)\n- [ ] A\n- [x] B\n\n## Progress log\n\n- 2026-02-07: Created\n- 2026-02-08: Implemented first slice\n"
        );
    }

    #[test]
    fn sync_turn_end_adds_progress_section_when_missing() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "# Plan\nStatus: Active\n\n## Plan (checklist)\n- [ ] A\n",
        )
        .expect("write plan");

        sync_turn_end(&path, "2026-02-08", "Checkpoint").expect("sync");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert_eq!(
            updated,
            "# Plan\nStatus: Active\nTODOs remaining: 1\nLast updated: 2026-02-08\n\n## Plan (checklist)\n- [ ] A\n\n## Progress log\n\n- 2026-02-08: Checkpoint\n"
        );
    }

    #[test]
    fn sync_turn_end_is_noop_for_done_plan() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        let initial = "# Plan\nStatus: Done\nTODOs remaining: 0\nLast updated: 2026-02-07\n\n## Progress log\n\n- 2026-02-07: Done.\n";
        fs::write(&path, initial).expect("write plan");

        sync_turn_end(&path, "2026-02-08", "Should not append").expect("sync");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert_eq!(updated, initial);
    }

    #[test]
    fn sync_turn_end_adr_lite_adds_sections_and_updates_context() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "# Plan\nStatus: Draft\nTODOs remaining: 999\nLast updated: 2026-01-01\nWorktree: /old\nBranch: old-branch\n\n## Plan (checklist)\n- [ ] A\n",
        )
        .expect("write plan");

        sync_turn_end_adr_lite(
            &path,
            "2026-02-09",
            "Checkpoint",
            "/repo/worktree",
            "feat/new",
        )
        .expect("sync");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert!(updated.contains("Status: Active"));
        assert!(updated.contains("TODOs remaining: 2"));
        assert!(updated.contains("Worktree: /repo/worktree"));
        assert!(updated.contains("Branch: feat/new"));
        assert!(updated.contains("## Decisions"));
        assert!(updated.contains("## Open questions"));
        assert!(updated.contains("## Acceptance criteria / verification"));
        assert!(updated.contains("## Learnings"));
        assert!(updated.contains("## Memories"));
        assert!(updated.contains("ADR-lite hygiene: added missing sections:"));
        assert!(updated.contains("Context updated: Worktree=`/repo/worktree`, Branch=`feat/new`."));
    }

    #[test]
    fn sync_adr_lite_open_or_resume_preserves_status_and_adds_context() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("plan.md");
        fs::write(
            &path,
            "# Plan\nStatus: Paused\nTODOs remaining: 1\nLast updated: 2026-01-01\n\n## Plan (checklist)\n- [ ] A\n\n## Progress log\n\n- 2026-01-01: Created.\n",
        )
        .expect("write plan");

        sync_adr_lite_open_or_resume(
            &path,
            "2026-02-09",
            "/repo/worktree",
            "feat/resume",
            "session recovery sync",
        )
        .expect("sync");
        let updated = fs::read_to_string(&path).expect("read plan");
        assert!(updated.contains("Status: Paused"));
        assert!(updated.contains("TODOs remaining: 2"));
        assert!(updated.contains("Worktree: /repo/worktree"));
        assert!(updated.contains("Branch: feat/resume"));
        assert!(updated.contains("ADR-lite session recovery sync:"));
    }
}
