use crate::chatwidget::ChatWidget;
use crate::slash_command::SlashCommand;
use codex_core::protocol::Op;

use super::PluginSubcommandNode;
use super::PluginSubcommandRoot;

const MCP_RETRY_CHILDREN: &[PluginSubcommandNode] = &[PluginSubcommandNode {
    token: "failed",
    full_name: "mcp retry failed",
    description: "retry MCP servers that failed to start",
    run_on_enter: true,
    insert_trailing_space: false,
    children: &[],
}];

const MCP_SUBCOMMANDS: &[PluginSubcommandNode] = &[
    PluginSubcommandNode {
        token: "retry",
        full_name: "mcp retry",
        description: "retry MCP servers (use 'failed' or a name)",
        run_on_enter: false,
        insert_trailing_space: true,
        children: MCP_RETRY_CHILDREN,
    },
    PluginSubcommandNode {
        token: "timeout",
        full_name: "mcp timeout",
        description: "set startup timeout for an MCP server",
        run_on_enter: false,
        insert_trailing_space: true,
        children: &[],
    },
];

pub(crate) const MCP_SUBCOMMAND_ROOT: PluginSubcommandRoot = PluginSubcommandRoot {
    root: "mcp",
    anchor: SlashCommand::Mcp,
    children: MCP_SUBCOMMANDS,

    list_hint_order: None,
};

pub(crate) fn try_handle_subcommand(chat: &mut ChatWidget, args: &[&str]) -> bool {
    match args {
        ["retry"] | ["retry", "failed"] => {
            if chat.mcp_failed_servers().is_empty() {
                chat.add_info_message("No failed MCP servers to retry.".to_string(), None);
            } else {
                let servers = chat.mcp_failed_servers().to_vec();
                chat.clear_mcp_startup_banner();
                chat.submit_op(Op::McpRetry { servers });
            }
            true
        }
        ["retry", server] => {
            chat.clear_mcp_startup_banner();
            chat.submit_op(Op::McpRetry {
                servers: vec![(*server).to_string()],
            });
            true
        }
        ["timeout", server, seconds] => {
            let secs: u64 = match seconds.parse() {
                Ok(secs) => secs,
                Err(_) => {
                    chat.add_info_message("Usage: /mcp timeout <name> <seconds>".to_string(), None);
                    return true;
                }
            };
            let server = (*server).to_string();
            chat.clear_mcp_startup_banner();
            chat.persist_mcp_startup_timeout(server.clone(), secs);
            chat.submit_op(Op::McpSetStartupTimeout {
                server: server.clone(),
                startup_timeout_sec: secs,
            });
            chat.submit_op(Op::McpRetry {
                servers: vec![server],
            });
            true
        }
        _ => false,
    }
}
