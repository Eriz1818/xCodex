# Changelog

Releases are published on GitHub: https://github.com/Eriz1818/xCodex/releases

This project follows SemVer (`xcodex-vX.Y.Z` / `xcodex-vX.Y.Z-alpha.N` tags) and publishes:

- `@eriz1818/xcodex` (CLI wrapper + native `xcodex` binaries + bundled `rg`)
- `@eriz1818/xcodex-responses-api-proxy` (native proxy binary)

## Unreleased

## 0.4.0

Side-by-side diff and major Plan mode workflow upgrades.

- Added side-by-side diff view for clearer comparisons during reviews.
- Plan mode can now be toggled directly with `Shift+Tab`, so you no longer need to type `/plan`.
- Added workflow modes for Plan mode: `default` (Codex-style), `adr-lite`, and `custom`.
- Added custom workflow templating for Plan mode, including seed options from `default` or `adr-lite`.
- Expanded Plan mode with richer options and controls for planning and execution.
- Improved mode ergonomics and clarity across TUI/TUI2, including stronger active-state cues.
- Added durable plan-file operations in CLI (`xcodex plan status|list|open|done|archive`).
- Added one-time `.gitignore` guidance when plan base-dir is inside a repo and not ignored.
- Fixed a response-routing regression where Plan mode and exclusion approvals could wait indefinitely after users submitted answers.
- Improved worktree/branch dedupe rendering by showing both git and folder context while keeping the deduped leaf visually clear.
- Improved exclusion approval readability by highlighting matched layer/value/reason fields in the prompt.

## 0.3.6

TUI exclusion management, transcript rendering reliability fixes, and GPT-5.3 Codex availability.

- Added a new `/exclusion` command for managing exclusions directly from the TUI.
- Fixed theme-related transcript rendering gaps.
- OpenAI `gpt-5.3-codex` is now available in xcodex.
- Upstream sync with additional stability and infrastructure fixes across core, TUI, and app-server.

## 0.3.5

Syntax highlighting, lazy MCP loading, and startup responsiveness fixes.

- Syntax highlighting for: Bash (bash/sh/zsh), C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Ruby, Rust, TypeScript, YAML (see `docs/xcodex/themes.md`).
- Resume/startup responsiveness fixes for faster session loading.
- Lazy MCP loading to defer server startup until needed (see `docs/xcodex/lazy-mcp-loading.md`).
- Internal code restructure for stability and maintainability.

## 0.3.0

Themes, privacy controls, and collaboration UX improvements.

- Themes: theme picker + preview, plus a built-in theme bundle.
- Privacy: sensitive-path exclusion + redaction controls for AI-visible files.
- Collaboration modes: improved UI + presets.
- Permissions: improved `/permissions` flow and approval prompts.
- Config: layered config support and more toggles.

## 0.2.0

Hooks and packaging improvements.

- Hooks system: external hooks, Python Host (“py-box”) hooks, and PyO3 hooks (separately built).
- Hooks tooling: guided setup (`xcodex hooks init`), SDK + sample installers, and `xcodex hooks test`.
- Worktrees: `/worktree` for switching between git worktrees (plus shared dirs).
- npm releases: publish `@eriz1818/xcodex` and `@eriz1818/xcodex-responses-api-proxy` via `xcodex-vX.Y.Z` tags.

## 0.1.0

Initial public release of the `xCodex` fork.

- Fork-only UX/features: `/compact`, `/autocompact`, `/thoughts`, `/ps` and `/ps-kill`, hooks automation, worktree/session helpers.
- First-run setup wizard for `xcodex` with fork-specific default home (`~/.xcodex`) and safer MCP token isolation from upstream.
- Npm packaging + release pipeline foundation: vendored native binaries per target, bundled `rg`, GitHub Actions tag-based releases (`xcodex-v*`) and OIDC publishing support.
