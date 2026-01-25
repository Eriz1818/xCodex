#!/usr/bin/env python3
"""Preflight checks for xcodex-release workflow integrity."""

from __future__ import annotations

import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "xcodex-release.yml"
INSTALLER = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"

REQUIRED_WINDOWS_BINS = {
    "codex",
    "codex-responses-api-proxy",
    "codex-windows-sandbox-setup",
    "codex-command-runner",
}


def main() -> int:
    workflow_src = WORKFLOW.read_text(encoding="utf-8").replace("\r\n", "\n")

    matrix = re.findall(r"- runner: (.+)\n\s+target: (.+)\n\s+bin: (.+)", workflow_src)
    windows = [(r.strip(), t.strip(), b.strip()) for r, t, b in matrix if "windows" in t]
    if not windows:
        raise SystemExit("No Windows matrix entries with bin detected.")

    bins_by_target: dict[str, set[str]] = {}
    for _, target, bin_name in windows:
        bins_by_target.setdefault(target, set()).add(bin_name)

    for target, bins in sorted(bins_by_target.items()):
        missing = REQUIRED_WINDOWS_BINS - bins
        if missing:
            raise SystemExit(f"Windows target {target} missing bins: {sorted(missing)}")

    if "format('{0}-{1}', matrix.target, matrix.bin)" not in workflow_src:
        raise SystemExit("Artifact naming does not include matrix.bin for Windows split.")

    if "Consolidate Windows artifacts" not in workflow_src:
        raise SystemExit("Missing consolidate Windows artifacts step.")

    installer_src = INSTALLER.read_text(encoding="utf-8")
    if "windows_job_bin" not in installer_src or "target}-{windows_job_bin" not in installer_src:
        raise SystemExit("install_native_deps.py missing windows split artifact lookup.")

    print("xcodex-release preflight checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
