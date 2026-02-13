# Plan mode and durable plans

`xcodex` supports plan-first workflows with a durable markdown plan file.

## In the TUI

- `Shift+Tab`: toggle Plan mode on/off.
- `/plan`: open the Plan popup (list + settings + plan actions).
- In Plan mode, plain composer messages are treated as planning prompts.
- `/plan settings mode <default|adr-lite|custom [default|adr-lite]>`: choose workflow defaults.
- `/plan settings mode custom <default|adr-lite>`: switch to custom mode and create the custom template if missing using the chosen seed.
- `/plan settings custom-template`: view/open/init the custom template file.

## CLI plan commands

Use these commands outside the TUI:

```sh
xcodex plan status
xcodex plan list [open|closed|all|archived]
xcodex plan open [PATH]
xcodex plan done
xcodex plan archive
```

Behavior:

- `status`: prints plan base directory, active plan path, and active plan metadata.
- `list`: lists plan files under the base directory filtered by status.
- `open`: creates or selects the active plan file. If `PATH` is omitted, `xcodex` creates a new plan file under `<base_dir>/<project_name>/<funny-name>.md`.
- `done`: updates `Status: Done` on the active plan file.
- `archive`: updates `Status: Archived` on the active plan file.

List scope behavior:

- `open`: `Draft|Active|Paused`
- `closed`: `Done`
- `all`: all non-archived plans
- `archived`: `Archived`

## Default storage location

By default, plan files live under:

```text
$CODEX_HOME/plans
```

The active plan pointer is persisted in:

```text
$CODEX_HOME/plans/.active-plan
```

## Gitignore behavior for in-repo plan directories

- Default behavior (no custom base dir): no gitignore prompt is shown because plans live under `$CODEX_HOME/plans`.
- If you set `/plan settings base-dir` to a directory inside a git worktree and that directory does not appear ignored, xcodex shows a one-time reminder to add an ignore rule.
- Example ignore rule:

```gitignore
plans/
```

## Config

You can set `/plan` defaults in `config.toml`:

```toml
[plan]
base_dir = "/absolute/path/to/plans"
mode = "default"         # default | adr-lite | custom
custom_template = "/absolute/path/to/template.md"
custom_seed_mode = "adr-lite" # default | adr-lite
```

Defaults when unset:

- `base_dir`: `$CODEX_HOME/plans`
- `mode`: `default`
- `custom_template`: `$CODEX_HOME/plans/custom/template.md`
- `custom_seed_mode`: `adr-lite`

State files under `$CODEX_HOME/plans` override config values when set via `/plan settings`.

## Safety semantics

- Plan approval does not bypass sandbox or approval policies.
- In Plan mode, mutation tools are guarded; execution still follows normal approval rules.

## Deferred TODOs

- `request_user_input` question types/cardinality:
  add explicit question type support in protocol/schema/UI for `single-choice`, `multi-choice`, and `freeform` (for example via a `selection_mode` field and optional min/max selections). Keep this deferred for now; current options behavior is single-choice.

## Troubleshooting

- See `docs/xcodex/plan-troubleshooting.md`.
