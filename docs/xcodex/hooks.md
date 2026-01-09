# xcodex hooks (automation)

xcodex includes a basic “automation hooks” system: Codex can spawn external programs on a small set of lifecycle events and pass a JSON payload to them.

This is intended for **notifications and integrations** (fire-and-forget). Hooks do not block the run if they fail.

This document is an overview; the authoritative reference is `docs/config.md#hooks`.

Notes on naming:

- This fork installs the binary as `xcodex`.
- Config keys are snake_case (e.g. `hooks.session_start`); payload `"type"` is kebab-case (e.g. `session-start`).

## What you need to use hooks

No additional commands are required.

1. Ensure you have a `$CODEX_HOME/config.toml` (for `xcodex` with no `CODEX_HOME`, this is `~/.xcodex/config.toml`)
2. Add a `[hooks]` section (examples below)

Notes:

- Hooks are configured in your active Codex home (`$CODEX_HOME/config.toml`).
- `xcodex` uses `$CODEX_HOME` too. When `CODEX_HOME` is unset, `xcodex` defaults to `~/.xcodex` (and upstream `codex` defaults to `~/.codex`), so hooks you configure via xcodex won’t affect upstream codex by default.
- If you *want* to share hooks/config with upstream codex, explicitly set `CODEX_HOME=~/.codex` before running `xcodex`.

If you want to disable all hooks (external and in-process) for a single run, pass `--no-hooks`:

```sh
xcodex --no-hooks
xcodex exec --no-hooks "…"
```

## Quickstart (copy/paste)

See also:

- Hooks gallery: `docs/xcodex/hooks-gallery.md`
- Ready-to-use scripts: `examples/hooks/`

If you prefer scaffolding, run:

```sh
xcodex hooks init
```

That creates a small set of example scripts under `$CODEX_HOME/hooks/` and prints a config snippet you can paste into `$CODEX_HOME/config.toml`.

To install language helpers/templates for writing typed external hooks, run:

```sh
xcodex hooks install --list
xcodex hooks install python
```

To see a quick overview of hooks commands and SDKs, run:

```sh
xcodex hooks help
```

See `docs/xcodex/hooks-sdks.md` for details.

In the interactive TUI/TUI2, you can type `/hooks` to see a quick reminder of the relevant `xcodex hooks ...` commands and where logs/payloads are written under `$CODEX_HOME`.

1. Copy a script into your Codex home:

```sh
mkdir -p "${CODEX_HOME:-$HOME/.xcodex}/hooks"
cp examples/hooks/log_all_jsonl.py "${CODEX_HOME:-$HOME/.xcodex}/hooks/"
```

2. Add a `[hooks]` entry to `$CODEX_HOME/config.toml` (use an absolute path):

```toml
[hooks]
agent_turn_complete = [["python3", "/absolute/path/to/log_all_jsonl.py"]]
```

Optional (experimental, xcodex): enable a built-in in-process (Rust) hook that logs compact `tool_call_finished` summaries:

```toml
[hooks]
inproc = ["tool_call_summary"]
```

For backward compatibility, you can also enable the same hook via:

```toml
[hooks]
inproc_tool_call_summary = true
```

This appends one line per tool call to `$CODEX_HOME/hooks-tool-calls.log` (same format as `examples/hooks/tool_call_summary.py`).
This is disabled by `xcodex --no-hooks`.

Optional (experimental, xcodex): enable a built-in in-process (Rust) hook that logs every hook payload to JSONL:

```toml
[hooks]
inproc = ["event_log_jsonl"]
```

This appends one JSON object per hook event to `$CODEX_HOME/hooks.jsonl` (same format as `examples/hooks/log_all_jsonl.py`).
This is disabled by `xcodex --no-hooks`.

Note: `hooks.jsonl` is not automatically rotated or pruned; manage it externally if you enable this long-term.

## Rust hooks (experimental)

External command hooks remain the supported, user-facing hooks system in xcodex. They are language-agnostic and are expected to stay supported long-term.

In addition, codex-core exposes an **in-process Rust hook API** (observer-only) under `codex_core::hooks`:

- `HookHandler` + `HookContext` for subscribing to events.
- `HookPayload` + `HookNotification` for receiving typed event data (these serialize to the same JSON shape as external hooks).

Why Rust/PyO3 hooks?

- Typed events (no JSON schema drift between producer/consumer).
- Stateful hooks (retain per-session state across events).
- Avoid per-event process spawn overhead.
- Potential PyO3 bridge for “Python hooks” while still using the Rust hook API.

Security note: in-process hooks run inside the Codex process and should be treated as trusted code (they have the same access as Codex itself). Use external hooks if you need a safer / more isolated integration.

3. Test it:

```sh
xcodex hooks test --configured-only
```

That command prints where hook logs and payload files were written under `$CODEX_HOME`.

To inspect your current hook configuration, run:

```sh
xcodex hooks list
```

To print where hook logs and payloads are written, run:

```sh
xcodex hooks paths
```

### Confirm-on-exit while hooks are running

In the interactive TUI, quitting while hooks are still running prompts for confirmation by default.

Toggle with:

```toml
[tui]
confirm_exit_with_running_hooks = false
```

## Testing your hooks

To exercise your configured hook commands without running a full session, use:

```sh
xcodex hooks test
```

This invokes your configured hook command(s) with synthetic payloads for the supported event types and prints a short summary (including where hook logs and payload files were written under `CODEX_HOME`).

Useful flags:

- `--configured-only` to run only events that have hook commands configured.
- `--event <event>` (repeatable) to select specific events, e.g. `--event approval-requested-exec`.
- `--timeout-seconds <n>` to cap how long each hook command is allowed to run.

## Supported events

- `agent_turn_complete`: runs after each completed turn
- `approval_requested`: runs when Codex asks for approval (exec / apply-patch / MCP elicitation)
- `session_start`: runs when a session starts (after `SessionConfigured`)
- `session_end`: runs when a session ends (best-effort during shutdown)
- `model_request_started`: runs immediately before issuing a model request
- `model_response_completed`: runs after a model response completes
- `tool_call_started`: runs when a tool call begins execution
- `tool_call_finished`: runs when a tool call finishes (success/failure/aborted)

Note: `tool_call_started` is emitted when the tool call is dispatched; `duration-ms` in `tool_call_finished` includes any time spent queued behind non-parallel tool calls.

Each hook command receives event JSON on **stdin**.

The payload always includes (at minimum):

- `schema-version`: currently `1`
- `type`: kebab-case event type (for example `agent-turn-complete`, `model-request-started`, `tool-call-finished`)
- `event-id`: unique id for the event
- `timestamp`: RFC3339 timestamp

Notes:

- `cwd` is the relevant working directory for the event. For exec approvals it is the command’s working directory; for most other events it is the session working directory.

## Configuration

Hooks are configured as argv arrays:

Note: hook command paths are treated as literal argv entries (no shell expansion), so prefer absolute paths.

```toml
[hooks]
agent_turn_complete = [
  ["python3", "/Users/alice/.xcodex/hooks/turn_complete.py"],
]

approval_requested = [
  ["python3", "/Users/alice/.xcodex/hooks/approval.py"],
]
```

### Long-lived hook host (experimental)

In addition to per-event external hooks (`[hooks]`), xcodex can stream hook events to a single **long-lived hook host** process (`[hooks.host]`).

This is intended for stateful hooks in languages like Python without per-event process spawn overhead. The host is **observer-only**: failures never block or fail a run.

Install the reference Python host:

```sh
xcodex hooks install python
```

That installs:

- `$CODEX_HOME/hooks/host/python/host.py` (the host runner)
- `$CODEX_HOME/hooks/host/python/example_hook.py` (an example `on_event(event: dict)` hook module)

To run it manually (for quick debugging):

```sh
python3 -u "$CODEX_HOME/hooks/host/python/host.py" "$CODEX_HOME/hooks/host/python/example_hook.py"
```

To enable it in `$CODEX_HOME/config.toml`:

```toml
[hooks.host]
enabled = true

# NOTE: This is argv (no shell expansion). Paths are resolved relative to $CODEX_HOME,
# because the host process is spawned with cwd=$CODEX_HOME.
command = ["python3", "-u", "hooks/host/python/host.py", "hooks/host/python/example_hook.py"]

# Optional: when unset, inherits the session sandbox policy.
# sandbox_mode = "read-only" | "workspace-write" | "danger-full-access"
```

Notes:

- `xcodex --no-hooks` disables both external hooks and the hook host.
- Host stdout/stderr are written to `$CODEX_HOME/tmp/hooks/host/logs/` (see also `xcodex hooks paths`).

#### Protocol + lifecycle contract (v1)

- Framing: Codex writes **one JSON object per line** to the host process stdin.
- Event messages:
  - `{"schema-version":1,"type":"hook-event","seq":123,"event":{...}}`
  - `event` is the same hook payload schema used for external hooks (see “Supported events” and `docs/xcodex/hooks.schema.json`).
- Fire-and-forget: Codex does not wait for acks; the host must not assume delivery guarantees.
- Backpressure: the host input queue is bounded; when full, Codex drops events (best-effort) and continues.
- Failures: if the host crashes or stdin breaks, Codex kills/restarts it on demand with a circuit breaker for repeated failures.

### Example: enable all events

```toml
[hooks]
session_start = [["python3", "/path/to/hook.py"]]
session_end = [["python3", "/path/to/hook.py"]]
model_request_started = [["python3", "/path/to/hook.py"]]
model_response_completed = [["python3", "/path/to/hook.py"]]
tool_call_started = [["python3", "/path/to/hook.py"]]
tool_call_finished = [["python3", "/path/to/hook.py"]]
agent_turn_complete = [["python3", "/path/to/hook.py"]]
approval_requested = [["python3", "/path/to/hook.py"]]
```

### Payload delivery (stdin + file fallback)

For small payloads, Codex writes the full JSON payload to stdin.

For large payloads, Codex writes the full payload JSON to a file under CODEX_HOME and writes a small JSON envelope to stdin containing `payload-path`. Hook scripts should handle both cases.

Payload/log files are kept under CODEX_HOME and pruned with a global keep-last-N policy.
While the TUI is running, hook stdout/stderr are redirected to log files under CODEX_HOME so hooks do not corrupt the terminal UI.

Config knobs:

```toml
[hooks]
# Defaults shown.
max_stdin_payload_bytes = 16384
keep_last_n_payloads = 50
```

### Compatibility policy (payload schema)

This policy applies to:

- Hook authors (scripts you run via `[hooks]`)
- “Typed hook SDKs” (helpers/templates/types that parse the hook JSON)

`schema-version` is currently `1`.

We will only bump `schema-version` for **breaking** changes. Additive fields and new event types should not require a bump.

Machine-readable schema (generated from the Rust source of truth):

- `docs/xcodex/hooks.schema.json`

#### Forward compatibility requirements

External hook consumers should be **forward compatible**:

- Treat unknown fields as ignorable.
- Treat unknown `"type"` values as ignorable (log/debug if you want), so new event types don’t break older hooks.
- Prefer `.get(...)` / optional access patterns and safe defaults (missing fields must not crash the hook).
- Always handle both delivery modes: full payload JSON on stdin **or** a small envelope with `payload-path`.

If you are writing a “typed SDK”, prefer designs that preserve unknown data:

- Include an `extras` / `additional_fields` map on typed events (so unknown fields can be round-tripped or inspected).
- Include an `Unknown`/`Other` event variant that holds the raw payload when `"type"` is not recognized.

#### What counts as “breaking”

The following are considered **breaking** and require a `schema-version` bump:

- Removing a field that was previously present.
- Changing a field’s type in a way that cannot be interpreted as the old type.
- Changing field semantics such that existing hooks would produce wrong behavior (even if parsing still succeeds).
- Making an optional field effectively required for correct interpretation of an event.

The following should be **non-breaking** (no bump):

- Adding new optional fields.
- Adding new event types (`"type": "..."`).
- Adding new values to existing enum-like string fields (as long as old consumers can ignore/handle unknown values).

#### How to use `schema-version` as a consumer

Consumers should not fail solely because `schema-version` is higher than expected. Instead:

- Parse what you understand.
- Ignore what you don’t.
- If you depend on specific fields, validate those fields directly (presence/type) rather than gating purely on `schema-version`.

## Example: log turn summaries to a file

Create `$CODEX_HOME/hooks/turn_complete.py`:

```python
#!/usr/bin/env python3
import json
import os
import pathlib
import sys

stdin_payload = sys.stdin.read()
payload = json.loads(stdin_payload)
payload_path = payload.get("payload-path")
if payload_path:
    payload = json.loads(pathlib.Path(payload_path).read_text())
out = pathlib.Path(os.environ["CODEX_HOME"]) / "hooks.log"
out.parent.mkdir(parents=True, exist_ok=True)

line = f"{payload.get('type')} cwd={payload.get('cwd')} last={payload.get('last-assistant-message')!r}\n"
out.write_text(out.read_text() + line if out.exists() else line)
```

Then wire it up in `$CODEX_HOME/config.toml` as shown above.

## Example: log all hook payloads (JSONL)

Create `$CODEX_HOME/hooks/log_all.py`:

```python
#!/usr/bin/env python3
import json
import os
import pathlib
import sys

payload = json.loads(sys.stdin.read() or "{}")
payload_path = payload.get("payload-path")
if payload_path:
    payload = json.loads(pathlib.Path(payload_path).read_text())

out = pathlib.Path(os.environ["CODEX_HOME"]) / "hooks.jsonl"
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text((out.read_text() if out.exists() else "") + json.dumps(payload) + "\n")
```

Then configure:

```toml
[hooks]
session_start = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
session_end = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
model_request_started = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
model_response_completed = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
tool_call_started = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
tool_call_finished = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
agent_turn_complete = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
approval_requested = [["python3", "/Users/alice/.xcodex/hooks/log_all.py"]]
```

## Example: approval notifications

When an approval is requested, the payload includes:

- `kind`: `exec`, `apply-patch`, or `elicitation`
- `command` (for exec approvals)
- `approval-policy` and `sandbox-policy` (for exec + apply-patch approvals)
- `proposed-execpolicy-amendment` (for exec approvals, when available)
- `paths` / `grant-root` (for apply_patch approvals)
- `server-name` / `request-id` / `message` (for MCP elicitation)

Use this to route alerts (macOS notification, Slack webhook, etc.).

## Making hooks easier to adopt (ideas)

To get users using hooks with minimal effort, we could add:

- A `xcodex hooks init` command that writes:
  - `$CODEX_HOME/hooks/` example scripts
  - a commented-out `[hooks]` section in `$CODEX_HOME/config.toml`
- A `just hooks-install` recipe that installs the examples into `$CODEX_HOME/hooks/`
- A small “marketplace” of prebuilt hook scripts in-repo (loggers, notifiers, memory capture)
- A richer “hooks test” UX (for example, selecting events interactively and previewing payload JSON)

## Avoiding recursion

If a hook script runs `xcodex exec` (for example to do background processing after a turn completes), use `xcodex exec --no-hooks ...` so the child run does not re-trigger hooks.

## Related docs

- `docs/config.md#hooks` (authoritative config reference)

## `notify` (deprecated)

The legacy `notify` config is deprecated; use `hooks.agent_turn_complete` instead.
