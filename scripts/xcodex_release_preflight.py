#!/usr/bin/env python3
"""Preflight checks for xcodex-release workflow integrity."""

from __future__ import annotations

import ast
import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "xcodex-release.yml"
INSTALLER = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"
BUILD_NPM_PACKAGE = REPO_ROOT / "codex-cli" / "scripts" / "build_npm_package.py"

REQUIRED_WINDOWS_BINS = {
    "codex",
    "codex-responses-api-proxy",
    "codex-windows-sandbox-setup",
    "codex-command-runner",
}


def main() -> int:
    workflow_src = WORKFLOW.read_text(encoding="utf-8").replace("\r\n", "\n")
    workflow_targets = set(
        re.findall(r"^[ \t]*target:\s*([A-Za-z0-9_-]+)", workflow_src, flags=re.MULTILINE)
    )

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

    if "--bin codex --bin codex-responses-api-proxy" not in workflow_src:
        raise SystemExit("Non-Windows builds are missing codex + responses-api-proxy binaries.")

    if "out_name=\"xcodex-${target}.exe\"" not in workflow_src:
        raise SystemExit("Windows artifact naming missing xcodex-<target>.exe mapping.")

    if "out_name=\"xcodex-responses-api-proxy-${target}.exe\"" not in workflow_src:
        raise SystemExit("Windows artifact naming missing xcodex-responses-api-proxy mapping.")

    if "xcodex-${target}" not in workflow_src:
        raise SystemExit("Non-Windows artifact naming missing xcodex-<target> mapping.")

    if "xcodex-responses-api-proxy-${target}" not in workflow_src:
        raise SystemExit("Non-Windows artifact naming missing xcodex-responses-api-proxy mapping.")

    installer_src = INSTALLER.read_text(encoding="utf-8")
    if "windows_job_bin" not in installer_src or "target}-{windows_job_bin" not in installer_src:
        raise SystemExit("install_native_deps.py missing windows split artifact lookup.")

    def _component_block(name: str) -> str:
        marker = f"\"{name}\": BinaryComponent("
        start = installer_src.find(marker)
        if start == -1:
            return ""
        end = installer_src.find("),", start)
        if end == -1:
            end = start + 500
        return installer_src[start:end]

    if "windows_job_bin=\"codex\"" not in _component_block("xcodex"):
        raise SystemExit("install_native_deps.py missing windows_job_bin mapping for xcodex.")

    if "windows_job_bin=\"codex-responses-api-proxy\"" not in _component_block(
        "xcodex-responses-api-proxy"
    ):
        raise SystemExit(
            "install_native_deps.py missing windows_job_bin mapping for xcodex-responses-api-proxy."
        )

    mod = ast.parse(installer_src, filename=str(INSTALLER))
    binary_targets = None
    rg_pairs = None
    binary_components = None
    for node in mod.body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id == "BINARY_TARGETS":
                    binary_targets = ast.literal_eval(node.value)
                if isinstance(target, ast.Name) and target.id == "RG_TARGET_PLATFORM_PAIRS":
                    rg_pairs = ast.literal_eval(node.value)
                if isinstance(target, ast.Name) and target.id == "BINARY_COMPONENTS":
                    if isinstance(node.value, ast.Dict):
                        binary_components = {
                            key.value for key in node.value.keys if isinstance(key, ast.Constant)
                        }
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            if node.target.id == "BINARY_TARGETS":
                binary_targets = ast.literal_eval(node.value)
            if node.target.id == "RG_TARGET_PLATFORM_PAIRS":
                rg_pairs = ast.literal_eval(node.value)
            if node.target.id == "BINARY_COMPONENTS":
                if isinstance(node.value, ast.Dict):
                    binary_components = {
                        key.value for key in node.value.keys if isinstance(key, ast.Constant)
                    }

    if not isinstance(binary_targets, tuple):
        raise SystemExit("Unable to read BINARY_TARGETS from install_native_deps.py.")
    if not isinstance(rg_pairs, list):
        raise SystemExit("Unable to read RG_TARGET_PLATFORM_PAIRS from install_native_deps.py.")
    if not isinstance(binary_components, set):
        raise SystemExit("Unable to read BINARY_COMPONENTS from install_native_deps.py.")

    rg_targets = {target for target, _ in rg_pairs}
    missing_rg = sorted(target for target in binary_targets if target not in rg_targets)
    if missing_rg:
        raise SystemExit(f"RG_TARGET_PLATFORM_PAIRS missing targets: {missing_rg}")

    missing_targets = sorted(set(binary_targets) - workflow_targets)
    extra_targets = sorted(workflow_targets - set(binary_targets))
    if missing_targets or extra_targets:
        raise SystemExit(
            "Workflow targets mismatch BINARY_TARGETS. "
            f"Missing: {missing_targets} Extra: {extra_targets}"
        )

    npm_src = BUILD_NPM_PACKAGE.read_text(encoding="utf-8")
    npm_mod = ast.parse(npm_src, filename=str(BUILD_NPM_PACKAGE))
    package_components = None
    windows_only_components = None
    for node in npm_mod.body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id == "PACKAGE_NATIVE_COMPONENTS":
                    package_components = ast.literal_eval(node.value)
                if isinstance(target, ast.Name) and target.id == "WINDOWS_ONLY_COMPONENTS":
                    windows_only_components = ast.literal_eval(node.value)
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            if node.target.id == "PACKAGE_NATIVE_COMPONENTS":
                package_components = ast.literal_eval(node.value)
            if node.target.id == "WINDOWS_ONLY_COMPONENTS":
                windows_only_components = ast.literal_eval(node.value)

    if not isinstance(package_components, dict):
        raise SystemExit("Unable to read PACKAGE_NATIVE_COMPONENTS from build_npm_package.py.")
    if not isinstance(windows_only_components, dict):
        raise SystemExit("Unable to read WINDOWS_ONLY_COMPONENTS from build_npm_package.py.")

    expected_components = {
        component
        for components in package_components.values()
        for component in components
        if component != "rg"
    }
    expected_components |= {
        component
        for components in windows_only_components.values()
        for component in components
        if component != "rg"
    }
    missing_components = sorted(expected_components - binary_components)
    if missing_components:
        raise SystemExit(f"BINARY_COMPONENTS missing expected entries: {missing_components}")

    print("xcodex-release preflight checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
