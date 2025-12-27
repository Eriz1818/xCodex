use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use async_channel::Sender;
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tracing::error;
use tracing::warn;
use uuid::Uuid;

use crate::config::HooksConfig;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::protocol::HookProcessBeginEvent;
use crate::protocol::HookProcessEndEvent;

const MAX_CONCURRENT_HOOKS: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct UserHooks {
    hooks: HooksConfig,
    codex_home: PathBuf,
    tx_event: Option<Sender<Event>>,
    semaphore: std::sync::Arc<Semaphore>,
}

impl UserHooks {
    pub(crate) fn new(
        codex_home: PathBuf,
        hooks: HooksConfig,
        tx_event: Option<Sender<Event>>,
    ) -> Self {
        Self {
            hooks,
            codex_home,
            tx_event,
            semaphore: std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_HOOKS)),
        }
    }

    pub(crate) fn agent_turn_complete(
        &self,
        thread_id: String,
        turn_id: String,
        cwd: String,
        input_messages: Vec<String>,
        last_assistant_message: Option<String>,
    ) {
        self.invoke_hook_commands(
            &self.hooks.agent_turn_complete,
            HookNotification::AgentTurnComplete {
                thread_id,
                turn_id,
                cwd,
                input_messages,
                last_assistant_message,
            },
        );
    }

    pub(crate) fn approval_requested_exec(
        &self,
        thread_id: String,
        turn_id: String,
        call_id: String,
        cwd: String,
        command: Vec<String>,
        reason: Option<String>,
    ) {
        self.invoke_hook_commands(
            &self.hooks.approval_requested,
            HookNotification::ApprovalRequested {
                thread_id,
                turn_id: Some(turn_id),
                cwd: Some(cwd),
                kind: ApprovalKind::Exec,
                call_id: Some(call_id),
                reason,
                command: Some(command),
                paths: None,
                grant_root: None,
                server_name: None,
                request_id: None,
                message: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn approval_requested_apply_patch(
        &self,
        thread_id: String,
        turn_id: String,
        call_id: String,
        cwd: String,
        paths: Vec<String>,
        reason: Option<String>,
        grant_root: Option<String>,
    ) {
        self.invoke_hook_commands(
            &self.hooks.approval_requested,
            HookNotification::ApprovalRequested {
                thread_id,
                turn_id: Some(turn_id),
                cwd: Some(cwd),
                kind: ApprovalKind::ApplyPatch,
                call_id: Some(call_id),
                reason,
                command: None,
                paths: Some(paths),
                grant_root,
                server_name: None,
                request_id: None,
                message: None,
            },
        );
    }

    pub(crate) fn approval_requested_elicitation(
        &self,
        thread_id: String,
        cwd: String,
        server_name: String,
        request_id: String,
        message: String,
    ) {
        self.invoke_hook_commands(
            &self.hooks.approval_requested,
            HookNotification::ApprovalRequested {
                thread_id,
                turn_id: None,
                cwd: Some(cwd),
                kind: ApprovalKind::Elicitation,
                call_id: None,
                reason: None,
                command: None,
                paths: None,
                grant_root: None,
                server_name: Some(server_name),
                request_id: Some(request_id),
                message: Some(message),
            },
        );
    }

    fn invoke_hook_commands(&self, commands: &[Vec<String>], notification: HookNotification) {
        if commands.is_empty() {
            return;
        }

        let payload = HookPayload::new(notification);
        let Ok(payload_json) = serde_json::to_vec(&payload) else {
            error!("failed to serialise hook payload");
            return;
        };

        let max_stdin_payload_bytes = self.hooks.max_stdin_payload_bytes;
        let keep_last_n_payloads = self.hooks.keep_last_n_payloads;
        let codex_home = self.codex_home.clone();
        let tx_event = self.tx_event.clone();
        let semaphore = self.semaphore.clone();

        for command in commands.to_vec() {
            if command.is_empty() {
                continue;
            }

            let payload = payload.clone();
            let payload_json = payload_json.clone();
            let codex_home = codex_home.clone();
            let tx_event = tx_event.clone();
            let semaphore = semaphore.clone();
            tokio::spawn(async move {
                run_hook_command(
                    command,
                    payload,
                    payload_json,
                    max_stdin_payload_bytes,
                    keep_last_n_payloads,
                    codex_home,
                    tx_event,
                    semaphore,
                )
                .await;
            });
        }
    }
}

async fn run_hook_command(
    command: Vec<String>,
    payload: HookPayload,
    payload_json: Vec<u8>,
    max_stdin_payload_bytes: usize,
    keep_last_n_payloads: usize,
    codex_home: PathBuf,
    tx_event: Option<Sender<Event>>,
    semaphore: std::sync::Arc<Semaphore>,
) {
    let _permit = semaphore.acquire().await;
    let hook_id = Uuid::new_v4();
    let event_type = payload.event_type().to_string();

    let (stdin_payload, payload_path) = if payload_json.len() <= max_stdin_payload_bytes {
        (payload_json, None)
    } else {
        match write_payload_file(&codex_home, &payload, &payload_json, keep_last_n_payloads) {
            Ok(path) => {
                let envelope = HookStdinEnvelope::from_payload(&payload, path.clone());
                match serde_json::to_vec(&envelope) {
                    Ok(envelope_json) => (envelope_json, Some(path)),
                    Err(e) => {
                        warn!("failed to serialise hook stdin envelope: {e}");
                        (payload_json, None)
                    }
                }
            }
            Err(e) => {
                warn!("failed to write hook payload file: {e}");
                (payload_json, None)
            }
        }
    };

    let (stdout, stderr) = open_hook_log_files(&codex_home, hook_id, keep_last_n_payloads);

    let child = {
        let mut cmd = tokio::process::Command::new(&command[0]);
        if command.len() > 1 {
            cmd.args(&command[1..]);
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(stdout);
        cmd.stderr(stderr);
        cmd.spawn()
    };

    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            #[allow(clippy::indexing_slicing)]
            let program = &command[0];
            warn!("failed to spawn hook '{program}': {e}");
            return;
        }
    };

    if let Some(tx_event) = &tx_event {
        let _ = tx_event
            .send(Event {
                id: "hook_process".to_string(),
                msg: EventMsg::HookProcessBegin(HookProcessBeginEvent {
                    hook_id,
                    payload_event_id: payload.event_id,
                    event_type: event_type.clone(),
                    command: command.clone(),
                }),
            })
            .await;
    }

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(&stdin_payload).await
    {
        warn!("failed to write hook payload to stdin: {e}");
    }

    let exit_code = match child.wait().await {
        Ok(status) => status.code(),
        Err(e) => {
            warn!("failed waiting for hook process to exit: {e}");
            None
        }
    };
    if let Some(code) = exit_code
        && code != 0
    {
        warn!("hook exited with non-zero status {code}: {event_type}");
    }

    if let Some(tx_event) = &tx_event {
        let _ = tx_event
            .send(Event {
                id: "hook_process".to_string(),
                msg: EventMsg::HookProcessEnd(HookProcessEndEvent { hook_id, exit_code }),
            })
            .await;
    }

    drop(payload_path);
}

fn open_hook_log_files(codex_home: &Path, hook_id: Uuid, keep_last_n: usize) -> (Stdio, Stdio) {
    let logs_dir = codex_home.join("tmp").join("hooks").join("logs");
    if let Err(e) = ensure_dir(&logs_dir) {
        warn!("failed to create hooks log dir: {e}");
        return (Stdio::null(), Stdio::null());
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let log_path = logs_dir.join(format!("{timestamp_ms}-{hook_id}.log"));
    let file = match OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(e) => {
            warn!("failed to open hook log file: {e}");
            return (Stdio::null(), Stdio::null());
        }
    };

    if let Err(e) = set_file_permissions(&log_path, &file) {
        warn!("failed to set hook log file permissions: {e}");
    }

    if let Err(e) = prune_old_files(&logs_dir, keep_last_n) {
        warn!("failed to prune hook log files: {e}");
    }

    let stderr = match file.try_clone() {
        Ok(clone) => clone,
        Err(e) => {
            warn!("failed to clone hook log file handle: {e}");
            return (Stdio::from(file), Stdio::null());
        }
    };

    (Stdio::from(file), Stdio::from(stderr))
}

fn write_payload_file(
    codex_home: &Path,
    payload: &HookPayload,
    payload_json: &[u8],
    keep_last_n: usize,
) -> anyhow::Result<PathBuf> {
    let payload_dir = codex_home.join("tmp").join("hooks").join("payloads");
    ensure_dir(&payload_dir)?;

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let event_id = payload.event_id;
    let filename = format!("{timestamp_ms}-{event_id}.json");
    let payload_path = payload_dir.join(filename);

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&payload_path)?;
    file.write_all(payload_json)?;

    set_file_permissions(&payload_path, &file)?;
    prune_old_files(&payload_dir, keep_last_n)?;

    Ok(payload_path)
}

fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    set_dir_permissions(path)?;
    Ok(())
}

fn prune_old_files(dir: &Path, keep_last_n: usize) -> anyhow::Result<()> {
    if keep_last_n == 0 {
        return Ok(());
    }

    let mut entries = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_file())
        .collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    if entries.len() <= keep_last_n {
        return Ok(());
    }

    let to_delete = entries.len().saturating_sub(keep_last_n);
    for entry in entries.into_iter().take(to_delete) {
        let _ = std::fs::remove_file(entry.path());
    }

    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn set_file_permissions(path: &Path, _file: &File) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
struct HookPayload {
    schema_version: u32,
    event_id: Uuid,
    timestamp: DateTime<Utc>,
    #[serde(flatten)]
    notification: HookNotification,
}

impl HookPayload {
    fn new(notification: HookNotification) -> Self {
        Self {
            schema_version: 1,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            notification,
        }
    }

    fn event_type(&self) -> &'static str {
        self.notification.event_type()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
struct HookStdinEnvelope {
    schema_version: u32,
    event_id: Uuid,
    timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    event_type: &'static str,
    payload_path: String,
}

impl HookStdinEnvelope {
    fn from_payload(payload: &HookPayload, payload_path: PathBuf) -> Self {
        Self {
            schema_version: payload.schema_version,
            event_id: payload.event_id,
            timestamp: payload.timestamp,
            event_type: payload.event_type(),
            payload_path: payload_path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ApprovalKind {
    Exec,
    ApplyPatch,
    Elicitation,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum HookNotification {
    #[serde(rename_all = "kebab-case")]
    AgentTurnComplete {
        thread_id: String,
        turn_id: String,
        cwd: String,

        input_messages: Vec<String>,
        last_assistant_message: Option<String>,
    },

    #[serde(rename_all = "kebab-case")]
    ApprovalRequested {
        thread_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,

        kind: ApprovalKind,

        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        grant_root: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        server_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl HookNotification {
    fn event_type(&self) -> &'static str {
        match self {
            Self::AgentTurnComplete { .. } => "agent-turn-complete",
            Self::ApprovalRequested { .. } => "approval-requested",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_hook_payload_includes_version_and_ids() -> Result<()> {
        let payload = HookPayload::new(HookNotification::ApprovalRequested {
            thread_id: "thread-1".to_string(),
            turn_id: None,
            cwd: Some("/tmp".to_string()),
            kind: ApprovalKind::Exec,
            call_id: Some("call-1".to_string()),
            reason: None,
            command: Some(vec!["echo".to_string(), "hi".to_string()]),
            paths: None,
            grant_root: None,
            server_name: None,
            request_id: None,
            message: None,
        });
        let serialized = serde_json::to_string(&payload)?;
        assert!(
            serialized.contains(r#""schema-version":1"#),
            "payload must include schema-version: {serialized}"
        );
        assert!(
            serialized.contains(r#""event-id":"#),
            "payload must include event-id: {serialized}"
        );
        assert!(
            serialized.contains(r#""timestamp":"#),
            "payload must include timestamp: {serialized}"
        );
        Ok(())
    }

    #[test]
    fn test_hook_stdin_envelope_has_payload_path() -> Result<()> {
        let payload = HookPayload::new(HookNotification::AgentTurnComplete {
            thread_id: "t".to_string(),
            turn_id: "turn".to_string(),
            cwd: "/tmp".to_string(),
            input_messages: Vec::new(),
            last_assistant_message: None,
        });
        let envelope =
            HookStdinEnvelope::from_payload(&payload, PathBuf::from("/tmp/payload.json"));
        let serialized = serde_json::to_string(&envelope)?;
        assert!(
            serialized.contains(r#""payload-path":"/tmp/payload.json""#),
            "envelope must include payload-path: {serialized}"
        );
        Ok(())
    }
}
