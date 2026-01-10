# Announcements (startup tips)

Both TUIs (`codex-tui` and `codex-tui2`) show a “startup tip” when you launch `xcodex`.

- By default, the tip is a random line from:
  - `codex-rs/tui/tooltips.txt`
  - `codex-rs/tui2/tooltips.txt`
- However, if an “announcement tip” is available, it takes precedence and is shown instead.

## Where announcements come from

On startup, the TUI fetches a TOML document from GitHub raw content and picks the **last matching** entry.

- Fetch URL (fork-specific):
  - `https://raw.githubusercontent.com/Eriz1818/xCodex/main/announcement_tip.toml`
- Source file in this repo:
  - `announcement_tip.toml`

If the fetch fails (offline, restricted network, GitHub down), the TUI silently falls back to the normal random tooltip list.

## Format

`announcement_tip.toml` contains `[[announcements]]` entries. Each entry has:

- `content` (string): the message to show (plain text).
- Optional filters:
  - `from_date` (YYYY-MM-DD, inclusive; UTC)
  - `to_date` (YYYY-MM-DD, exclusive; UTC)
  - `version_regex` (regex matched against the CLI version, `env!("CARGO_PKG_VERSION")`)
  - `target_app` (currently only `"cli"` is used)

## Suggested use

Use announcements for short-lived, high-signal changes:

- “New feature” tips (e.g. `/worktree`, `/hooks`, `--no-alt-screen`)
- Deprecations or behavior changes
- Known issues / mitigations

Keep messages short; they are shown as a single tip line on startup.
