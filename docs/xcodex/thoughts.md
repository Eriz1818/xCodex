# Hiding thoughts (`/thoughts`)

Codex can emit “thoughts” / reasoning summaries during a turn. In the TUI, these can be useful for context, but they can also be noisy.

Use `/thoughts` to toggle whether these are shown in the chat transcript.

## Usage

- `/thoughts` — toggle
- `/thoughts on` — show thoughts
- `/thoughts off` — hide thoughts
- `/thoughts toggle` — toggle
- `/thoughts status` — print current setting

This setting persists across sessions (it updates the `hide_agent_reasoning` config value).

Notes:

- This affects newly-received messages; previously-printed terminal scrollback can’t be retroactively removed.
