# Themes

xcodex supports configurable UI themes:

- A **built-in theme catalog** (so theming works out of the box).
- A **local theme directory** (`$CODEX_HOME/themes`) for custom YAML themes.
- An **interactive picker** via `/theme` inside the TUI.

This page is the “how to use it” guide. For the full config reference and theme file format, see `docs/config.md#themes`.

## Quickstart

1) Open the theme picker:

- Run `/theme` in the TUI.

2) Pick a theme and preview it live:

- Navigate the list and select a theme to apply it.

3) Customize and save (optional):

- Use the editor controls inside `/theme` to adjust `roles.*` / `palette.*`, then save a copy into your theme directory.

## Where themes live

By default, xcodex uses:

- `CODEX_HOME`: `~/.xcodex` (when invoked as `xcodex` and `CODEX_HOME` is unset)
- Theme directory: `$CODEX_HOME/themes`

You can override the theme directory in config (see `docs/config.md#themes`).

## Built-in theme catalog

xcodex ships with built-in themes, so `/theme` and `[themes]` work even if your theme directory is empty.

The primary built-in catalog is generated from Mark Badolato’s “iTerm2 Color Schemes” collection. Details:

- `docs/xcodex/themes-mbadolato.md`

## Custom themes (YAML)

xcodex loads `*.yml` / `*.yaml` files from your theme directory and merges them into the catalog.

- A custom theme can **override** a built-in theme by using the same `name`.
- Themes are expected to define:
  - `palette.*` (base terminal palette colors)
  - `roles.*` (semantic roles like transcript background, status bar background, etc.)

Reference examples:

- `docs/themes/example-dark.yaml`
- `docs/themes/example-light.yaml`

To generate starter YAMLs into your theme directory:

- Run `/theme template`

## `/theme` commands and controls

Common actions:

- `/theme` — open the picker with live preview.
- `/theme help` — explains the theme model (especially `roles.*` vs `palette.*`).
- `/theme template` — write example YAML files into your theme directory.

In the `/theme` UI:

- You can preview changes live.
- You can edit and save a customized copy into `$CODEX_HOME/themes`.

## Troubleshooting

If `/theme` shows only built-ins:

- Verify your `themes.dir` points where you expect (see `docs/config.md#themes`).
- Verify your theme files end in `.yml` or `.yaml`.

If colors look wrong:

- Check whether your terminal theme and xcodex theme are fighting (e.g., unusual terminal background).
- Start from `docs/themes/example-dark.yaml` / `docs/themes/example-light.yaml` and adjust incrementally.

