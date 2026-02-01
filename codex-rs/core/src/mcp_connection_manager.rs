//! Connection manager for Model Context Protocol (MCP) servers.
//!
//! The [`McpConnectionManager`] owns one [`codex_rmcp_client::RmcpClient`] per
//! configured server (keyed by the *server name*). It offers convenience
//! helpers to query the available tools across *all* servers and returns them
//! in a single aggregated map using the fully-qualified tool name
//! `"<server><MCP_TOOL_NAME_DELIMITER><tool>"` as the key.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::hooks::UserHooks;
use crate::mcp::CODEX_APPS_MCP_SERVER_NAME;
use crate::mcp::auth::McpAuthStatusEntry;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_channel::Sender;
use codex_async_utils::CancelErr;
use codex_async_utils::OrCancelExt;
use codex_protocol::approvals::ElicitationRequestEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::McpServerSnapshotState;
use codex_protocol::protocol::McpStartupCompleteEvent;
use codex_protocol::protocol::McpStartupFailure;
use codex_protocol::protocol::McpStartupStatus;
use codex_protocol::protocol::McpStartupUpdateEvent;
use codex_protocol::protocol::SandboxPolicy;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use codex_rmcp_client::RmcpClient;
use codex_rmcp_client::SendElicitation;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use mcp_types::ClientCapabilities;
use mcp_types::Implementation;
use mcp_types::ListResourceTemplatesRequestParams;
use mcp_types::ListResourceTemplatesResult;
use mcp_types::ListResourcesRequestParams;
use mcp_types::ListResourcesResult;
use mcp_types::ReadResourceRequestParams;
use mcp_types::ReadResourceResult;
use mcp_types::RequestId;
use mcp_types::Resource;
use mcp_types::ResourceTemplate;
use mcp_types::Tool;
use mcp_types::ToolInputSchema;

use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use sha1::Digest;
use sha1::Sha1;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct McpHookContext {
    user_hooks: UserHooks,
    thread_id: String,
    cwd: String,
}

impl McpHookContext {
    pub(crate) fn new(user_hooks: UserHooks, thread_id: String, cwd: String) -> Self {
        Self {
            user_hooks,
            thread_id,
            cwd,
        }
    }
}
use tracing::instrument;
use tracing::warn;

use crate::codex::INITIAL_SUBMIT_ID;
use crate::config::types::McpServerConfig;
use crate::config::types::McpServerTransportConfig;
use crate::config::types::McpStartupMode;

/// Delimiter used to separate the server name from the tool name in a fully
/// qualified tool name.
///
/// OpenAI requires tool names to conform to `^[a-zA-Z0-9_-]+$`, so we must
/// choose a delimiter from this character set.
const MCP_TOOL_NAME_DELIMITER: &str = "__";
const MAX_TOOL_NAME_LENGTH: usize = 64;

/// Default timeout for initializing MCP server & initially listing tools.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Default timeout for individual tool calls.
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// The Responses API requires tool names to match `^[a-zA-Z0-9_-]+$`.
/// MCP server/tool names are user-controlled, so sanitize the fully-qualified
/// name we expose to the model by replacing any disallowed character with `_`.
fn sanitize_responses_api_tool_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }

    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}

fn sha1_hex(s: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let sha1 = hasher.finalize();
    format!("{sha1:x}")
}

fn manifest_cache_path(codex_home: &Path) -> PathBuf {
    codex_home.join("mcp").join("manifest-cache.json")
}

async fn load_manifest_cache(codex_home: &Path) -> ManifestCache {
    let path = manifest_cache_path(codex_home);
    let Ok(contents) = tokio::fs::read_to_string(&path).await else {
        return ManifestCache::default();
    };
    match serde_json::from_str::<ManifestCache>(&contents) {
        Ok(cache) => cache,
        Err(err) => {
            warn!(
                "Failed to parse MCP manifest cache at {}: {err:#}",
                path.display()
            );
            ManifestCache::default()
        }
    }
}

async fn persist_manifest_cache(codex_home: &Path, cache: &ManifestCache) {
    let path = manifest_cache_path(codex_home);
    if let Some(parent) = path.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        warn!(
            "Failed to create MCP manifest cache dir {}: {err:#}",
            parent.display()
        );
        return;
    }
    match serde_json::to_string_pretty(cache) {
        Ok(contents) => {
            if let Err(err) = tokio::fs::write(&path, contents).await {
                warn!(
                    "Failed to write MCP manifest cache {}: {err:#}",
                    path.display()
                );
            }
        }
        Err(err) => {
            warn!(
                "Failed to serialize MCP manifest cache {}: {err:#}",
                path.display()
            );
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ManifestTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        env_vars: Vec<String>,
        cwd: Option<String>,
    },
    StreamableHttp {
        url: String,
        bearer_token_env_var: Option<String>,
        http_headers: BTreeMap<String, String>,
        env_http_headers: BTreeMap<String, String>,
    },
}

#[derive(Serialize)]
struct ManifestFingerprint {
    transport: ManifestTransport,
    startup_timeout_sec: Option<f64>,
    tool_timeout_sec: Option<f64>,
    enabled_tools: Option<Vec<String>>,
    disabled_tools: Option<Vec<String>>,
}

fn to_btreemap(input: Option<HashMap<String, String>>) -> BTreeMap<String, String> {
    input.unwrap_or_default().into_iter().collect()
}

fn server_config_hash(config: &McpServerConfig) -> String {
    let transport = match &config.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
            cwd,
        } => ManifestTransport::Stdio {
            command: command.clone(),
            args: args.clone(),
            env: to_btreemap(env.clone()),
            env_vars: env_vars.clone(),
            cwd: cwd.as_ref().map(|p| p.to_string_lossy().to_string()),
        },
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => ManifestTransport::StreamableHttp {
            url: url.clone(),
            bearer_token_env_var: bearer_token_env_var.clone(),
            http_headers: to_btreemap(http_headers.clone()),
            env_http_headers: to_btreemap(env_http_headers.clone()),
        },
    };
    let fingerprint = ManifestFingerprint {
        transport,
        startup_timeout_sec: config
            .startup_timeout_sec
            .as_ref()
            .map(Duration::as_secs_f64),
        tool_timeout_sec: config.tool_timeout_sec.as_ref().map(Duration::as_secs_f64),
        enabled_tools: config.enabled_tools.clone(),
        disabled_tools: config.disabled_tools.clone(),
    };
    let payload = serde_json::to_string(&fingerprint).unwrap_or_default();
    sha1_hex(&payload)
}

async fn update_manifest_cache(
    codex_home: &Path,
    manifest_cache: &Arc<Mutex<ManifestCache>>,
    server_name: &str,
    config: &McpServerConfig,
    tools: &[ToolInfo],
) {
    let config_hash = server_config_hash(config);
    let cached_tools = tools
        .iter()
        .map(|tool| CachedTool {
            name: tool.tool_name.clone(),
            description: tool.tool.description.clone(),
            connector_id: tool.connector_id.clone(),
            connector_name: tool.connector_name.clone(),
        })
        .collect::<Vec<_>>();
    let mut cache = manifest_cache.lock().await;
    cache.servers.insert(
        server_name.to_string(),
        CachedServerTools {
            config_hash,
            tools: cached_tools,
        },
    );
    persist_manifest_cache(codex_home, &cache).await;
}

fn qualify_tools<I>(tools: I) -> HashMap<String, ToolInfo>
where
    I: IntoIterator<Item = ToolInfo>,
{
    let mut used_names = HashSet::new();
    let mut seen_raw_names = HashSet::new();
    let mut qualified_tools = HashMap::new();
    for tool in tools {
        let qualified_name_raw = format!(
            "mcp{}{}{}{}",
            MCP_TOOL_NAME_DELIMITER, tool.server_name, MCP_TOOL_NAME_DELIMITER, tool.tool_name
        );
        if !seen_raw_names.insert(qualified_name_raw.clone()) {
            warn!("skipping duplicated tool {}", qualified_name_raw);
            continue;
        }

        // Start from a "pretty" name (sanitized), then deterministically disambiguate on
        // collisions by appending a hash of the *raw* (unsanitized) qualified name. This
        // ensures tools like `foo.bar` and `foo_bar` don't collapse to the same key.
        let mut qualified_name = sanitize_responses_api_tool_name(&qualified_name_raw);

        // Enforce length constraints early; use the raw name for the hash input so the
        // output remains stable even when sanitization changes.
        if qualified_name.len() > MAX_TOOL_NAME_LENGTH {
            let sha1_str = sha1_hex(&qualified_name_raw);
            let prefix_len = MAX_TOOL_NAME_LENGTH - sha1_str.len();
            qualified_name = format!("{}{}", &qualified_name[..prefix_len], sha1_str);
        }

        if used_names.contains(&qualified_name) {
            warn!("skipping duplicated tool {}", qualified_name);
            continue;
        }

        used_names.insert(qualified_name.clone());
        qualified_tools.insert(qualified_name, tool);
    }

    qualified_tools
}

#[derive(Clone)]
pub(crate) struct ToolInfo {
    pub(crate) server_name: String,
    pub(crate) tool_name: String,
    pub(crate) tool: Tool,
    pub(crate) connector_id: Option<String>,
    pub(crate) connector_name: Option<String>,
}

type ResponderMap = HashMap<(String, RequestId), oneshot::Sender<ElicitationResponse>>;

#[derive(Clone, Default)]
struct ElicitationRequestManager {
    requests: Arc<Mutex<ResponderMap>>,
}

impl ElicitationRequestManager {
    async fn resolve(
        &self,
        server_name: String,
        id: RequestId,
        response: ElicitationResponse,
    ) -> Result<()> {
        self.requests
            .lock()
            .await
            .remove(&(server_name, id))
            .ok_or_else(|| anyhow!("elicitation request not found"))?
            .send(response)
            .map_err(|e| anyhow!("failed to send elicitation response: {e:?}"))
    }

    fn make_sender(
        &self,
        server_name: String,
        tx_event: Sender<Event>,
        hook_context: Option<McpHookContext>,
    ) -> SendElicitation {
        let elicitation_requests = self.requests.clone();
        Box::new(move |id, elicitation| {
            let elicitation_requests = elicitation_requests.clone();
            let tx_event = tx_event.clone();
            let server_name = server_name.clone();
            let hook_context = hook_context.clone();
            async move {
                let (tx, rx) = oneshot::channel();
                {
                    let mut lock = elicitation_requests.lock().await;
                    lock.insert((server_name.clone(), id.clone()), tx);
                }
                if let Some(hook_context) = &hook_context {
                    let request_id = match &id {
                        RequestId::String(id) => id.clone(),
                        RequestId::Integer(id) => id.to_string(),
                    };
                    hook_context.user_hooks.approval_requested_elicitation(
                        hook_context.thread_id.clone(),
                        hook_context.cwd.clone(),
                        server_name.clone(),
                        request_id,
                        elicitation.message.clone(),
                    );
                }
                let _ = tx_event
                    .send(Event {
                        id: "mcp_elicitation_request".to_string(),
                        msg: EventMsg::ElicitationRequest(ElicitationRequestEvent {
                            server_name,
                            id,
                            message: elicitation.message,
                        }),
                    })
                    .await;
                rx.await
                    .context("elicitation request channel closed unexpectedly")
            }
            .boxed()
        })
    }
}

#[derive(Clone)]
struct ManagedClient {
    client: Arc<RmcpClient>,
    tools: Vec<ToolInfo>,
    tool_filter: ToolFilter,
    tool_timeout: Option<Duration>,
    server_supports_sandbox_state_capability: bool,
}

impl ManagedClient {
    /// Returns once the server has ack'd the sandbox state update.
    async fn notify_sandbox_state_change(&self, sandbox_state: &SandboxState) -> Result<()> {
        if !self.server_supports_sandbox_state_capability {
            return Ok(());
        }

        let _response = self
            .client
            .send_custom_request(
                MCP_SANDBOX_STATE_METHOD,
                Some(serde_json::to_value(sandbox_state)?),
            )
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct AsyncManagedClient {
    client: Shared<BoxFuture<'static, Result<ManagedClient, StartupOutcomeError>>>,
}

impl AsyncManagedClient {
    fn new(
        server_name: String,
        config: McpServerConfig,
        store_mode: OAuthCredentialsStoreMode,
        cancel_token: CancellationToken,
        tx_event: Sender<Event>,
        elicitation_requests: ElicitationRequestManager,
        hook_context: Option<McpHookContext>,
    ) -> Self {
        let tool_filter = ToolFilter::from_config(&config);
        let fut = async move {
            if let Err(error) = validate_mcp_server_name(&server_name) {
                return Err(error.into());
            }

            let startup_timeout = config.startup_timeout_sec.or(Some(DEFAULT_STARTUP_TIMEOUT));

            let client_fut = make_rmcp_client(&server_name, config.transport, store_mode);
            let client_result = match startup_timeout {
                Some(duration) => {
                    tokio::time::timeout(duration, client_fut.or_cancel(&cancel_token))
                        .await
                        .map_err(|_| anyhow!("request timed out after {duration:?}"))?
                }
                None => client_fut.or_cancel(&cancel_token).await,
            };

            let client = match client_result {
                Ok(Ok(client)) => Arc::new(client),
                Ok(Err(err)) => return Err(err),
                Err(CancelErr::Cancelled) => return Err(StartupOutcomeError::Cancelled),
            };
            match start_server_task(
                server_name,
                client,
                startup_timeout,
                config.tool_timeout_sec.unwrap_or(DEFAULT_TOOL_TIMEOUT),
                tool_filter,
                tx_event,
                elicitation_requests,
                hook_context,
            )
            .or_cancel(&cancel_token)
            .await
            {
                Ok(result) => result,
                Err(CancelErr::Cancelled) => Err(StartupOutcomeError::Cancelled),
            }
        };
        Self {
            client: fut.boxed().shared(),
        }
    }

    async fn client(&self) -> Result<ManagedClient, StartupOutcomeError> {
        self.client.clone().await
    }

    async fn notify_sandbox_state_change(&self, sandbox_state: &SandboxState) -> Result<()> {
        let managed = self.client().await?;
        managed.notify_sandbox_state_change(sandbox_state).await
    }
}

pub const MCP_SANDBOX_STATE_CAPABILITY: &str = "codex/sandbox-state";

/// Custom MCP request to push sandbox state updates.
/// When used, the `params` field of the notification is [`SandboxState`].
pub const MCP_SANDBOX_STATE_METHOD: &str = "codex/sandbox-state/update";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxState {
    pub sandbox_policy: SandboxPolicy,
    pub codex_linux_sandbox_exe: Option<PathBuf>,
    pub sandbox_cwd: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ManifestCache {
    servers: HashMap<String, CachedServerTools>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedServerTools {
    config_hash: String,
    tools: Vec<CachedTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTool {
    name: String,
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connector_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connector_name: Option<String>,
}

/// A thin wrapper around a set of running [`RmcpClient`] instances.
pub(crate) struct McpConnectionManager {
    clients: HashMap<String, AsyncManagedClient>,
    elicitation_requests: ElicitationRequestManager,
    ready_clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    manifest_cache: Arc<Mutex<ManifestCache>>,
    server_configs: HashMap<String, McpServerConfig>,
    startup_mode: McpStartupMode,
    codex_home: Option<PathBuf>,
    sandbox_state: Option<SandboxState>,
    tx_event: Option<Sender<Event>>,
}

impl Default for McpConnectionManager {
    fn default() -> Self {
        Self {
            clients: HashMap::new(),
            elicitation_requests: ElicitationRequestManager::default(),
            ready_clients: Arc::new(Mutex::new(HashMap::new())),
            manifest_cache: Arc::new(Mutex::new(ManifestCache::default())),
            server_configs: HashMap::new(),
            startup_mode: McpStartupMode::default(),
            codex_home: None,
            sandbox_state: None,
            tx_event: None,
        }
    }
}

impl McpConnectionManager {
    #[allow(clippy::too_many_arguments)]
    pub async fn initialize(
        &mut self,
        mcp_servers: &HashMap<String, McpServerConfig>,
        store_mode: OAuthCredentialsStoreMode,
        auth_entries: HashMap<String, McpAuthStatusEntry>,
        tx_event: Sender<Event>,
        cancel_token: CancellationToken,
        initial_sandbox_state: SandboxState,
        hook_context: Option<McpHookContext>,
        startup_mode: McpStartupMode,
        codex_home: PathBuf,
    ) {
        if cancel_token.is_cancelled() {
            return;
        }
        let mut clients = HashMap::new();
        let mut join_set = JoinSet::new();
        let mut started_any = false;
        let elicitation_requests = ElicitationRequestManager::default();
        let mcp_servers = mcp_servers.clone();
        let ready_clients = Arc::new(Mutex::new(HashMap::new()));
        let manifest_cache = Arc::new(Mutex::new(load_manifest_cache(&codex_home).await));

        self.server_configs = mcp_servers.clone();
        self.startup_mode = startup_mode;
        self.codex_home = Some(codex_home.clone());
        self.sandbox_state = Some(initial_sandbox_state.clone());
        self.tx_event = Some(tx_event.clone());
        self.ready_clients = Arc::clone(&ready_clients);
        self.manifest_cache = Arc::clone(&manifest_cache);
        self.elicitation_requests = elicitation_requests.clone();

        for (server_name, cfg) in mcp_servers.into_iter().filter(|(_, cfg)| cfg.enabled) {
            let cancel_token = cancel_token.child_token();
            let hook_context = hook_context.clone();
            let async_managed_client = AsyncManagedClient::new(
                server_name.clone(),
                cfg.clone(),
                store_mode,
                cancel_token.clone(),
                tx_event.clone(),
                elicitation_requests.clone(),
                hook_context,
            );
            clients.insert(server_name.clone(), async_managed_client.clone());

            if cfg.startup_mode.unwrap_or(startup_mode) != McpStartupMode::Eager {
                continue;
            }

            let _ = emit_update(
                &tx_event,
                McpStartupUpdateEvent {
                    server: server_name.clone(),
                    status: McpStartupStatus::Starting,
                },
            )
            .await;

            let tx_event = tx_event.clone();
            let auth_entry = auth_entries.get(&server_name).cloned();
            let sandbox_state = initial_sandbox_state.clone();
            let ready_clients = Arc::clone(&ready_clients);
            let manifest_cache = Arc::clone(&manifest_cache);
            let codex_home = codex_home.clone();
            started_any = true;
            join_set.spawn(async move {
                let outcome = async_managed_client.client().await;
                if cancel_token.is_cancelled() {
                    return (server_name, Err(StartupOutcomeError::Cancelled));
                }
                let status = match &outcome {
                    Ok(managed) => {
                        ready_clients
                            .lock()
                            .await
                            .insert(server_name.clone(), managed.clone());
                        update_manifest_cache(
                            &codex_home,
                            &manifest_cache,
                            &server_name,
                            &cfg,
                            &managed.tools,
                        )
                        .await;
                        if let Err(e) = async_managed_client
                            .notify_sandbox_state_change(&sandbox_state)
                            .await
                        {
                            warn!(
                                "Failed to notify sandbox state to MCP server {server_name}: {e:#}",
                            );
                        }
                        McpStartupStatus::Ready
                    }
                    Err(error) => {
                        let error_str = mcp_init_error_display(
                            server_name.as_str(),
                            auth_entry.as_ref(),
                            error,
                        );
                        McpStartupStatus::Failed { error: error_str }
                    }
                };

                let _ = emit_update(
                    &tx_event,
                    McpStartupUpdateEvent {
                        server: server_name.clone(),
                        status,
                    },
                )
                .await;

                (server_name, outcome)
            });
        }

        self.clients = clients;

        if started_any {
            tokio::spawn(async move {
                let outcomes = join_set.join_all().await;
                let mut summary = McpStartupCompleteEvent::default();
                for (server_name, outcome) in outcomes {
                    match outcome {
                        Ok(_) => summary.ready.push(server_name),
                        Err(StartupOutcomeError::Cancelled) => summary.cancelled.push(server_name),
                        Err(StartupOutcomeError::Failed { error }) => {
                            summary.failed.push(McpStartupFailure {
                                server: server_name,
                                error,
                            })
                        }
                    }
                }
                let _ = tx_event
                    .send(Event {
                        id: INITIAL_SUBMIT_ID.to_owned(),
                        msg: EventMsg::McpStartupComplete(summary),
                    })
                    .await;
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn retry_servers(
        &mut self,
        mcp_servers: HashMap<String, McpServerConfig>,
        store_mode: OAuthCredentialsStoreMode,
        auth_entries: HashMap<String, McpAuthStatusEntry>,
        tx_event: Sender<Event>,
        cancel_token: CancellationToken,
        sandbox_state: SandboxState,
        hook_context: Option<McpHookContext>,
    ) {
        if cancel_token.is_cancelled() {
            return;
        }

        let mut join_set = JoinSet::new();
        for (server_name, cfg) in mcp_servers.into_iter().filter(|(_, cfg)| cfg.enabled) {
            let cancel_token = cancel_token.child_token();
            let hook_context = hook_context.clone();
            let _ = emit_update(
                &tx_event,
                McpStartupUpdateEvent {
                    server: server_name.clone(),
                    status: McpStartupStatus::Starting,
                },
            )
            .await;
            let async_managed_client = AsyncManagedClient::new(
                server_name.clone(),
                cfg,
                store_mode,
                cancel_token.clone(),
                tx_event.clone(),
                self.elicitation_requests.clone(),
                hook_context,
            );

            // Replace any previous client for this server (dropping the old connection).
            self.clients
                .insert(server_name.clone(), async_managed_client.clone());

            let tx_event = tx_event.clone();
            let auth_entry = auth_entries.get(&server_name).cloned();
            let sandbox_state = sandbox_state.clone();
            join_set.spawn(async move {
                let outcome = async_managed_client.client().await;
                if cancel_token.is_cancelled() {
                    return (server_name, Err(StartupOutcomeError::Cancelled));
                }
                let status = match &outcome {
                    Ok(_) => {
                        // Send sandbox state notification immediately after Ready
                        if let Err(e) = async_managed_client
                            .notify_sandbox_state_change(&sandbox_state)
                            .await
                        {
                            warn!(
                                "Failed to notify sandbox state to MCP server {server_name}: {e:#}",
                            );
                        }
                        McpStartupStatus::Ready
                    }
                    Err(error) => {
                        let error_str = mcp_init_error_display(
                            server_name.as_str(),
                            auth_entry.as_ref(),
                            error,
                        );
                        McpStartupStatus::Failed { error: error_str }
                    }
                };

                let _ = emit_update(
                    &tx_event,
                    McpStartupUpdateEvent {
                        server: server_name.clone(),
                        status,
                    },
                )
                .await;

                (server_name, outcome)
            });
        }

        let tx_event = tx_event.clone();
        tokio::spawn(async move {
            let outcomes = join_set.join_all().await;
            let mut summary = McpStartupCompleteEvent::default();
            for (server_name, outcome) in outcomes {
                match outcome {
                    Ok(_) => summary.ready.push(server_name),
                    Err(StartupOutcomeError::Cancelled) => summary.cancelled.push(server_name),
                    Err(StartupOutcomeError::Failed { error }) => {
                        summary.failed.push(McpStartupFailure {
                            server: server_name,
                            error,
                        })
                    }
                }
            }
            let _ = tx_event
                .send(Event {
                    id: INITIAL_SUBMIT_ID.to_owned(),
                    msg: EventMsg::McpStartupComplete(summary),
                })
                .await;
        });
    }

    async fn ensure_server_ready(
        &self,
        server_name: &str,
        trigger: StartupTrigger,
    ) -> Result<ManagedClient> {
        if let Some(managed) = self.ready_clients.lock().await.get(server_name).cloned() {
            return Ok(managed);
        }

        let Some(config) = self.server_configs.get(server_name) else {
            return Err(anyhow!("unknown MCP server '{server_name}'"));
        };
        if !config.enabled {
            return Err(anyhow!("MCP server '{server_name}' is disabled"));
        }
        let startup_mode = config.startup_mode.unwrap_or(self.startup_mode);
        if startup_mode == McpStartupMode::Manual && matches!(trigger, StartupTrigger::ToolCall) {
            return Err(anyhow!(
                "MCP server '{server_name}' is not running (manual mode). Run `/mcp load {server_name}`."
            ));
        }

        let async_client = self
            .clients
            .get(server_name)
            .ok_or_else(|| anyhow!("unknown MCP server '{server_name}'"))?
            .clone();

        if let Some(tx_event) = &self.tx_event {
            let _ = emit_update(
                tx_event,
                McpStartupUpdateEvent {
                    server: server_name.to_string(),
                    status: McpStartupStatus::Starting,
                },
            )
            .await;
        }

        let outcome = async_client.client().await;
        let status = match &outcome {
            Ok(managed) => {
                self.ready_clients
                    .lock()
                    .await
                    .insert(server_name.to_string(), managed.clone());
                if let Some(codex_home) = &self.codex_home {
                    update_manifest_cache(
                        codex_home,
                        &self.manifest_cache,
                        server_name,
                        config,
                        &managed.tools,
                    )
                    .await;
                }
                if let Some(sandbox_state) = &self.sandbox_state
                    && let Err(err) = async_client
                        .notify_sandbox_state_change(sandbox_state)
                        .await
                {
                    warn!("Failed to notify sandbox state to MCP server {server_name}: {err:#}");
                }
                McpStartupStatus::Ready
            }
            Err(error) => {
                let error_str = mcp_init_error_display(server_name, None, error);
                McpStartupStatus::Failed { error: error_str }
            }
        };

        if let Some(tx_event) = &self.tx_event {
            let _ = emit_update(
                tx_event,
                McpStartupUpdateEvent {
                    server: server_name.to_string(),
                    status: status.clone(),
                },
            )
            .await;

            let mut summary = McpStartupCompleteEvent::default();
            match &status {
                McpStartupStatus::Ready => summary.ready.push(server_name.to_string()),
                McpStartupStatus::Failed { error } => summary.failed.push(McpStartupFailure {
                    server: server_name.to_string(),
                    error: error.clone(),
                }),
                McpStartupStatus::Cancelled => summary.cancelled.push(server_name.to_string()),
                McpStartupStatus::Starting => {}
            }
            if !summary.ready.is_empty()
                || !summary.failed.is_empty()
                || !summary.cancelled.is_empty()
            {
                let _ = tx_event
                    .send(Event {
                        id: INITIAL_SUBMIT_ID.to_owned(),
                        msg: EventMsg::McpStartupComplete(summary),
                    })
                    .await;
            }
        }

        match outcome {
            Ok(managed) => Ok(managed),
            Err(error) => Err(anyhow!(
                "MCP server '{server_name}' failed to start: {error}"
            )),
        }
    }

    pub async fn load_servers(&self, servers: &[String]) -> Result<()> {
        for server in servers {
            self.ensure_server_ready(server, StartupTrigger::ManualLoad)
                .await?;
        }
        Ok(())
    }

    pub async fn resolve_elicitation(
        &self,
        server_name: String,
        id: RequestId,
        response: ElicitationResponse,
    ) -> Result<()> {
        self.elicitation_requests
            .resolve(server_name, id, response)
            .await
    }

    pub(crate) async fn wait_for_server_ready(&self, server_name: &str, timeout: Duration) -> bool {
        let Some(async_managed_client) = self.clients.get(server_name) else {
            return false;
        };

        match tokio::time::timeout(timeout, async_managed_client.client()).await {
            Ok(Ok(_)) => true,
            Ok(Err(_)) | Err(_) => false,
        }
    }

    /// Returns a single map that contains all tools. Each key is the
    /// fully-qualified name for the tool.
    #[instrument(level = "trace", skip_all)]
    pub async fn list_all_tools(&self) -> HashMap<String, ToolInfo> {
        let mut tools = HashMap::new();
        let ready_clients = self.ready_clients.lock().await;
        for managed in ready_clients.values() {
            tools.extend(qualify_tools(filter_tools(
                managed.tools.clone(),
                managed.tool_filter.clone(),
            )));
        }

        if let Some(managed_client) = self.clients.get(CODEX_APPS_MCP_SERVER_NAME)
            && !ready_clients.contains_key(CODEX_APPS_MCP_SERVER_NAME)
        {
            // Avoid blocking on codex_apps_mcp startup; use tools only when ready.
            if let Some(Ok(client)) = managed_client.client.clone().now_or_never() {
                tools.extend(qualify_tools(filter_tools(
                    client.tools,
                    client.tool_filter,
                )));
            }
        }

        let manifest_cache = self.manifest_cache.lock().await;
        for (server_name, cached) in manifest_cache.servers.iter() {
            if ready_clients.contains_key(server_name) {
                continue;
            }
            let Some(config) = self.server_configs.get(server_name) else {
                continue;
            };
            if server_config_hash(config) != cached.config_hash {
                continue;
            }
            let filter = ToolFilter::from_config(config);
            let cached_tools = cached
                .tools
                .iter()
                .map(|tool| ToolInfo {
                    server_name: server_name.clone(),
                    tool_name: tool.name.clone(),
                    tool: stub_tool_from_manifest(tool),
                    connector_id: tool.connector_id.clone(),
                    connector_name: tool.connector_name.clone(),
                })
                .collect::<Vec<_>>();
            tools.extend(qualify_tools(filter_tools(cached_tools, filter)));
        }

        tools
    }

    /// Returns a single map that contains all resources. Each key is the
    /// server name and the value is a vector of resources.
    pub async fn list_all_resources(&self) -> HashMap<String, Vec<Resource>> {
        let mut join_set = JoinSet::new();

        let ready_clients = self
            .ready_clients
            .lock()
            .await
            .iter()
            .map(|(server_name, managed)| (server_name.clone(), managed.clone()))
            .collect::<Vec<_>>();

        for (server_name, managed_client) in ready_clients {
            let timeout = managed_client.tool_timeout;
            let client = managed_client.client.clone();

            join_set.spawn(async move {
                let mut collected: Vec<Resource> = Vec::new();
                let mut cursor: Option<String> = None;

                loop {
                    let params = cursor.as_ref().map(|next| ListResourcesRequestParams {
                        cursor: Some(next.clone()),
                    });
                    let response = match client.list_resources(params, timeout).await {
                        Ok(result) => result,
                        Err(err) => return (server_name, Err(err)),
                    };

                    collected.extend(response.resources);

                    match response.next_cursor {
                        Some(next) => {
                            if cursor.as_ref() == Some(&next) {
                                return (
                                    server_name,
                                    Err(anyhow!("resources/list returned duplicate cursor")),
                                );
                            }
                            cursor = Some(next);
                        }
                        None => return (server_name, Ok(collected)),
                    }
                }
            });
        }

        let mut aggregated: HashMap<String, Vec<Resource>> = HashMap::new();

        while let Some(join_res) = join_set.join_next().await {
            match join_res {
                Ok((server_name, Ok(resources))) => {
                    aggregated.insert(server_name, resources);
                }
                Ok((server_name, Err(err))) => {
                    warn!("Failed to list resources for MCP server '{server_name}': {err:#}");
                }
                Err(err) => {
                    warn!("Task panic when listing resources for MCP server: {err:#}");
                }
            }
        }

        aggregated
    }

    pub async fn list_server_snapshot_states(&self) -> HashMap<String, McpServerSnapshotState> {
        let mut states = HashMap::new();

        let ready_clients = self.ready_clients.lock().await;
        let manifest_cache = self.manifest_cache.lock().await;

        for (server_name, config) in &self.server_configs {
            if !config.enabled {
                continue;
            }

            if ready_clients.contains_key(server_name) {
                states.insert(server_name.clone(), McpServerSnapshotState::Ready);
                continue;
            }

            if let Some(cached) = manifest_cache.servers.get(server_name)
                && server_config_hash(config) == cached.config_hash
            {
                states.insert(server_name.clone(), McpServerSnapshotState::Cached);
            }
        }

        states
    }

    /// Returns a single map that contains all resource templates. Each key is the
    /// server name and the value is a vector of resource templates.
    pub async fn list_all_resource_templates(&self) -> HashMap<String, Vec<ResourceTemplate>> {
        let mut join_set = JoinSet::new();

        let ready_clients = self
            .ready_clients
            .lock()
            .await
            .iter()
            .map(|(server_name, managed)| (server_name.clone(), managed.clone()))
            .collect::<Vec<_>>();

        for (server_name_cloned, managed_client) in ready_clients {
            let client = managed_client.client.clone();
            let timeout = managed_client.tool_timeout;

            join_set.spawn(async move {
                let mut collected: Vec<ResourceTemplate> = Vec::new();
                let mut cursor: Option<String> = None;

                loop {
                    let params = cursor
                        .as_ref()
                        .map(|next| ListResourceTemplatesRequestParams {
                            cursor: Some(next.clone()),
                        });
                    let response = match client.list_resource_templates(params, timeout).await {
                        Ok(result) => result,
                        Err(err) => return (server_name_cloned, Err(err)),
                    };

                    collected.extend(response.resource_templates);

                    match response.next_cursor {
                        Some(next) => {
                            if cursor.as_ref() == Some(&next) {
                                return (
                                    server_name_cloned,
                                    Err(anyhow!(
                                        "resources/templates/list returned duplicate cursor"
                                    )),
                                );
                            }
                            cursor = Some(next);
                        }
                        None => return (server_name_cloned, Ok(collected)),
                    }
                }
            });
        }

        let mut aggregated: HashMap<String, Vec<ResourceTemplate>> = HashMap::new();

        while let Some(join_res) = join_set.join_next().await {
            match join_res {
                Ok((server_name, Ok(templates))) => {
                    aggregated.insert(server_name, templates);
                }
                Ok((server_name, Err(err))) => {
                    warn!(
                        "Failed to list resource templates for MCP server '{server_name}': {err:#}"
                    );
                }
                Err(err) => {
                    warn!("Task panic when listing resource templates for MCP server: {err:#}");
                }
            }
        }

        aggregated
    }

    /// Invoke the tool indicated by the (server, tool) pair.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<mcp_types::CallToolResult> {
        let client = self
            .ensure_server_ready(server, StartupTrigger::ToolCall)
            .await?;
        if !client.tool_filter.allows(tool) {
            return Err(anyhow!(
                "tool '{tool}' is disabled for MCP server '{server}'"
            ));
        }

        client
            .client
            .call_tool(tool.to_string(), arguments, client.tool_timeout)
            .await
            .with_context(|| format!("tool call failed for `{server}/{tool}`"))
    }

    /// List resources from the specified server.
    pub async fn list_resources(
        &self,
        server: &str,
        params: Option<ListResourcesRequestParams>,
    ) -> Result<ListResourcesResult> {
        let managed = self
            .ensure_server_ready(server, StartupTrigger::ToolCall)
            .await?;
        let timeout = managed.tool_timeout;

        managed
            .client
            .list_resources(params, timeout)
            .await
            .with_context(|| format!("resources/list failed for `{server}`"))
    }

    /// List resource templates from the specified server.
    pub async fn list_resource_templates(
        &self,
        server: &str,
        params: Option<ListResourceTemplatesRequestParams>,
    ) -> Result<ListResourceTemplatesResult> {
        let managed = self
            .ensure_server_ready(server, StartupTrigger::ToolCall)
            .await?;
        let client = managed.client.clone();
        let timeout = managed.tool_timeout;

        client
            .list_resource_templates(params, timeout)
            .await
            .with_context(|| format!("resources/templates/list failed for `{server}`"))
    }

    /// Read a resource from the specified server.
    pub async fn read_resource(
        &self,
        server: &str,
        params: ReadResourceRequestParams,
    ) -> Result<ReadResourceResult> {
        let managed = self
            .ensure_server_ready(server, StartupTrigger::ToolCall)
            .await?;
        let client = managed.client.clone();
        let timeout = managed.tool_timeout;
        let uri = params.uri.clone();

        client
            .read_resource(params, timeout)
            .await
            .with_context(|| format!("resources/read failed for `{server}` ({uri})"))
    }

    pub async fn parse_tool_name(&self, tool_name: &str) -> Option<(String, String)> {
        if let Some((server, tool)) = crate::mcp::split_qualified_tool_name(tool_name) {
            return Some((server, tool));
        }

        self.list_all_tools()
            .await
            .get(tool_name)
            .map(|tool| (tool.server_name.clone(), tool.tool_name.clone()))
    }

    pub async fn notify_sandbox_state_change(&self, sandbox_state: &SandboxState) -> Result<()> {
        let mut join_set = JoinSet::new();

        let ready_clients = self
            .ready_clients
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();

        for managed_client in ready_clients {
            let sandbox_state = sandbox_state.clone();
            join_set.spawn(async move {
                managed_client
                    .notify_sandbox_state_change(&sandbox_state)
                    .await
            });
        }

        while let Some(join_res) = join_set.join_next().await {
            match join_res {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    warn!("Failed to notify sandbox state change to MCP server: {err:#}");
                }
                Err(err) => {
                    warn!("Task panic when notifying sandbox state change to MCP server: {err:#}");
                }
            }
        }

        Ok(())
    }
}

async fn emit_update(
    tx_event: &Sender<Event>,
    update: McpStartupUpdateEvent,
) -> Result<(), async_channel::SendError<Event>> {
    tx_event
        .send(Event {
            id: INITIAL_SUBMIT_ID.to_owned(),
            msg: EventMsg::McpStartupUpdate(update),
        })
        .await
}

/// A tool is allowed to be used if both are true:
/// 1. enabled is None (no allowlist is set) or the tool is explicitly enabled.
/// 2. The tool is not explicitly disabled.
#[derive(Default, Clone)]
pub(crate) struct ToolFilter {
    enabled: Option<HashSet<String>>,
    disabled: HashSet<String>,
}

impl ToolFilter {
    fn from_config(cfg: &McpServerConfig) -> Self {
        let enabled = cfg
            .enabled_tools
            .as_ref()
            .map(|tools| tools.iter().cloned().collect::<HashSet<_>>());
        let disabled = cfg
            .disabled_tools
            .as_ref()
            .map(|tools| tools.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();

        Self { enabled, disabled }
    }

    fn allows(&self, tool_name: &str) -> bool {
        if let Some(enabled) = &self.enabled
            && !enabled.contains(tool_name)
        {
            return false;
        }

        !self.disabled.contains(tool_name)
    }
}

fn filter_tools(tools: Vec<ToolInfo>, filter: ToolFilter) -> Vec<ToolInfo> {
    tools
        .into_iter()
        .filter(|tool| filter.allows(&tool.tool_name))
        .collect()
}

fn normalize_codex_apps_tool_title(
    server_name: &str,
    connector_name: Option<&str>,
    value: &str,
) -> String {
    if server_name != CODEX_APPS_MCP_SERVER_NAME {
        return value.to_string();
    }

    let Some(connector_name) = connector_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return value.to_string();
    };

    let prefix = format!("{connector_name}_");
    if let Some(stripped) = value.strip_prefix(&prefix)
        && !stripped.is_empty()
    {
        return stripped.to_string();
    }

    value.to_string()
}

fn stub_tool_from_manifest(tool: &CachedTool) -> Tool {
    Tool {
        name: tool.name.clone(),
        input_schema: ToolInputSchema {
            properties: Some(json!({})),
            required: None,
            r#type: "object".to_string(),
        },
        output_schema: None,
        title: None,
        annotations: None,
        description: tool.description.clone(),
    }
}

fn resolve_bearer_token(
    server_name: &str,
    bearer_token_env_var: Option<&str>,
) -> Result<Option<String>> {
    let Some(env_var) = bearer_token_env_var else {
        return Ok(None);
    };

    match env::var(env_var) {
        Ok(value) => {
            if value.is_empty() {
                Err(anyhow!(
                    "Environment variable {env_var} for MCP server '{server_name}' is empty"
                ))
            } else {
                Ok(Some(value))
            }
        }
        Err(env::VarError::NotPresent) => Err(anyhow!(
            "Environment variable {env_var} for MCP server '{server_name}' is not set"
        )),
        Err(env::VarError::NotUnicode(_)) => Err(anyhow!(
            "Environment variable {env_var} for MCP server '{server_name}' contains invalid Unicode"
        )),
    }
}

#[derive(Debug, Clone, thiserror::Error)]
enum StartupOutcomeError {
    #[error("MCP startup cancelled")]
    Cancelled,
    // We can't store the original error here because anyhow::Error doesn't implement
    // `Clone`.
    #[error("MCP startup failed: {error}")]
    Failed { error: String },
}

impl From<anyhow::Error> for StartupOutcomeError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed {
            error: error.to_string(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_server_task(
    server_name: String,
    client: Arc<RmcpClient>,
    startup_timeout: Option<Duration>, // TODO: cancel_token should handle this.
    tool_timeout: Duration,
    tool_filter: ToolFilter,
    tx_event: Sender<Event>,
    elicitation_requests: ElicitationRequestManager,
    hook_context: Option<McpHookContext>,
) -> Result<ManagedClient, StartupOutcomeError> {
    let params = mcp_types::InitializeRequestParams {
        capabilities: ClientCapabilities {
            experimental: None,
            roots: None,
            sampling: None,
            // https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation#capabilities
            // indicates this should be an empty object.
            elicitation: Some(json!({})),
        },
        client_info: Implementation {
            name: "codex-mcp-client".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            title: Some("Codex".into()),
            // This field is used by Codex when it is an MCP
            // server: it should not be used when Codex is
            // an MCP client.
            user_agent: None,
        },
        protocol_version: mcp_types::MCP_SCHEMA_VERSION.to_owned(),
    };

    let send_elicitation =
        elicitation_requests.make_sender(server_name.clone(), tx_event, hook_context);

    let initialize_result = client
        .initialize(params, startup_timeout, send_elicitation)
        .await
        .map_err(StartupOutcomeError::from)?;

    let tools = list_tools_for_client(&server_name, &client, startup_timeout)
        .await
        .map_err(StartupOutcomeError::from)?;

    let server_supports_sandbox_state_capability = initialize_result
        .capabilities
        .experimental
        .as_ref()
        .and_then(|exp| exp.get(MCP_SANDBOX_STATE_CAPABILITY))
        .is_some();

    let managed = ManagedClient {
        client: Arc::clone(&client),
        tools,
        tool_timeout: Some(tool_timeout),
        tool_filter,
        server_supports_sandbox_state_capability,
    };

    Ok(managed)
}

async fn make_rmcp_client(
    server_name: &str,
    transport: McpServerTransportConfig,
    store_mode: OAuthCredentialsStoreMode,
) -> Result<RmcpClient, StartupOutcomeError> {
    match transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
            cwd,
        } => {
            let command_os: OsString = command.into();
            let args_os: Vec<OsString> = args.into_iter().map(Into::into).collect();
            RmcpClient::new_stdio_client(command_os, args_os, env, &env_vars, cwd)
                .await
                .map_err(|err| StartupOutcomeError::from(anyhow!(err)))
        }
        McpServerTransportConfig::StreamableHttp {
            url,
            http_headers,
            env_http_headers,
            bearer_token_env_var,
        } => {
            let resolved_bearer_token =
                match resolve_bearer_token(server_name, bearer_token_env_var.as_deref()) {
                    Ok(token) => token,
                    Err(error) => return Err(error.into()),
                };
            RmcpClient::new_streamable_http_client(
                server_name,
                &url,
                resolved_bearer_token,
                http_headers,
                env_http_headers,
                store_mode,
            )
            .await
            .map_err(StartupOutcomeError::from)
        }
    }
}

async fn list_tools_for_client(
    server_name: &str,
    client: &Arc<RmcpClient>,
    timeout: Option<Duration>,
) -> Result<Vec<ToolInfo>> {
    let resp = client.list_tools_with_connector_ids(None, timeout).await?;
    Ok(resp
        .tools
        .into_iter()
        .map(|tool| {
            let connector_name = tool.connector_name;
            let mut tool_def = tool.tool;
            if let Some(title) = tool_def.title.as_deref() {
                let normalized_title =
                    normalize_codex_apps_tool_title(server_name, connector_name.as_deref(), title);
                if tool_def.title.as_deref() != Some(normalized_title.as_str()) {
                    tool_def.title = Some(normalized_title);
                }
            }
            ToolInfo {
                server_name: server_name.to_owned(),
                tool_name: tool_def.name.clone(),
                tool: tool_def,
                connector_id: tool.connector_id,
                connector_name,
            }
        })
        .collect())
}

fn validate_mcp_server_name(server_name: &str) -> Result<()> {
    let re = regex_lite::Regex::new(r"^[a-zA-Z0-9_-]+$")?;
    if !re.is_match(server_name) {
        return Err(anyhow!(
            "Invalid MCP server name '{server_name}': must match pattern {pattern}",
            pattern = re.as_str()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum StartupTrigger {
    ToolCall,
    ManualLoad,
}

fn mcp_init_error_display(
    server_name: &str,
    entry: Option<&McpAuthStatusEntry>,
    err: &StartupOutcomeError,
) -> String {
    if let Some(McpServerTransportConfig::StreamableHttp {
        url,
        bearer_token_env_var,
        http_headers,
        ..
    }) = &entry.map(|entry| &entry.config.transport)
        && url == "https://api.githubcopilot.com/mcp/"
        && bearer_token_env_var.is_none()
        && http_headers.as_ref().map(HashMap::is_empty).unwrap_or(true)
    {
        format!(
            "GitHub MCP does not support OAuth. Log in by adding a personal access token (https://github.com/settings/personal-access-tokens) to your environment and config.toml:\n[mcp_servers.{server_name}]\nbearer_token_env_var = CODEX_GITHUB_PERSONAL_ACCESS_TOKEN"
        )
    } else if is_mcp_client_auth_required_error(err) {
        format!(
            "The {server_name} MCP server is not logged in. Run `codex mcp login {server_name}`."
        )
    } else if is_mcp_client_startup_timeout_error(err) {
        let startup_timeout_secs = match entry {
            Some(entry) => match entry.config.startup_timeout_sec {
                Some(timeout) => timeout,
                None => DEFAULT_STARTUP_TIMEOUT,
            },
            None => DEFAULT_STARTUP_TIMEOUT,
        }
        .as_secs();
        format!(
            "MCP client for `{server_name}` timed out after {startup_timeout_secs} seconds. Add or adjust `startup_timeout_sec` in your config.toml:\n[mcp_servers.{server_name}]\nstartup_timeout_sec = XX"
        )
    } else {
        format!("MCP client for `{server_name}` failed to start: {err:#}")
    }
}

fn is_mcp_client_auth_required_error(error: &StartupOutcomeError) -> bool {
    match error {
        StartupOutcomeError::Failed { error } => error.contains("Auth required"),
        _ => false,
    }
}

fn is_mcp_client_startup_timeout_error(error: &StartupOutcomeError) -> bool {
    match error {
        StartupOutcomeError::Failed { error } => {
            error.contains("request timed out")
                || error.contains("timed out handshaking with MCP server")
        }
        _ => false,
    }
}

#[cfg(test)]
mod mcp_init_error_display_tests {}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::McpAuthStatus;
    use mcp_types::ToolInputSchema;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use tempfile::tempdir;

    fn create_test_tool(server_name: &str, tool_name: &str) -> ToolInfo {
        ToolInfo {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            tool: Tool {
                annotations: None,
                description: Some(format!("Test tool: {tool_name}")),
                input_schema: ToolInputSchema {
                    properties: None,
                    required: None,
                    r#type: "object".to_string(),
                },
                name: tool_name.to_string(),
                output_schema: None,
                title: None,
            },
            connector_id: None,
            connector_name: None,
        }
    }

    #[test]
    fn test_qualify_tools_short_non_duplicated_names() {
        let tools = vec![
            create_test_tool("server1", "tool1"),
            create_test_tool("server1", "tool2"),
        ];

        let qualified_tools = qualify_tools(tools);

        assert_eq!(qualified_tools.len(), 2);
        assert!(qualified_tools.contains_key("mcp__server1__tool1"));
        assert!(qualified_tools.contains_key("mcp__server1__tool2"));
    }

    #[test]
    fn test_qualify_tools_duplicated_names_skipped() {
        let tools = vec![
            create_test_tool("server1", "duplicate_tool"),
            create_test_tool("server1", "duplicate_tool"),
        ];

        let qualified_tools = qualify_tools(tools);

        // Only the first tool should remain, the second is skipped
        assert_eq!(qualified_tools.len(), 1);
        assert!(qualified_tools.contains_key("mcp__server1__duplicate_tool"));
    }

    #[test]
    fn test_qualify_tools_long_names_same_server() {
        let server_name = "my_server";

        let tools = vec![
            create_test_tool(
                server_name,
                "extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
            ),
            create_test_tool(
                server_name,
                "yet_another_extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
            ),
        ];

        let qualified_tools = qualify_tools(tools);

        assert_eq!(qualified_tools.len(), 2);

        let mut keys: Vec<_> = qualified_tools.keys().cloned().collect();
        keys.sort();

        assert_eq!(keys[0].len(), 64);
        assert_eq!(
            keys[0],
            "mcp__my_server__extremel119a2b97664e41363932dc84de21e2ff1b93b3e9"
        );

        assert_eq!(keys[1].len(), 64);
        assert_eq!(
            keys[1],
            "mcp__my_server__yet_anot419a82a89325c1b477274a41f8c65ea5f3a7f341"
        );
    }

    #[test]
    fn test_qualify_tools_sanitizes_invalid_characters() {
        let tools = vec![create_test_tool("server.one", "tool.two")];

        let qualified_tools = qualify_tools(tools);

        assert_eq!(qualified_tools.len(), 1);
        let (qualified_name, tool) = qualified_tools.into_iter().next().expect("one tool");
        assert_eq!(qualified_name, "mcp__server_one__tool_two");

        // The key is sanitized for OpenAI, but we keep original parts for the actual MCP call.
        assert_eq!(tool.server_name, "server.one");
        assert_eq!(tool.tool_name, "tool.two");

        assert!(
            qualified_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "qualified name must be Responses API compatible: {qualified_name:?}"
        );
    }

    #[test]
    fn tool_filter_allows_by_default() {
        let filter = ToolFilter::default();

        assert!(filter.allows("any"));
    }

    #[test]
    fn tool_filter_applies_enabled_list() {
        let filter = ToolFilter {
            enabled: Some(HashSet::from(["allowed".to_string()])),
            disabled: HashSet::new(),
        };

        assert!(filter.allows("allowed"));
        assert!(!filter.allows("denied"));
    }

    #[test]
    fn tool_filter_applies_disabled_list() {
        let filter = ToolFilter {
            enabled: None,
            disabled: HashSet::from(["blocked".to_string()]),
        };

        assert!(!filter.allows("blocked"));
        assert!(filter.allows("open"));
    }

    #[test]
    fn tool_filter_applies_enabled_then_disabled() {
        let filter = ToolFilter {
            enabled: Some(HashSet::from(["keep".to_string(), "remove".to_string()])),
            disabled: HashSet::from(["remove".to_string()]),
        };

        assert!(filter.allows("keep"));
        assert!(!filter.allows("remove"));
        assert!(!filter.allows("unknown"));
    }

    #[test]
    fn filter_tools_applies_per_server_filters() {
        let server1_tools = vec![
            create_test_tool("server1", "tool_a"),
            create_test_tool("server1", "tool_b"),
        ];
        let server2_tools = vec![create_test_tool("server2", "tool_a")];
        let server1_filter = ToolFilter {
            enabled: Some(HashSet::from(["tool_a".to_string(), "tool_b".to_string()])),
            disabled: HashSet::from(["tool_b".to_string()]),
        };
        let server2_filter = ToolFilter {
            enabled: None,
            disabled: HashSet::from(["tool_a".to_string()]),
        };

        let filtered: Vec<_> = filter_tools(server1_tools, server1_filter)
            .into_iter()
            .chain(filter_tools(server2_tools, server2_filter))
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].server_name, "server1");
        assert_eq!(filtered[0].tool_name, "tool_a");
    }

    #[test]
    fn mcp_init_error_display_prompts_for_github_pat() {
        let server_name = "github";
        let entry = McpAuthStatusEntry {
            config: McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://api.githubcopilot.com/mcp/".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                },
                enabled: true,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                startup_mode: None,
                scopes: None,
            },
            auth_status: McpAuthStatus::Unsupported,
        };
        let err: StartupOutcomeError = anyhow::anyhow!("OAuth is unsupported").into();

        let display = mcp_init_error_display(server_name, Some(&entry), &err);

        let expected = format!(
            "GitHub MCP does not support OAuth. Log in by adding a personal access token (https://github.com/settings/personal-access-tokens) to your environment and config.toml:\n[mcp_servers.{server_name}]\nbearer_token_env_var = CODEX_GITHUB_PERSONAL_ACCESS_TOKEN"
        );

        assert_eq!(expected, display);
    }

    #[test]
    fn mcp_init_error_display_prompts_for_login_when_auth_required() {
        let server_name = "example";
        let err: StartupOutcomeError = anyhow::anyhow!("Auth required for server").into();

        let display = mcp_init_error_display(server_name, None, &err);

        let expected = format!(
            "The {server_name} MCP server is not logged in. Run `codex mcp login {server_name}`."
        );

        assert_eq!(expected, display);
    }

    #[test]
    fn mcp_init_error_display_reports_generic_errors() {
        let server_name = "custom";
        let entry = McpAuthStatusEntry {
            config: McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://example.com".to_string(),
                    bearer_token_env_var: Some("TOKEN".to_string()),
                    http_headers: None,
                    env_http_headers: None,
                },
                enabled: true,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                startup_mode: None,
                scopes: None,
            },
            auth_status: McpAuthStatus::Unsupported,
        };
        let err: StartupOutcomeError = anyhow::anyhow!("boom").into();

        let display = mcp_init_error_display(server_name, Some(&entry), &err);

        let expected = format!("MCP client for `{server_name}` failed to start: {err:#}");

        assert_eq!(expected, display);
    }

    #[test]
    fn mcp_init_error_display_includes_startup_timeout_hint() {
        let server_name = "slow";
        let err: StartupOutcomeError = anyhow::anyhow!("request timed out").into();

        let display = mcp_init_error_display(server_name, None, &err);

        assert_eq!(
            "MCP client for `slow` timed out after 10 seconds. Add or adjust `startup_timeout_sec` in your config.toml:\n[mcp_servers.slow]\nstartup_timeout_sec = XX",
            display
        );
    }

    #[tokio::test]
    async fn manifest_cache_round_trips() {
        let codex_home = tempdir().expect("tempdir");
        let cache = ManifestCache {
            servers: HashMap::from([(
                "docs".to_string(),
                CachedServerTools {
                    config_hash: "hash".to_string(),
                    tools: vec![CachedTool {
                        name: "search".to_string(),
                        description: Some("Search docs".to_string()),
                        connector_id: None,
                        connector_name: None,
                    }],
                },
            )]),
        };

        persist_manifest_cache(codex_home.path(), &cache).await;

        let loaded = load_manifest_cache(codex_home.path()).await;
        let server = loaded.servers.get("docs").expect("docs cache entry");
        assert_eq!(server.config_hash, "hash");
        assert_eq!(server.tools.len(), 1);
        assert_eq!(server.tools[0].name, "search");
        assert_eq!(server.tools[0].description.as_deref(), Some("Search docs"));
    }

    #[tokio::test]
    async fn list_all_tools_uses_cached_manifest_for_unready_server() {
        let config = McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: Vec::new(),
                env: None,
                env_vars: Vec::new(),
                cwd: None,
            },
            enabled: true,
            disabled_reason: None,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
            startup_mode: None,
            scopes: None,
        };
        let config_hash = server_config_hash(&config);

        let mut manager = McpConnectionManager::default();
        manager.server_configs.insert("docs".to_string(), config);
        manager.manifest_cache = Arc::new(Mutex::new(ManifestCache {
            servers: HashMap::from([(
                "docs".to_string(),
                CachedServerTools {
                    config_hash,
                    tools: vec![CachedTool {
                        name: "search".to_string(),
                        description: Some("Search docs".to_string()),
                        connector_id: Some("docs".to_string()),
                        connector_name: Some("Docs".to_string()),
                    }],
                },
            )]),
        }));

        let tools = manager.list_all_tools().await;
        let tool = tools.get("mcp__docs__search").expect("cached tool");
        assert_eq!(tool.tool.description.as_deref(), Some("Search docs"));
        assert_eq!(tool.connector_id.as_deref(), Some("docs"));
        assert_eq!(tool.connector_name.as_deref(), Some("Docs"));
    }
}
