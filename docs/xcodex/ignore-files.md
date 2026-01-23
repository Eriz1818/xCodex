# Ignore Files

Codex supports repo-local ignore files to exclude paths from AI-assisted workflows.

## Quick Start

Create a `.aiexclude` or `.xcodexignore` file in your Git repository root:

```gitignore
# Secrets and credentials
.env
.env.*
secrets/
**/.aws/**
*.pem
*.key

# Internal documentation
internal-docs/
```

## Supported Files

| Filename | Location |
|----------|----------|
| `.aiexclude` | Repository root |
| `.xcodexignore` | Repository root |

Both files use [gitignore-style pattern matching](https://git-scm.com/docs/gitignore#_pattern_format). To customize which filenames Codex loads, set `[exclusion].files` in `config.toml`.

## What Gets Protected

When a path matches an ignore pattern:

| Protection | Description |
|------------|-------------|
| **File discovery** | Excluded from search results and file listings |
| **Structured tools** | `read_file`, `list_dir`, `grep_files`, `view_image`, `apply_patch` refuse to access |
| **Prompt inclusion** | Blocked from being sent to the model |
| **Path redaction** | Mentions of excluded paths are redacted in outbound text |
| **Self-protection** | The ignore files themselves are never sent to the model |

## Security Model

> [!IMPORTANT]
> **This is best-effort protection at the AI layer, not a security boundary.**

Codex blocks excluded files in its *own* tools and prevents their content from reaching the model through normal workflows. However:

- **Shell commands can still read excluded files via indirection** — Codex is not an OS-level boundary. xcodex can preflight and block obvious excluded-path references in `shell` tool calls (see `preflight_shell_paths`), but scripts/globs/indirect reads can still leak content if allowed to run.
- **MCP servers operate outside Codex's control** — External MCP servers may read any file the OS allows.
- **Content detection is pattern-based** — Codex can redact path mentions and common secret patterns, but cannot reliably detect "this text came from excluded file X" once it's in memory.

### What This Means in Practice

| Scenario | Protected? |
|----------|-----------|
| User asks "read secrets/.env" | ✅ Blocked by structured tools |
| User asks "run `cat secrets/.env`" | ⚠️ May be blocked by preflight; if it runs (or reads indirectly), content may reach model |
| MCP server reads excluded file | ⚠️ Depends on MCP policy (`unattested_output_policy`) |
| User manually pastes secret content | ❌ Not protected (user action) |

### For Stronger Guarantees

If you need stricter isolation:

1. **Don't store secrets in the repo** — Use environment variables, secret managers, or paths outside the workspace.
2. **Use `unattested_output_policy = "confirm"`** — Requires approval before MCP outputs reach the model.
3. **Run in a sandboxed environment** — External process restrictions beyond Codex's scope.

## Pattern Syntax

Patterns use gitignore semantics:

```gitignore
# Directories (trailing slash)
secrets/
**/internal/**

# Specific files
.env
config/production.yml

# Wildcards
*.pem
**/*.key

# Negation (un-exclude)
!public.key
```

**Notes:**
- Patterns are relative to the repository root.
- Directory patterns also exclude all descendants.
- On Windows, use forward slashes (`secrets/`) — avoid `C:\` or UNC paths.

## MCP and External Commands

Ignore files apply to Codex's structured file tools. They do **not** prevent external processes from reading files.

| Tool Type | Exclusion Applied? |
|-----------|-------------------|
| Codex built-in tools (`read_file`, etc.) | ✅ Yes |
| Shell commands (`cat`, `grep`, scripts) | ⚠️ Best-effort (preflight can block obvious excluded-path refs; not an OS boundary) |
| MCP servers | ❌ No — but see `unattested_output_policy` |

**To control MCP output handling**, set in `config.toml`:

```toml
# Options: "allow" (default), "warn", "confirm", "block"
unattested_output_policy = "warn"
```

## Configuration Reference

```toml
[exclusion]
enabled = true                    # Master toggle
paranoid_mode = false             # Enable all scanning layers (L2/L4)
path_matching = true              # Block based on path patterns
secret_patterns = true            # Redact common secrets (API keys, etc.)
on_match = "redact"               # "warn", "redact", or "block"
files = [".aiexclude", ".xcodexignore"]  # Ignore filenames to load
```

See [config.md](../config.md) for the full `[exclusion]` schema.
