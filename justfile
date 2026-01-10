set working-directory := "codex-rs"
set positional-arguments

# Display help
help:
    just -l

# `codex`
alias c := codex
codex *args:
    bazel run //codex-rs/cli:codex -- "$@"

# `codex exec`
exec *args:
    bazel run //codex-rs/cli:codex -- exec "$@"

# `codex tui`
tui *args:
    bazel run //codex-rs/cli:codex -- tui "$@"

# Build and install the CLI as `xcodex` (defaults to ~/.local/bin/xcodex).
xcodex-install *args:
    bash ../scripts/install-xcodex.sh "$@"

# Run the CLI version of the file-search crate.
file-search *args:
    bazel run //codex-rs/file-search:codex-file-search -- "$@"

# Build the CLI and run the app-server test client
app-server-test-client *args:
    bazel build //codex-rs/cli:codex
    CODEX_BIN="$(bazel info bazel-bin)/codex-rs/cli/codex"         bazel run //codex-rs/app-server-test-client:codex-app-server-test-client --         --codex-bin "$CODEX_BIN" "$@"

# Fetch deps needed for Bazel builds
install:
    bazel fetch //...

# Format the Rust workspace (defaults to `codex-rs/` via `working-directory`).
fmt:
    cargo fmt

# Run clippy with fixes applied. Prefer scoping: `just fix -p codex-tui`.
fix *args:
    cargo clippy --fix --allow-dirty --allow-staged "$@"

# Default test runner
test:
    bazel test //... --keep_going

# Run the MCP server
mcp-server-run *args:
    bazel run //codex-rs/mcp-server:codex-mcp-server -- "$@"
bazel-test:
    bazel test //... --keep_going

bazel-remote-test:
    bazel test //... --config=remote --platforms=//:rbe --keep_going

build-for-release:
    bazel build //codex-rs/cli:release_binaries --config=remote

# Run the MCP server via Cargo (useful when Bazel isn't available/configured)
mcp-server-run-cargo *args:
    cargo run -p codex-mcp-server -- "$@"
