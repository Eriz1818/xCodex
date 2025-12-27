# Keeping context under control (`/compact`, `/autocompact`)

xcodex extends upstream Codex with two slash commands to keep long sessions usable:

- `/compact`: compacts the current conversation immediately.
- `/autocompact`: enables automatic compaction near the model’s context limit.

## `/compact`

Use this when the conversation is getting long and you want to keep going without starting a new session.

## `/autocompact`

`/autocompact` supports `on`, `off`, `toggle`, and `status` and persists across sessions.

Examples:

```text
/autocompact on
/autocompact status
/autocompact toggle
```

## Related docs

- `docs/slash_commands.md` (slash command reference)
- `docs/config.md` (config reference; includes the persisted setting)
