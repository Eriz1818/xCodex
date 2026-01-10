use anyhow::Result;
use app_test_support::MCP_CLIENT_NAME;
use app_test_support::MCP_CLIENT_VERSION;
use app_test_support::McpProcess;
use app_test_support::to_response;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::GetUserAgentResponse;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_user_agent_returns_current_codex_user_agent() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut mcp = McpProcess::new(codex_home.path()).await?;
    let initialized = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.initialize_with_client_info(ClientInfo {
            name: MCP_CLIENT_NAME.to_string(),
            title: None,
            version: MCP_CLIENT_VERSION.to_string(),
        }),
    )
    .await??;
    let initialized = match initialized {
        codex_app_server_protocol::JSONRPCMessage::Response(response) => response,
        other => anyhow::bail!("Expected initialize response, got {other:?}"),
    };
    let initialized: InitializeResponse = to_response(initialized)?;

    let request_id = mcp.send_get_user_agent_request().await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;

    let received: GetUserAgentResponse = to_response(response)?;
    let expected = GetUserAgentResponse {
        user_agent: initialized.user_agent,
    };

    assert_eq!(received, expected);
    Ok(())
}
