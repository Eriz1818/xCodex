use std::collections::BTreeSet;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use codex_core::protocol::McpStartupCompleteEvent;
use codex_core::protocol::McpStartupFailure;
use codex_core::protocol::McpStartupStatus;
use codex_core::protocol::McpStartupUpdateEvent;

#[derive(Default)]
pub(crate) struct McpStartupState {
    started_at: Option<Instant>,
    ready_duration: Option<Duration>,
    status: Option<HashMap<String, McpStartupStatus>>,
    failed_servers: Vec<String>,
    server_start_times: HashMap<String, Instant>,
    startup_durations: HashMap<String, Duration>,
}

pub(crate) struct McpStartupCompleteOutcome {
    pub(crate) banner: Option<String>,
    pub(crate) warning: Option<String>,
}

impl McpStartupState {
    pub(crate) fn reset_for_session(&mut self) {
        self.started_at = Some(Instant::now());
        self.ready_duration = None;
        self.status = None;
        self.failed_servers.clear();
        self.server_start_times.clear();
        self.startup_durations.clear();
    }

    pub(crate) fn status(&self) -> Option<&HashMap<String, McpStartupStatus>> {
        self.status.as_ref()
    }

    pub(crate) fn ready_duration(&self) -> Option<Duration> {
        self.ready_duration
    }

    pub(crate) fn startup_durations(&self) -> &HashMap<String, Duration> {
        &self.startup_durations
    }

    pub(crate) fn failed_servers(&self) -> &[String] {
        &self.failed_servers
    }

    pub(crate) fn take_failed_servers(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failed_servers)
    }

    pub(crate) fn on_update(&mut self, ev: McpStartupUpdateEvent) -> Option<String> {
        let mut status = self.status.take().unwrap_or_default();
        let now = Instant::now();
        let server = ev.server;
        let state = ev.status;
        match &state {
            McpStartupStatus::Starting => {
                self.server_start_times.insert(server.clone(), now);
                self.startup_durations.remove(&server);
            }
            McpStartupStatus::Ready
            | McpStartupStatus::Cancelled
            | McpStartupStatus::Failed { .. } => {
                if let Some(started_at) = self.server_start_times.remove(&server) {
                    self.startup_durations
                        .insert(server.clone(), now.saturating_duration_since(started_at));
                }
            }
        }
        status.insert(server, state);
        self.status = Some(status);
        self.status_header()
    }

    pub(crate) fn on_complete(
        &mut self,
        ev: McpStartupCompleteEvent,
        can_retry_in_place: bool,
    ) -> McpStartupCompleteOutcome {
        let now = Instant::now();
        self.ready_duration = self
            .started_at
            .map(|started_at| now.saturating_duration_since(started_at));

        let mut retryable: BTreeSet<String> = self.failed_servers.drain(..).collect();
        for server in &ev.ready {
            retryable.remove(server);
        }
        for failure in &ev.failed {
            retryable.insert(failure.server.clone());
        }
        for server in &ev.cancelled {
            retryable.insert(server.clone());
        }
        self.failed_servers = retryable.into_iter().collect();

        for server in &ev.ready {
            self.record_completion_if_missing(server, now);
        }
        for failure in &ev.failed {
            self.record_completion_if_missing(&failure.server, now);
        }
        for server in &ev.cancelled {
            self.record_completion_if_missing(server, now);
        }

        self.status = None;

        if self.failed_servers.is_empty() {
            return McpStartupCompleteOutcome {
                banner: None,
                warning: None,
            };
        }

        let message = Self::startup_failure_message(&ev.failed, &ev.cancelled);
        if can_retry_in_place {
            McpStartupCompleteOutcome {
                banner: Some(message),
                warning: None,
            }
        } else {
            McpStartupCompleteOutcome {
                banner: None,
                warning: Some(message),
            }
        }
    }

    fn record_completion_if_missing(&mut self, server: &str, now: Instant) {
        if self.startup_durations.contains_key(server) {
            return;
        }
        if let Some(started_at) = self.server_start_times.remove(server) {
            self.startup_durations.insert(
                server.to_string(),
                now.saturating_duration_since(started_at),
            );
        }
    }

    fn status_header(&self) -> Option<String> {
        let current = self.status.as_ref()?;
        let total = current.len();
        let mut starting: Vec<_> = current
            .iter()
            .filter_map(|(name, state)| {
                if matches!(state, McpStartupStatus::Starting) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        starting.sort();
        let first = starting.first()?;
        let completed = total.saturating_sub(starting.len());
        let max_to_show = 3;
        let mut to_show: Vec<String> = starting
            .iter()
            .take(max_to_show)
            .map(ToString::to_string)
            .collect();
        if starting.len() > max_to_show {
            to_show.push("…".to_string());
        }
        if total > 1 {
            Some(format!(
                "Starting MCP servers ({completed}/{total}): {}",
                to_show.join(", ")
            ))
        } else {
            Some(format!("Booting MCP server: {first}"))
        }
    }

    fn startup_failure_message(failures: &[McpStartupFailure], cancelled: &[String]) -> String {
        let mut parts = Vec::new();
        if !failures.is_empty() {
            let failed_servers: Vec<_> = failures.iter().map(|f| f.server.as_str()).collect();
            let failed = failed_servers.join(", ");
            parts.push(format!("failed: {failed}"));
        }
        if !cancelled.is_empty() {
            let cancelled = cancelled.join(", ");
            parts.push(format!("not initialized: {cancelled}"));
        }
        let mut message = if parts.is_empty() {
            "MCP startup incomplete.".to_string()
        } else {
            let summary = parts.join("; ");
            format!("MCP startup incomplete ({summary}).")
        };
        message.push_str(" Press `r` or run `/mcp retry failed` to retry.");
        if let Some(tip) = Self::timeout_tip_for_failures(failures) {
            message.push(' ');
            message.push_str(&tip);
        }
        message.push_str(" Run `/mcp` for details.");
        message
    }

    fn timeout_tip_for_failures(failures: &[McpStartupFailure]) -> Option<String> {
        failures
            .iter()
            .find_map(|failure| Self::timeout_tip_for_failure(&failure.server, &failure.error))
    }

    fn timeout_tip_for_failure(server: &str, error: &str) -> Option<String> {
        let secs = Self::parse_timeout_seconds(error);
        if secs.is_none() && !error.to_ascii_lowercase().contains("timed out") {
            return None;
        }

        let suggested = secs.map_or(30, |secs| std::cmp::max(30, secs.saturating_mul(3)));
        Some(format!(
            "Tip: increase startup timeout: `/mcp timeout {server} {suggested}`."
        ))
    }

    fn parse_timeout_seconds(error: &str) -> Option<u64> {
        let lower = error.to_ascii_lowercase();
        if !lower.contains("timed out") {
            return None;
        }

        let after_idx = lower.rfind("after ")?;
        let mut digits = String::new();
        for ch in lower[after_idx + "after ".len()..].chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if !digits.is_empty() {
                break;
            }
        }
        if digits.is_empty() {
            return None;
        }
        digits.parse().ok()
    }
}

#[cfg(test)]
impl McpStartupState {
    pub(crate) fn set_status(&mut self, status: HashMap<String, McpStartupStatus>) {
        self.status = Some(status);
    }

    pub(crate) fn set_startup_durations(&mut self, durations: HashMap<String, Duration>) {
        self.startup_durations = durations;
    }

    pub(crate) fn set_ready_duration(&mut self, duration: Option<Duration>) {
        self.ready_duration = duration;
    }
}
