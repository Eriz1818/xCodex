use crate::config::types::LogRedactionsMode;
use crate::content_gateway::RedactionReason;
use crate::content_gateway::ScanReport;
use crate::exclusion_counters::ExclusionLayer;
use crate::exclusion_counters::ExclusionSource;
use anyhow::Result;
use once_cell::sync::Lazy;
use serde_json::json;
use std::cmp;
use std::fs;
use std::fs::OpenOptions;
use std::fs::create_dir_all;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const LOG_DIR_NAME: &str = "log";
const LOG_FILE_NAME: &str = "exclusion-redactions.jsonl";
const LOG_FLUSH_MAX_ENTRIES: usize = 100;
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
const CONTEXT_WINDOW_LINES: usize = 5;

static LOG_QUEUE: Lazy<std::sync::Mutex<LogQueue>> = Lazy::new(|| {
    std::sync::Mutex::new(LogQueue {
        buffer: Vec::new(),
        last_flush: Instant::now(),
    })
});

struct LogQueue {
    buffer: Vec<String>,
    last_flush: Instant,
}

pub(crate) struct RedactionLogContext<'a> {
    pub codex_home: &'a Path,
    pub layer: ExclusionLayer,
    pub source: ExclusionSource,
    pub tool_name: &'a str,
    pub origin_type: &'a str,
    pub origin_path: Option<&'a str>,
    pub log_mode: LogRedactionsMode,
    pub max_bytes: u64,
    pub max_files: usize,
}

pub(crate) fn log_redaction_event(
    ctx: &RedactionLogContext<'_>,
    report: &ScanReport,
    original: &str,
    sanitized: &str,
) {
    if ctx.log_mode == LogRedactionsMode::Off {
        return;
    }
    if !report.redacted && !report.blocked {
        return;
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let (context_start_line, match_line, original_context, sanitized_context) =
        build_context_window(original, sanitized, CONTEXT_WINDOW_LINES);
    let reasons = report
        .reasons
        .iter()
        .map(|reason| reason_label(*reason))
        .collect::<Vec<_>>();
    let entry = match ctx.log_mode {
        LogRedactionsMode::Summary => json!({
            "timestamp_ms": timestamp_ms,
            "layer": layer_label(ctx.layer),
            "source": source_label(ctx.source),
            "tool_name": ctx.tool_name,
            "redacted": report.redacted,
            "blocked": report.blocked,
            "origin_type": ctx.origin_type,
            "origin_path": ctx.origin_path,
            "reasons": reasons,
            "context_start_line": context_start_line,
            "match_line": match_line,
            "context_sanitized": sanitized_context,
        }),
        LogRedactionsMode::Raw => json!({
            "timestamp_ms": timestamp_ms,
            "layer": layer_label(ctx.layer),
            "source": source_label(ctx.source),
            "tool_name": ctx.tool_name,
            "redacted": report.redacted,
            "blocked": report.blocked,
            "origin_type": ctx.origin_type,
            "origin_path": ctx.origin_path,
            "reasons": reasons,
            "context_start_line": context_start_line,
            "match_line": match_line,
            "context_original": original_context,
            "context_sanitized": sanitized_context,
        }),
        LogRedactionsMode::Off => return,
    };
    let line = match serde_json::to_string(&entry) {
        Ok(line) => line,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "failed to serialize exclusion log entry",
            );
            return;
        }
    };

    let mut queue = LOG_QUEUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.buffer.push(line);
    let should_flush = queue.buffer.len() >= LOG_FLUSH_MAX_ENTRIES
        || queue.last_flush.elapsed() >= LOG_FLUSH_INTERVAL;
    if !should_flush {
        return;
    }
    let mut pending = Vec::new();
    std::mem::swap(&mut pending, &mut queue.buffer);
    queue.last_flush = Instant::now();
    drop(queue);
    if let Err(err) = flush_entries(ctx, &pending) {
        tracing::warn!(error = %err, "failed to flush exclusion log entries");
    }
}

fn flush_entries(ctx: &RedactionLogContext<'_>, entries: &[String]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let log_dir = ctx.codex_home.join(LOG_DIR_NAME);
    ensure_dir(&log_dir)?;
    let log_path = log_dir.join(LOG_FILE_NAME);
    rotate_logs_if_needed(&log_path, ctx.max_bytes, ctx.max_files, entries)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    set_file_permissions(&log_path)?;
    for line in entries {
        writeln!(file, "{line}")?;
    }
    Ok(())
}

fn rotate_logs_if_needed(
    log_path: &Path,
    max_bytes: u64,
    max_files: usize,
    pending: &[String],
) -> Result<()> {
    if max_bytes == 0 {
        return Ok(());
    }
    let max_files = max_files.max(1);
    let pending_bytes: u64 = pending.iter().map(|line| line.len() as u64 + 1).sum();
    let current_size = fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
    if current_size.saturating_add(pending_bytes) <= max_bytes {
        return Ok(());
    }
    if max_files == 1 {
        let _ = fs::remove_file(log_path);
        return Ok(());
    }
    for idx in (1..max_files).rev() {
        let src = rotated_path(log_path, idx - 1);
        let dst = rotated_path(log_path, idx);
        if src.exists() {
            let _ = fs::remove_file(&dst);
            let _ = fs::rename(&src, &dst);
        }
    }
    Ok(())
}

fn rotated_path(log_path: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return log_path.to_path_buf();
    }
    let ext = log_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("jsonl");
    let mut next = log_path.to_path_buf();
    next.set_extension(format!("{ext}.{index}"));
    next
}

fn build_context_window(
    original: &str,
    sanitized: &str,
    window: usize,
) -> (usize, usize, Vec<String>, Vec<String>) {
    let original_lines = split_lines(original);
    let sanitized_lines = split_lines(sanitized);
    let diff_index = first_diff_line(&original_lines, &sanitized_lines);
    let start = diff_index.saturating_sub(window);
    let end = cmp::min(
        cmp::max(original_lines.len(), sanitized_lines.len()),
        diff_index.saturating_add(window + 1),
    );
    let original_context = slice_lines(&original_lines, start, end);
    let sanitized_context = slice_lines(&sanitized_lines, start, end);
    (
        start + 1,
        diff_index + 1,
        original_context,
        sanitized_context,
    )
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(std::string::ToString::to_string)
        .collect()
}

fn first_diff_line(original: &[String], sanitized: &[String]) -> usize {
    let min_len = cmp::min(original.len(), sanitized.len());
    for idx in 0..min_len {
        if original[idx] != sanitized[idx] {
            return idx;
        }
    }
    min_len
}

fn slice_lines(lines: &[String], start: usize, end: usize) -> Vec<String> {
    if start >= lines.len() || start >= end {
        return Vec::new();
    }
    lines[start..cmp::min(end, lines.len())].to_vec()
}

fn ensure_dir(path: &Path) -> Result<()> {
    create_dir_all(path)?;
    set_dir_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn layer_label(layer: ExclusionLayer) -> &'static str {
    match layer {
        ExclusionLayer::Layer1InputGuards => "layer1_input_guards",
        ExclusionLayer::Layer2OutputSanitization => "layer2_output_sanitization",
        ExclusionLayer::Layer3SendFirewall => "layer3_send_firewall",
        ExclusionLayer::Layer4RequestInterceptor => "layer4_request_interceptor",
    }
}

fn source_label(source: ExclusionSource) -> &'static str {
    match source {
        ExclusionSource::Filesystem => "filesystem",
        ExclusionSource::Mcp => "mcp",
        ExclusionSource::Shell => "shell",
        ExclusionSource::Prompt => "prompt",
        ExclusionSource::Other => "other",
    }
}

fn reason_label(reason: RedactionReason) -> &'static str {
    match reason {
        RedactionReason::FingerprintCache => "fingerprint_cache",
        RedactionReason::IgnoredPath => "ignored_path",
        RedactionReason::SecretPattern => "secret_pattern",
    }
}
