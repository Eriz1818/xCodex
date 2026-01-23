# Worktrees (`/worktree`)

`/worktree` lets you run multiple `xcodex` sessions side-by-side, each pinned to a different git worktree (and therefore branch).

This makes it easy to work on multiple branches in parallel and see each session’s current worktree/branch at a glance via the footer status bar.

## Quickstart

1) (Optional) Show the worktree path in the status bar:

```text
/settings status-bar worktree on
```

2) Switch this session to a different worktree:

```text
/worktree
```

3) If you created worktrees outside xcodex, refresh the picker list:

```text
/worktree detect
```

4) If you keep persistent git-untracked dirs (notes/scratch), pick one:

- **Pinned paths** (structured file tools only; shell unchanged): set `pinned_paths`.
- **Shared dirs** (also visible to shell): set `shared_dirs` and link them.

Pinned paths config:

```toml
[worktrees]
pinned_paths = ["notes/**"]
```

Shared dirs config:

```toml
[worktrees]
shared_dirs = ["notes", "tmp"]
auto_link_shared_dirs = true
```

Or, configure them from inside the TUI (writes to config):

```text
/worktree shared add notes
/worktree shared add tmp
/worktree shared list
```

Then apply them once in each existing worktree:

```text
/worktree link-shared
```

If the worktree already has content under a shared dir, `/worktree link-shared` will guide you through per-dir actions (migrate+link, replace+link, skip).

## Roots + shared dirs contract

`xcodex` deliberately distinguishes three related concepts:

- **Active worktree root**: the git worktree directory your session is currently “in” (the footer `wt:`).
  - Tool calls that use the session working directory run from this directory.
- **Workspace root**: the root of the *main* git worktree (the directory that owns the shared git metadata).
  - This is stable even while you switch to other worktrees.
  - `xcodex` uses this as the target location for “shared dirs”.
- **Shared dirs** (optional): repo-relative directories that are linked *from the active worktree* back to the workspace root.
  - When linked, reads/writes to `./<shared_dir>` inside a non-root worktree resolve to `workspace_root/<shared_dir>`.
  - The intent is to keep persistent **git-untracked** content (notes, scratch artifacts, local-only scripts) in one place, even while switching worktrees.

## Picking a worktree

- Run `/worktree` to open the worktree picker.
- Select a worktree to switch the session root to that directory.

Once switched:

- The footer `wt:` and `branch:` are derived from the session’s worktree.
- The footer updates immediately after switching, and will track branch/HEAD changes automatically while the session is running (file watcher + polling fallback).
- Tool calls that use the session working directory will also run from that worktree.
- If untracked files are detected in the previous worktree, xcodex will warn (so you don’t accidentally delete a worktree and lose local files).

## Refreshing the list

Worktrees are detected automatically when a session starts.

If you created or removed worktrees while `xcodex` is running, use either:

- `/worktree detect`
- the “Refresh worktrees” option in the picker

## Switching by name or path

You can switch directly without opening the picker:

```text
/worktree <name>
/worktree <path>
```

Where `<name>` matches the last path component of a detected worktree.

## Shared dirs (optional)

If you keep persistent **untracked** files (notes, scratch artifacts, local-only scripts), you can opt into “shared dirs” so those paths always resolve to a stable **workspace root** even while you switch worktrees.

Configure shared dirs in `config.toml`:

```toml
[worktrees]
shared_dirs = ["notes", "tmp"]
auto_link_shared_dirs = true
```

Or manage them from inside the TUI (writes to config):

```text
/worktree shared add notes
/worktree shared rm notes
/worktree shared list
```

Notes:

- No defaults: if `shared_dirs` is empty, nothing is linked.
- Safety checks: xcodex refuses to link a shared dir when it contains tracked files or when the path is non-empty (to avoid accidental data loss).
- If you want `shared_dirs` to “just work” when switching worktrees, enable `auto_link_shared_dirs` (otherwise link manually via `/worktree link-shared`).
- `/worktree link-shared` is safe to rerun; it can repair existing links that point elsewhere.

### Commands

- `/worktree doctor` shows link status + untracked summaries for configured `shared_dirs`.
- `/worktree shared add|rm|list` edits `worktrees.shared_dirs` without opening `config.toml`.
- `/worktree link-shared` applies shared-dir links to the current worktree, with a guided workflow for non-empty dirs (migrate+link or replace+link).
  - Migration includes ignored-but-not-tracked content (e.g. untracked `notes/` or `tmp/` directories).
  - Use this after creating worktrees outside xcodex (e.g. `git worktree add ...`).
- `/worktree link-shared --migrate` opens the same workflow but preselects migrate+link actions.
- `/worktree init` opens a guided worktree creation flow and switches this session to it.
  - Fast path: `/worktree init <name> <branch> [<path>]` (non-interactive).
  - Default location: `workspace_root/.worktrees/<name>` (if `<path>` is omitted).
  - `<path>` is interpreted relative to the workspace root unless it’s absolute.
  - Shared dirs: the flow starts from `worktrees.shared_dirs` and does not add defaults; use “Add shared dir…” to add new entries (persisted on success). Recommended: `notes`, `tmp`.

## Untracked files (advanced workflow)

If you only work with tracked files, you can create and switch worktrees freely — no extra setup is needed.

If you keep persistent **untracked** files (notes, scratch artifacts, local-only scripts), remember:

- Worktrees are separate directories.
- Untracked files live inside the worktree directory where they were created.
- If you delete a worktree, those untracked files can be lost.

### Options

- Quick manual safety net: stash untracked files before deleting a worktree:
  - `git stash push -u -m "worktree scratch"`
- Shared dirs across worktrees (opt-in): configure `[worktrees].shared_dirs` so untracked writes land in a stable “workspace root” even while you switch worktrees (see “Shared dirs” above).
  - Pick untracked directories (xcodex refuses to link shared dirs that contain tracked files).

## Pinned paths (structured file tools only)

If you want certain repo-relative paths to always resolve to the **workspace root** for structured file tools (so they don’t get “stranded” per-worktree), you can opt into pinned paths:

```toml
[worktrees]
pinned_paths = ["notes/**"]
```

Edit from the TUI:

```text
/settings worktrees
```

Behavior:

- Applies to structured file tools (e.g. patching/reading/listing/searching files).
- Does **not** change shell behavior: a shell `ls notes` still runs from the active worktree and won’t see pinned paths unless you also use `shared_dirs` + `/worktree link-shared`.
