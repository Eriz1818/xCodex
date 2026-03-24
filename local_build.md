# Local xCodex Build and Wrapper Setup

This is the local setup path used to make this fork the default `xcodex` without using the Bazel install pipeline.

## 1. Build and install the fork with Cargo

From the repo root:

```sh
cd /home/neepo/Software/OpenCLaw/xCodex/codex-rs
cargo install --path cli --locked --force --root ~/.local
```

This installs the binary as:

```sh
~/.local/bin/codex
```

## 2. Expose it as `xcodex`

```sh
ln -sf ~/.local/bin/codex ~/.local/bin/xcodex
rehash
```

## 3. Ensure `~/.local/bin` wins in PATH

Add this to `~/.zshrc`:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Reload shell config:

```sh
source ~/.zshrc
rehash
```

Verify:

```sh
type -a xcodex
which xcodex
xcodex --version
```

Expected: `xcodex` should resolve to `~/.local/bin/xcodex` before `/usr/bin/xcodex`.

## 4. Keep the custom `codex` wrapper, but point it at the right Agentic Rice executable

In `~/.zshrc`, these settings were present/needed:

```sh
export AGENTIC_RICE_HOME="$HOME/Software/agentic-rice"
export AGENTIC_RICE_VENV="$AGENTIC_RICE_HOME/.venv"
```

The `codex()` shell function was updated to call `agentic-rice-xcodex` instead of `agentic-rice-codex`:

```sh
codex() {
  if [[ -x "$AGENTIC_RICE_VENV/bin/agentic-rice-xcodex" ]]; then
    "$AGENTIC_RICE_VENV/bin/agentic-rice-xcodex" "$@"
    return
  fi

  PYTHONPATH="$AGENTIC_RICE_HOME/src" python3 -m agentic_rice.codex_wrapper "$@"
}
```

Reload after editing:

```sh
source ~/.zshrc
rehash
type -a codex
type -a xcodex
```

## 5. Resulting command behavior

- `xcodex` runs the fork installed from this checkout.
- `codex` runs the Agentic Rice wrapper.
- The Agentic Rice wrapper defaults to launching `xcodex`, so `codex` also ends up running this fork.

## 6. Optional cleanup

If an older npm-installed `xcodex` still exists, it can remain as a fallback as long as `~/.local/bin` comes first in `PATH`.

To remove the npm-installed fallback entirely:

```sh
npm uninstall -g @eriz1818/xcodex
```
