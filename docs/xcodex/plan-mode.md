# Plan mode quickstart

Use Plan mode when you want `xcodex` to work in a planning-first loop.

## Fast start

- Press `Shift+Tab` to toggle Plan mode on/off.
- Run `/plan` to open the Plan menu (status, settings, and actions).
- In Plan mode, normal composer messages are treated as planning prompts.

## Workflow modes

Choose one of:

- `default`: Codex-style planning flow.
- `adr-lite`: ADR-style structure.
- `custom`: your own template.

You can set mode from `/plan settings mode ...`.

## Durable plans

Plan files are persisted and can be managed from CLI:

```sh
xcodex plan status
xcodex plan list [open|closed|all|archived]
xcodex plan open [PATH]
xcodex plan done
xcodex plan archive
```

## More details

- Full reference: `docs/xcodex/plan.md`
- Troubleshooting: `docs/xcodex/plan-troubleshooting.md`
