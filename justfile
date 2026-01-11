set working-directory := "codex-rs"
set positional-arguments

# Display help
help:
    just -l

# `codex`
alias c := codex
codex *args:
    cargo run --bin codex -- "$@"

# `codex exec`
exec *args:
    cargo run --bin codex -- exec "$@"

# `codex tui`
tui *args:
    cargo run --bin codex -- tui "$@"

# Build and install the CLI as `xcodex` (defaults to ~/.local/bin/xcodex).
xcodex-install *args:
    bash ../scripts/install-xcodex.sh "$@"

# Build and install a PyO3-enabled CLI as `xcodex-pyo3` (defaults to ~/.local/bin/xcodex-pyo3).
xcodex-pyo3-install *args:
    bash ../scripts/install-xcodex-pyo3.sh "$@"
# Run the CLI version of the file-search crate.
file-search *args:
    cargo run --bin codex-file-search -- "$@"

# Build the CLI and run the app-server test client
app-server-test-client *args:
    cargo build -p codex-cli
    cargo run -p codex-app-server-test-client -- --codex-bin ./target/debug/codex "$@"

# format code
fmt:
    cargo fmt -- --config imports_granularity=Item

fix *args:
    bash -lc 'set -euo pipefail; \
      if [[ -z "${PYO3_PYTHON:-}" ]] && command -v python3.11 >/dev/null; then export PYO3_PYTHON="$(command -v python3.11)"; fi; \
      cargo clippy --fix --all-features --tests --allow-dirty "$@"'

clippy:
    bash -lc 'set -euo pipefail; \
      if [[ -z "${PYO3_PYTHON:-}" ]] && command -v python3.11 >/dev/null; then export PYO3_PYTHON="$(command -v python3.11)"; fi; \
      cargo clippy --all-features --tests "$@"'

hooks-codegen:
    cargo run -p codex-core --bin hooks_schema --features hooks-schema --quiet > ../docs/xcodex/hooks.schema.json
    cargo run -p codex-core --bin hooks_typescript --features hooks-schema --quiet > common/src/hooks_sdk_assets/js/xcodex_hooks.d.ts
    cargo run -p codex-core --bin hooks_python_types --features hooks-schema --quiet > common/src/hooks_sdk_assets/python/xcodex_hooks_types.py
    cargo run -p codex-core --bin hooks_python_models --features hooks-schema --quiet > common/src/hooks_sdk_assets/python/xcodex_hooks_models.py
    cargo run -p codex-core --bin hooks_python_models --features hooks-schema --quiet > ../examples/hooks/xcodex_hooks_models.py
    cargo run -p codex-core --bin hooks_go_types --features hooks-schema --quiet > common/src/hooks_sdk_assets/go/hooksdk/types.go
    cargo run -p codex-core --bin hooks_rust_sdk --features hooks-schema --quiet > hooks-sdk/src/generated.rs

hooks-codegen-check:
    just hooks-codegen
    git diff --exit-code -- \
      ../docs/xcodex/hooks.schema.json \
      common/src/hooks_sdk_assets/js/xcodex_hooks.d.ts \
      common/src/hooks_sdk_assets/python/xcodex_hooks_types.py \
      common/src/hooks_sdk_assets/python/xcodex_hooks_models.py \
      ../examples/hooks/xcodex_hooks_models.py \
      hooks-sdk/src/generated.rs \
      common/src/hooks_sdk_assets/go/hooksdk/types.go

hooks-templates-smoke:
    bash -c 'set -euo pipefail; command -v docker >/dev/null || { echo "docker is required"; exit 1; }; \
      run_python() { docker run --rm -v "{{invocation_directory()}}:/ws" python:3.11-slim bash -lc '"'"'set -euo pipefail; CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        mkdir -p "$CODEX_HOME/hooks/templates/python"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/python/xcodex_hooks.py "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/python/xcodex_hooks_models.py "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/python/xcodex_hooks_runtime.py "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/python/xcodex_hooks_types.py "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/python/template_hook.py "$CODEX_HOME/hooks/templates/python/log_jsonl.py"; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"py\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | python3 "$CODEX_HOME/hooks/templates/python/log_jsonl.py"; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"py\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_node() { docker run --rm -v "{{invocation_directory()}}:/ws" node:22 bash -lc '"'"'set -euo pipefail; CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        mkdir -p "$CODEX_HOME/hooks/templates/js"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/js/xcodex_hooks.mjs "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/js/template_hook.mjs "$CODEX_HOME/hooks/templates/js/log_jsonl.mjs"; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"node\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | node "$CODEX_HOME/hooks/templates/js/log_jsonl.mjs"; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"node\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_ruby() { docker run --rm -v "{{invocation_directory()}}:/ws" ruby:3.3 bash -lc '"'"'set -euo pipefail; CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        mkdir -p "$CODEX_HOME/hooks/templates/ruby"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/ruby/xcodex_hooks.rb "$CODEX_HOME/hooks/"; \
        cp /ws/codex-rs/common/src/hooks_sdk_assets/ruby/template_hook.rb "$CODEX_HOME/hooks/templates/ruby/log_jsonl.rb"; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"rb\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | ruby "$CODEX_HOME/hooks/templates/ruby/log_jsonl.rb"; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"rb\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_go() { docker run --rm -v "{{invocation_directory()}}:/ws" -w /ws/codex-rs/common/src/hooks_sdk_assets/go golang:1.22 bash -lc '"'"'set -euo pipefail; \
        /usr/local/go/bin/go build -o /tmp/hook ./cmd/log_jsonl; \
        CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"go\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | /tmp/hook; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"go\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_java() { docker run --rm -v "{{invocation_directory()}}:/ws" -w /ws/codex-rs/common/src/hooks_sdk_assets/java maven:3.9-eclipse-temurin-17 bash -lc '"'"'set -euo pipefail; \
        mvn -q -DskipTests -pl template -am package dependency:copy-dependencies; \
        CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"java\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | java -cp "template/target/classes:template/target/dependency/*" dev.xcodex.hooks.LogJsonlHook; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"java\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_rust() { docker run --rm -v "{{invocation_directory()}}:/ws" -w /ws/codex-rs/common/src/hooks_sdk_assets/rust rust:1.90 bash -lc '"'"'set -euo pipefail; \
        /usr/local/cargo/bin/cargo build --release; \
        CODEX_HOME="$(mktemp -d)"; export CODEX_HOME; \
        payload_path="$CODEX_HOME/payload.json"; \
        printf "%s" "{\"schema_version\":1,\"event_id\":\"e\",\"timestamp\":\"t\",\"session_id\":\"th\",\"transcript_path\":\"\",\"permission_mode\":\"default\",\"hook_event_name\":\"PostToolUse\",\"xcodex_event_type\":\"tool-call-finished\",\"turn_id\":\"tu\",\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_use_id\":\"c\",\"tool_response\":null,\"status\":\"completed\",\"duration_ms\":1,\"success\":true,\"output_bytes\":0,\"__marker__\":\"rust\"}" > "$payload_path"; \
        printf "%s" "{\"payload_path\":\"$payload_path\"}" | ./target/release/xcodex-hooks-rust-template; \
        grep -Eq "\"__marker__\"[[:space:]]*:[[:space:]]*\"rust\"" "$CODEX_HOME/hooks.jsonl"; \
      '"'"'; }; \
      run_python; run_node; run_ruby; run_go; run_java; run_rust; echo "ok";'

install:
    rustup show active-toolchain
    cargo fetch

# Run `cargo nextest` since it's faster than `cargo test`, though including
# --no-fail-fast is important to ensure all tests are run.
#
# Run `cargo install cargo-nextest` if you don't have it installed.
test:
    cargo nextest run --no-fail-fast

# Run the MCP server
mcp-server-run *args:
    cargo run -p codex-mcp-server -- "$@"
