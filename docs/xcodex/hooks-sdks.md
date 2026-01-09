# Typed external hook SDKs

These SDKs are small, optional helpers/templates for writing **external hooks** in various languages.

They do **not** change how hooks run: external hooks are still executed as commands configured under `[hooks]` and receive event JSON on stdin (with `payload-path` envelopes for large payloads).

The installed files are intentionally “readable first”: you should be able to open them in `$CODEX_HOME/hooks/` and understand the hook flow just by reading the comments/docstrings.

## Packaging (optional)

You do **not** need any published packages (PyPI / npm / crates.io / Maven Central) to use these SDKs.

`xcodex hooks install ...` vendors the helpers/templates directly into `$CODEX_HOME/hooks/`.

Notes:
- Script-based templates (Python/Node/Ruby) run as-is (you just need the language runtime installed).
- Compiled templates (Go/Rust/Java) are small project skeletons; you build them using your normal toolchain (Go / Cargo / Maven).

## Templates vs libraries

Some SDKs are just single-file helpers (meant to be copied), while others are reusable “real” libraries:

- Templates: example hooks under `$CODEX_HOME/hooks/templates/` that you copy and customize.
- Libraries: reusable code installed under `$CODEX_HOME/hooks/sdk/` that templates depend on, so parsing logic doesn’t drift across copies.

## Install

Install SDK helpers into your active `CODEX_HOME`:

```sh
xcodex hooks install --list
xcodex hooks install python
xcodex hooks install javascript
xcodex hooks install typescript
xcodex hooks install ruby
xcodex hooks install go
xcodex hooks install rust
xcodex hooks install java
```

To install everything (or overwrite existing files), use:

```sh
xcodex hooks install --all
xcodex hooks install --all --force
```

In the TUI / TUI2 you can also run:

```text
/hooks install list
/hooks install python
/hooks install all --force
```

Note: `xcodex hooks init` scaffolds Python hook examples under `$CODEX_HOME/hooks/` and also installs the Python helper (`xcodex_hooks.py`) so the generated examples work out of the box.

## What gets installed

Files are written under:

- `$CODEX_HOME/hooks/` (shared helpers like `xcodex_hooks.py`, `xcodex_hooks.mjs`, `xcodex_hooks.rb`)
- `$CODEX_HOME/hooks/templates/` (ready-to-run templates, including Go/Rust/Java project skeletons)
- `$CODEX_HOME/hooks/sdk/` (installed libraries used by some templates)

Python-specific notes:
- `xcodex_hooks.py` is the main helper (`read_payload()` and `read_payload_model()`).
- `xcodex_hooks_types.py` contains generated `TypedDict` event types.
- `xcodex_hooks_models.py` contains generated dataclass models + `parse_hook_event(...)` with light coercions.
- `xcodex_hooks_runtime.py` contains `TypeGuard` helpers like `is_tool_call_finished(...)`.

Rust-specific notes:
- `$CODEX_HOME/hooks/sdk/rust/` is an installed copy of the `codex-hooks-sdk` crate (tolerant stdin/envelope parsing + typed events).
- The Rust template under `$CODEX_HOME/hooks/templates/rust/` depends on that local crate (no copy-pasted parsing helpers).

Java-specific notes:
- `$CODEX_HOME/hooks/templates/java/` is a small Maven multi-module project:
  - `sdk/` contains the reusable library (`HookReader`, typed `HookEvent` models, unknown-field preservation via `extra()`).
  - `template/` contains a runnable example hook.

## Next steps

1) Pick a template from `$CODEX_HOME/hooks/templates/` and customize it.

2) Configure it in `config.toml`:

```toml
[hooks]
tool_call_finished = [["python3", "/absolute/path/to/your_hook.py"]]
```

For the full hooks contract (events + payloads), see `docs/xcodex/hooks.md` and `docs/config.md#hooks`.

## Toward “fully typed” SDKs

Today’s SDK installers focus on the most error-prone part of hook authoring: correctly handling stdin vs the `payload-path` envelope.

To make the SDKs *fully typed* across languages, we should:

1) Define a single “payload contract” document and compatibility policy (forward-compatible parsing, unknown fields, how `schema-version` evolves).
2) Generate a machine-readable schema from the Rust source-of-truth (`codex_core::hooks::{HookPayload, HookNotification, ...}`), e.g. JSON Schema.
3) Generate language-specific types (TS `.d.ts`, Python TypedDicts, Go structs, etc.) from that schema, and keep generation in CI.

The key is to keep the SDK parsers tolerant (unknown fields/types should not break hooks) while still providing strong typing for known events.

### Compatibility policy

The compatibility policy for external hook payloads is documented in:

- `docs/xcodex/hooks.md` (“Compatibility policy (payload schema)”)

If you’re writing hooks or SDKs, that section is the source of truth for:

- What `schema-version` means (and when it bumps)
- How unknown fields / unknown event types must be handled
- Why `payload-path` is treated as a transport envelope detail (SDK responsibility)

### Machine-readable schema

xcodex checks in a generated JSON Schema bundle at:

- `docs/xcodex/hooks.schema.json`

To regenerate it after hook payload changes:

```sh
cd codex-rs
cargo run -p codex-core --bin hooks_schema --features hooks-schema --quiet > ../docs/xcodex/hooks.schema.json
```

The TypeScript `.d.ts` installed by `xcodex hooks install {javascript,typescript}` is also generated from the Rust source of truth:

```sh
cd codex-rs
cargo run -p codex-core --bin hooks_typescript --features hooks-schema --quiet > common/src/hooks_sdk_assets/js/xcodex_hooks.d.ts
```

Python types are generated too:

```sh
cd codex-rs
cargo run -p codex-core --bin hooks_python_types --features hooks-schema --quiet \
  > common/src/hooks_sdk_assets/python/xcodex_hooks_types.py
```

Python runtime models (dataclasses) are generated too:

```sh
cd codex-rs
cargo run -p codex-core --bin hooks_python_models --features hooks-schema --quiet \
  > common/src/hooks_sdk_assets/python/xcodex_hooks_models.py
```

Go types are generated too:

```sh
cd codex-rs
cargo run -p codex-core --bin hooks_go_types --features hooks-schema --quiet \
  > common/src/hooks_sdk_assets/go/hooksdk/types.go
```
