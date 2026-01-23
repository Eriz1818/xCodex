# Built-in theme catalog (mbadolato iTerm2 Color Schemes)

Codex’s built-in theme catalog is generated from the upstream “iTerm2 Color Schemes” collection by Mark Badolato:

- Upstream project: https://github.com/mbadolato/iTerm2-Color-Schemes
- Upstream license: MIT (see upstream `LICENSE`)
  - Note: upstream also states that “the copyright/license for each individual theme belongs to the author of that theme.”

## Source format used

We ingest the `wezterm/*.toml` theme files from the upstream collection (these include:

- 8 `ansi` colors + 8 `brights` colors (ANSI 0–15)
- `foreground`, `background`
- `cursor_bg`, `cursor_fg`
- `selection_bg`, `selection_fg`

These are converted into xcodex-native `ThemeDefinition` entries (with required `palette.*` and `roles.*` fields populated).

## Where the generated bundle lives

- `codex-rs/core/src/themes/mbadolato_builtins.json`

This file is embedded into the `xcodex` binary via `include_bytes!` and parsed at runtime into the built-in theme catalog.

## How to regenerate

From `codex-rs/`:

```sh
cargo run -p codex-core --bin import_mbadolato_themes -- \
  /path/to/mbadolato-iTerm2-Color-Schemes/wezterm \
  codex-rs/core/src/themes/mbadolato_builtins.json
```
