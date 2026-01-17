#!/bin/bash

set -euo pipefail

usage() {
    cat <<'EOF'
Install the current working tree's Codex CLI binary as `xcodex`.

Usage:
  scripts/install-xcodex.sh [--release|--debug] [--local|--remote] [--offline] [--bazel-arg ARG ...] [--dest PATH]

Options:
  --release    Build with Bazel `--compilation_mode=opt`.
  --debug      Build with Bazel `--compilation_mode=dbg` (default).
  --local      Disable BuildBuddy BEP + remote cache (default).
  --remote     Use repo .bazelrc defaults (may use BuildBuddy/remote cache).
  --offline    Don't fetch any missing deps (implies --local). Fails if deps aren't already cached.
  --bazel-arg  Extra bazel arg to pass through (repeatable).
  --dest PATH  Install path (default: ~/.local/bin/xcodex).
EOF
}

PROFILE="debug"
DEST="${XCODEX_DEST:-$HOME/.local/bin/xcodex}"
LOCAL="1"
OFFLINE="0"
BAZEL_ARGS=()

while [ $# -gt 0 ]; do
    case "$1" in
        --release)
            PROFILE="release"
            shift
            ;;
        --debug)
            PROFILE="debug"
            shift
            ;;
        --local)
            LOCAL="1"
            shift
            ;;
        --remote)
            LOCAL="0"
            shift
            ;;
        --offline)
            LOCAL="1"
            OFFLINE="1"
            shift
            ;;
        --bazel-arg)
            BAZEL_ARGS+=("${2:?missing value for --bazel-arg}")
            shift 2
            ;;
        --dest)
            DEST="${2:?missing value for --dest}"
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

ROOT_DIR="$(realpath "$(dirname "$0")/..")"

extract_cargo_version_env() {
    local cargo_toml="$1"
    local cargo_ver

    cargo_ver="$(grep -m1 '^version' "$cargo_toml" | sed -E 's/version *= *"([^"]+)".*/\1/')"

    if [ -z "${CARGO_PKG_VERSION:-}" ]; then
        export CARGO_PKG_VERSION="$cargo_ver"
    fi

    local base_ver
    base_ver="${cargo_ver%%-*}"

    local major minor patch
    IFS='.' read -r major minor patch <<<"$base_ver"

    if [ -z "${CARGO_PKG_VERSION_MAJOR:-}" ]; then
        export CARGO_PKG_VERSION_MAJOR="$major"
    fi
    if [ -z "${CARGO_PKG_VERSION_MINOR:-}" ]; then
        export CARGO_PKG_VERSION_MINOR="$minor"
    fi
    if [ -z "${CARGO_PKG_VERSION_PATCH:-}" ]; then
        export CARGO_PKG_VERSION_PATCH="$patch"
    fi
}


BAZEL_COMPILATION_MODE="dbg"
if [ "$PROFILE" = "release" ]; then
    BAZEL_COMPILATION_MODE="opt"
fi

if [ "$LOCAL" = "1" ]; then
    BAZEL_ARGS+=(
        --bes_backend=
        --bes_results_url=
        --remote_cache=
        --remote_executor=
    )
fi

if [ "$OFFLINE" = "1" ]; then
    BAZEL_ARGS+=(--fetch=false)
fi

extract_cargo_version_env "$ROOT_DIR/codex-rs/Cargo.toml"

(
    cd "$ROOT_DIR"
    bazel build "${BAZEL_ARGS[@]}" --compilation_mode="$BAZEL_COMPILATION_MODE" //codex-rs/cli:codex
)

BAZEL_BIN="$(bazel info "${BAZEL_ARGS[@]}" --compilation_mode="$BAZEL_COMPILATION_MODE" bazel-bin)"
BIN="$BAZEL_BIN/codex-rs/cli/codex"
if [ ! -f "$BIN" ]; then
    echo "expected built binary at $BIN" >&2
    exit 1
fi

mkdir -p "$(dirname "$DEST")"
install -m 755 "$BIN" "$DEST"

echo "Installed xcodex to: $DEST"
"$DEST" --version || true

if command -v codex >/dev/null 2>&1; then
    echo "codex: $(command -v codex)"
fi
if command -v xcodex >/dev/null 2>&1; then
    echo "xcodex: $(command -v xcodex)"
else
    echo "xcodex is not on PATH; add $(dirname "$DEST") to PATH."
fi
