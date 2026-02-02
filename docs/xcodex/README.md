# xcodex (xtreme-codex)

xcodex (xtreme-codex) is an effort to add features to upstream Codex CLI.

## Status

- Slash commands: `/help xcodex`, `/status`, `/settings`, `/compact`, `/autocompact`, `/thoughts`, `/worktree`, `/theme`, and `/mcp` are working.
- Background terminals: `/ps` lists running background terminals and hooks; `/ps-kill` can terminate background terminals.
- Hooks: 3 levels of automation hooks (external, Python Host “py-box”, and PyO3 in-proc) are in place.
- Other features are in progress; expect rough edges and some churn.

## What’s here

- Settings: `docs/xcodex/settings.md`
- Worktrees: `docs/xcodex/worktrees.md` (quickstart + shared-dirs contract at the top)
- Hooks: `docs/xcodex/hooks.md`
- External hooks: `docs/xcodex/hooks-external.md`
- Python Host hooks (“py-box”): `docs/xcodex/hooks-python-host.md`
- PyO3 hooks (in-proc; separate build): `docs/xcodex/hooks-pyo3.md`
- Hooks gallery: `docs/xcodex/hooks-gallery.md`
- Keeping context under control: `docs/xcodex/compact.md`
- Hiding thoughts: `docs/xcodex/thoughts.md`
- Background terminals: `docs/xcodex/background-terminals.md`
- Announcements (startup tips): `docs/xcodex/announcements.md`
- Themes: `docs/xcodex/themes.md` (start here) and `docs/xcodex/themes-mbadolato.md` (built-in catalog details)
- Lazy MCP loading: `docs/xcodex/lazy-mcp-loading.md`

## Local install (as `xcodex`)

To build the current working tree and install it as `xcodex` locally:

```sh
just xcodex-install

# Default: local Bazel build (no BuildBuddy/remote cache).
# Opt into remote cache/BEP: just xcodex-install --remote
# Avoid all network fetches (requires deps already cached): just xcodex-install --offline
```

This installs to `~/.local/bin/xcodex` by default.

## Npm packages

`xcodex` and the responses API proxy are distributed as npm packages that bundle prebuilt native binaries:

- Target packages:
  - `@eriz1818/xcodex` → `xcodex`
  - `@eriz1818/xcodex-responses-api-proxy` → `xcodex-responses-api-proxy`

Install:

```sh
npm i -g @eriz1818/xcodex
npm i -g @eriz1818/xcodex-responses-api-proxy
```

Prereleases are published under the `alpha` dist-tag:

```sh
npm i -g @eriz1818/xcodex@alpha
```

Local smoke test (once vendor binaries are staged into the package tarballs):

```sh
cd codex-cli
npm pack
npm i -g ./eriz1818-xcodex-*.tgz
xcodex --version

cd ../codex-rs/responses-api-proxy/npm
npm pack
npm i -g ./eriz1818-xcodex-responses-api-proxy-*.tgz
xcodex-responses-api-proxy --help
```

## Coexisting with upstream `codex`

- Home directory: when invoked as `xcodex` and `CODEX_HOME` is unset, the default home is `~/.xcodex` (upstream `codex` remains `~/.codex`).
- MCP OAuth tokens: `xcodex` stores keyring-backed MCP OAuth tokens under a separate keyring service name from upstream `codex`, so refreshing/deleting tokens in `xcodex` won’t impact upstream `codex`.
- Hooks: hooks are configured under `$CODEX_HOME/config.toml`; with separate default homes, hooks you configure/run via `xcodex` won’t affect upstream `codex` unless you intentionally set `CODEX_HOME=~/.codex`.

## Config commands

Quick helpers for finding and editing your config:

- `xcodex config path`: prints `CODEX_HOME`, `$CODEX_HOME/config.toml`, and any in-repo `.codex/config.toml` layers that apply to the current working directory.
- `xcodex config edit`: opens `$CODEX_HOME/config.toml` in `$VISUAL`/`$EDITOR` (or prints the path if no editor is set).
  - `--project` edits the nearest `./.codex/config.toml` instead (project-local config for the current repo).
- `xcodex config doctor`: validates config parsing and reports common issues like unknown keys.

## First run setup wizard

On first interactive run, `xcodex` may prompt to initialize its own home directory. You can:

- Start from scratch.
- Copy all from an existing codex home (includes sensitive data like `.credentials.json` and `history.jsonl`).
- Select what you copy from an existing codex home (scan-derived checklist with a “Select all” option).

`xcodex` does not overwrite existing destination files during the copy step.

Non-interactive `xcodex exec` requires first-run setup to be complete; if setup is missing, it fails fast and tells you to run `xcodex` once (or set `CODEX_HOME` to an initialized directory).

## Troubleshooting

### Re-run the setup wizard

The setup wizard runs when both of these are true:

- `$CODEX_HOME/config.toml` does not exist
- `$CODEX_HOME/.xcodex-first-run-wizard.complete` does not exist

To re-run the wizard without deleting anything, rename the directory (recommended), then start `xcodex` again:

```sh
mv "${CODEX_HOME:-$HOME/.xcodex}" "${CODEX_HOME:-$HOME/.xcodex}.bak"
xcodex
```

If you prefer a targeted reset, remove (or rename) both files in your xcodex home and rerun `xcodex`:

```sh
mv "${CODEX_HOME:-$HOME/.xcodex}/config.toml" "${CODEX_HOME:-$HOME/.xcodex}/config.toml.bak"
mv "${CODEX_HOME:-$HOME/.xcodex}/.xcodex-first-run-wizard.complete" "${CODEX_HOME:-$HOME/.xcodex}/.xcodex-first-run-wizard.complete.bak"
xcodex
```

### Use a fresh home directory

If you want to keep your current xcodex home intact, point `CODEX_HOME` at a new directory:

```sh
export CODEX_HOME="$HOME/.xcodex-new"
xcodex
```
