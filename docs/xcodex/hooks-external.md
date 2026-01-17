# External hooks (spawn per event)

External hooks are the simplest and most portable hook mode: xcodex **spawns a command per event** and writes the hook payload JSON to stdin.

Use cases:

- Log events to a file (JSONL) for later analysis.
- Post notifications (desktop, Slack/Discord/webhook, email) when approvals are needed or a turn completes.
- Emit metrics (StatsD/OTel/Prometheus push gateway) from lifecycle events.
- Build lightweight automations (e.g. “open files that changed”, “summarize tool calls”, “append to changelog”).

If you want **stateful** Python hooks or very high throughput, prefer Python Host (“py-box”) hooks: `docs/xcodex/hooks-python-host.md`.

If you want Python to run **in-process** (no JSON parsing per event, no IPC), see PyO3 hooks: `docs/xcodex/hooks-pyo3.md`.

## Quickstart

1) Generate a starter config snippet:

```sh
xcodex hooks init external
```

2) Install sample hook scripts into your active `CODEX_HOME`:

```sh
xcodex hooks install samples external
```

3) (Optional) Install typed SDK helpers/templates:

```sh
xcodex hooks install sdks python
xcodex hooks install sdks js
xcodex hooks install sdks rust
```

4) Smoke-test your configured hooks without running a full session:

```sh
xcodex hooks test external --configured-only
```

5) Check logs / payload dumps:

```sh
xcodex hooks paths
```

## Configuration

The authoritative reference is `docs/config.md#hooks`.

In short:

- Prefer `hooks.command` (new matcher-based config); legacy `[hooks]` argv arrays are still supported.
- External hooks receive either:
  - the full payload JSON on stdin (small payloads), or
  - a small JSON envelope containing `payload_path` (large payloads).

### Minimal config.toml examples

Legacy `[hooks]` (argv arrays):

```toml
[hooks]
agent_turn_complete = [["python3", "/absolute/path/to/hook.py"]]
tool_call_finished = [["python3", "/absolute/path/to/hook.py"]]
```

Recommended `hooks.command` (matchers + per-hook options):

```toml
[hooks.command]
default_timeout_sec = 30

[[hooks.command.tool_call_finished]]
matcher = "write_file|edit_block"
  [[hooks.command.tool_call_finished.hooks]]
  argv = ["python3", "hooks/tool_call_summary.py"]
  timeout_sec = 10
```

## Payload + events

See:

- `docs/xcodex/hooks.md` for payload shape, supported events, and the stdin/file fallback envelope.
- `docs/xcodex/hooks.schema.json` for a machine-readable schema bundle.
- `docs/xcodex/hooks-gallery.md` for copy/paste scripts.
