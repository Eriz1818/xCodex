# xcodex (xtreme-codex)

xcodex (xtreme-codex) is an effort to add features to upstream Codex CLI.

## Status

- Slash commands: `/compact`, `/autocompact`, and `/thoughts` are working.
- Background terminals: `/ps` lists running background terminals and hooks; `/ps-kill` can terminate background terminals.
- Hooks: basic automation hooks are in place.
- Other features are in progress; expect rough edges and some churn.

## What’s here

- Hooks: `docs/xcodex/hooks.md`
- Keeping context under control: `docs/xcodex/compact.md`
- Hiding thoughts: `docs/xcodex/thoughts.md`
- Background terminals: `docs/xcodex/background-terminals.md`

## Local install (as `xcodex`)

To build the current working tree and install it as `xcodex` locally:

```sh
just xcodex-install
```

This installs to `~/.local/bin/xcodex` by default.
