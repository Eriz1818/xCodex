## Install & build

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

### Build from source

```bash
# Clone the repository and navigate to the root of the Cargo workspace.
git clone https://github.com/Eriz1818/xCodex.git
cd xCodex

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy
# Install helper tools used by the workspace justfile:
cargo install just
# Optional: install nextest for the `just test` helper (or use `cargo test --all-features` as a fallback)
cargo install cargo-nextest

# Build the CLI.
cd codex-rs
cargo build -p codex-cli --bin codex

# Launch the TUI with a sample prompt.
cargo run --bin codex -- "explain this codebase to me"

# Install this fork locally as `xcodex` (default: ~/.local/bin/xcodex).
just xcodex-install --release
xcodex --version

# After making changes, use the root justfile helpers (they default to codex-rs):
just fmt
just fix -p <crate-you-touched>

# Run the relevant tests (project-specific is fastest), for example:
cargo test -p codex-tui
# If you have cargo-nextest installed, `just test` runs the full suite:
just test
# Otherwise, fall back to:
cargo test --all-features
```
