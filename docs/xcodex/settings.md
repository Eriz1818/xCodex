# Settings (`/settings`)

`/settings` exposes a small set of runtime/persistent configuration toggles for `xcodex`.

In the legacy TUI (`tui`), `/settings` opens a bottom-pane tabbed menu (replacing the composer temporarily).

## Status bar items

`xcodex` can optionally show additional items in the footer status bar, behind toggles.

The values shown are derived from the session’s working directory (the same directory shown in `/status`), so different sessions can show different branches/worktrees.

While enabled, `xcodex` refreshes these values automatically (about every 5 seconds). If `HEAD` is detached, it shows `branch: (detached)`.

### Toggle via `/settings`

```text
/settings status-bar git-branch [on|off|toggle|status]
/settings status-bar worktree   [on|off|toggle|status]
```

- When the action is omitted, `toggle` is assumed.
- `status` prints the current values (no changes).
- Changes persist to your config.

### Configure via `config.toml`

These keys live under `[tui]`:

```toml
[tui]
status_bar_show_git_branch = false
status_bar_show_worktree = false

# Render the active composer with only top/bottom borders (default: false)
# minimal_composer = false

# Xcodex-only UI styling: auto | on | off (default: on)
# xtreme_mode = "on"

# Xcodex-only: per-turn ramp status flows (defaults: true)
# ramps_rotate = true
# ramps_build = true
# ramps_devops = true
```

See:

- `docs/config.md` (`[tui]`)
- `docs/example-config.md`

## Transcript rendering

xcodex also exposes transcript-related rendering toggles. These affect how the transcript is *displayed* in the TUI (they do not change what is sent to the model).

### Toggle via `/settings`

```text
/settings transcript diff-highlight           [on|off|toggle|status]
/settings transcript highlight-past-prompts   [on|off|toggle|status]
/settings transcript syntax-highlight         [on|off|toggle|status]
```

- `diff-highlight`: emphasizes added/removed lines when rendering diffs in the transcript.
- `highlight-past-prompts`: adds a theme-derived background to past user prompts in the transcript.
- `syntax-highlight`: syntax-highlights fenced code blocks in the transcript when supported (themeable).

### Configure via `config.toml`

These keys live under `[tui]`:

```toml
[tui]
transcript_diff_highlight = true
transcript_user_prompt_highlight = true
transcript_syntax_highlight = true
```

For theming syntax highlighting (token colors), see `docs/xcodex/themes.md` and `docs/config.md#themes` (`roles.code_*`).

## Related

- `docs/slash_commands.md`
- `docs/xcodex/README.md`
