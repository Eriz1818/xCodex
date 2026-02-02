# Lazy MCP loading

MCP (Model Context Protocol) servers let xcodex talk to external tools and services.
By default, Codex starts enabled MCP servers at session start, but xcodex also supports
deferring startup to improve responsiveness and avoid starting servers you never use.

This doc explains:

- What “startup modes” mean (`eager`, `lazy`, `manual`)
- What config options are new/important
- How to control MCP startup from the TUI (`/mcp ...`)

For the full configuration reference, see `docs/config.md#mcp_startup_mode` and
`docs/config.md#mcp_servers`.

## Startup modes

Startup mode can be set globally (default for all servers) and overridden per server.

### eager (default)

- Starts all enabled MCP servers at session start.
- Best when you always use MCP and want tool discovery to be ready immediately.

### lazy

- Does not start servers at session start.
- Starts a server automatically the first time a tool/resource from that server is needed.
- Best when you want fast startup and only occasionally use MCP.

### manual

- Never auto-starts servers.
- You must explicitly start servers before use (for example via `/mcp load <name>`).
- Best for deterministic workflows where you only want MCP started on command.

## Configuration

### Global default (`mcp_startup_mode`)

In `$CODEX_HOME/config.toml`:

```toml
# eager (default) | lazy | manual
mcp_startup_mode = "lazy"
```

### Per-server overrides

Each server can override the global default:

```toml
[mcp_servers.docs]
command = "docs-server"
args = ["--port", "4000"]

# Override the global default for this server only.
startup_mode = "manual"
```

### Common per-server options

These are useful when you start using `lazy` or `manual` because they help you tune
startup behavior and reduce failure modes:

```toml
[mcp_servers.my_server]
command = "npx"
args = ["-y", "mcp-server"]

# Control timeouts.
startup_timeout_sec = 20
tool_timeout_sec = 60

# Enable/disable quickly.
enabled = true

# Optional allow/deny lists.
enabled_tools = ["search", "summarize"]
disabled_tools = ["slow-tool"]
```

### CLI override (one run)

You can override the startup mode without changing config:

```bash
xcodex --mcp-startup-mode lazy
```

Supported values are `eager`, `lazy`, and `manual`.

## TUI usage (`/mcp`)

xcodex exposes MCP management commands in the TUI:

```text
/mcp
/mcp load <name>
/mcp retry [failed|<name>]
/mcp timeout <name> <seconds>
```

Notes:

- `/mcp` shows configured servers, their startup status, and available tools (when known).
- `/mcp load <name>` explicitly starts a server (useful for `manual` mode, or to pre-warm in `lazy` mode).
- `/mcp retry ...` restarts failed servers without restarting the session.
- `/mcp timeout ...` persists a new `startup_timeout_sec` for the server and retries it immediately.

When an MCP server fails to start, the UI may also suggest retrying (for example, press `r` when idle or run `/mcp retry failed`).

## Practical recipes

### Fast startup, only load what you use

```toml
mcp_startup_mode = "lazy"
```

### Keep most servers eager, but defer one slow server

```toml
mcp_startup_mode = "eager"

[mcp_servers.big_server]
command = "big-mcp-server"
startup_mode = "lazy"
startup_timeout_sec = 60
```

### Fully manual control

```toml
mcp_startup_mode = "manual"
```

Then use `/mcp load <name>` when you actually want a server running.

## Troubleshooting

If a server is not available:

- Run `/mcp` and check whether the server is enabled and whether it failed to start.
- If it failed, try `/mcp retry <name>` or `/mcp retry failed`.

If startup is slow or timing out:

- Increase the timeout with `/mcp timeout <name> <seconds>` (this persists to config), or edit `startup_timeout_sec` directly.

If tools are missing:

- Check `enabled_tools` / `disabled_tools` in `config.toml`.
- Remember: in `lazy`/`manual`, a server may need to be started before its tool list can be discovered.

