#!/usr/bin/env python3
"""Preflight checks for xcodex-release workflow integrity."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys
from types import ModuleType

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "xcodex-release.yml"
INSTALLER = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"
BUILD_NPM_PACKAGE = REPO_ROOT / "codex-cli" / "scripts" / "build_npm_package.py"

REQUIRED_CORE_BINS = {"codex", "codex-responses-api-proxy"}
REQUIRED_WINDOWS_BINS = {
    "codex",
    "codex-responses-api-proxy",
    "codex-windows-sandbox-setup",
    "codex-command-runner",
}
REQUIRED_ARM_RUNNER_PAIRS = {
    ("ubuntu-24.04-arm", "aarch64-unknown-linux-musl"),
    ("ubuntu-24.04-arm", "aarch64-unknown-linux-gnu"),
    ("windows-11-arm", "aarch64-pc-windows-msvc"),
}


def main() -> int:
    allow_musl_omission = os.environ.get("XCODEX_ALLOW_MUSL_OMISSION") == "1"
    workflow_src = WORKFLOW.read_text(encoding="utf-8").replace("\r\n", "\n")
    cargo_chef_condition = (
        "matrix.target == 'aarch64-apple-darwin' || matrix.target == "
        "'aarch64-unknown-linux-gnu' || (matrix.target == 'x86_64-apple-darwin' && "
        "matrix.bin == 'codex')"
    )
    if workflow_src.count(cargo_chef_condition) < 2:
        raise SystemExit(
            "cargo-chef prewarm must be limited to aarch64-apple-darwin, "
            "aarch64-unknown-linux-gnu, and x86_64-apple-darwin codex-only."
        )
    matrix_entries = parse_workflow_matrix_entries(workflow_src)
    if not matrix_entries:
        raise SystemExit("No matrix include entries found in xcodex-release workflow.")

    installer = load_module(INSTALLER, "xcodex_install_native_deps")
    npm_packager = load_module(BUILD_NPM_PACKAGE, "xcodex_build_npm_package")

    binary_targets = tuple(installer.BINARY_TARGETS)
    if not binary_targets:
        raise SystemExit("BINARY_TARGETS is empty in install_native_deps.py.")

    musl_targets = set(installer.MUSL_TARGETS)
    expected_targets = set(binary_targets)
    if allow_musl_omission:
        expected_targets -= musl_targets

    workflow_targets = {entry["target"] for entry in matrix_entries}
    missing_targets = sorted(expected_targets - workflow_targets)
    extra_targets = sorted(workflow_targets - set(binary_targets))
    if missing_targets or extra_targets:
        raise SystemExit(
            "Workflow targets mismatch BINARY_TARGETS. "
            f"Missing: {missing_targets} Extra: {extra_targets}"
        )

    bins_by_target: dict[str, set[str]] = {target: set() for target in workflow_targets}
    unsplit_targets: set[str] = set()
    runner_target_pairs: set[tuple[str, str]] = set()
    for entry in matrix_entries:
        target = entry["target"]
        runner_target_pairs.add((entry["runner"], target))
        bin_name = entry.get("bin")
        if bin_name:
            bins_by_target[target].add(bin_name)
        else:
            unsplit_targets.add(target)

    missing_arm_pairs = REQUIRED_ARM_RUNNER_PAIRS.copy()
    if allow_musl_omission:
        missing_arm_pairs = {
            pair for pair in missing_arm_pairs if "linux-musl" not in pair[1]
        }
    missing_arm_pairs = missing_arm_pairs - runner_target_pairs
    if missing_arm_pairs:
        raise SystemExit(
            "ARM runner mapping mismatch; expected these (runner, target) pairs:\n"
            + "\n".join(
                f"- {runner} / {target}" for runner, target in sorted(missing_arm_pairs)
            )
        )

    for target in sorted(expected_targets):
        required_bins = set()
        if target not in unsplit_targets:
            required_bins |= REQUIRED_CORE_BINS
        if "windows" in target:
            required_bins |= REQUIRED_WINDOWS_BINS

        missing_bins = sorted(required_bins - bins_by_target.get(target, set()))
        if missing_bins:
            raise SystemExit(
                f"Target {target} is missing required split bins in workflow matrix: {missing_bins}"
            )

    rg_targets = {target for target, _ in installer.RG_TARGET_PLATFORM_PAIRS}
    missing_rg = sorted(target for target in binary_targets if target not in rg_targets)
    if missing_rg:
        raise SystemExit(f"RG_TARGET_PLATFORM_PAIRS missing targets: {missing_rg}")

    package_components = npm_packager.PACKAGE_NATIVE_COMPONENTS
    windows_only_components = npm_packager.WINDOWS_ONLY_COMPONENTS
    binary_components = installer.BINARY_COMPONENTS

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

    missing_components = sorted(expected_components - set(binary_components))
    if missing_components:
        raise SystemExit(f"BINARY_COMPONENTS missing expected entries: {missing_components}")

    verify_artifact_lookup_coverage(
        installer=installer,
        binary_targets=set(binary_targets),
        expected_targets=expected_targets,
        bins_by_target=bins_by_target,
        unsplit_targets=unsplit_targets,
        expected_components=expected_components,
    )

    print("xcodex-release preflight checks passed.")
    return 0


def parse_workflow_matrix_entries(workflow_src: str) -> list[dict[str, str]]:
    """Parse strategy.matrix.include entries from the workflow YAML.

    This parser intentionally handles only the subset used by our workflow:
    key/value scalar lines under `matrix.include`.
    """
    lines = workflow_src.splitlines()
    matrix_indent = None
    include_indent = None
    entries: list[dict[str, str]] = []
    current_entry: dict[str, str] | None = None
    entry_indent = None

    for line in lines:
        stripped = line.strip()
        indent = len(line) - len(line.lstrip(" "))

        if matrix_indent is None:
            if stripped == "matrix:":
                matrix_indent = indent
            continue

        if include_indent is None:
            if stripped and indent <= matrix_indent:
                matrix_indent = None
                continue
            if stripped == "include:":
                include_indent = indent
            continue

        if stripped and indent <= include_indent:
            if current_entry is not None:
                entries.append(current_entry)
            break

        if not stripped or stripped.startswith("#"):
            continue

        if stripped.startswith("- "):
            if current_entry is not None:
                entries.append(current_entry)
            current_entry = {}
            entry_indent = indent
            remainder = stripped[2:].strip()
            if remainder:
                key, separator, value = remainder.partition(":")
                if separator:
                    current_entry[key.strip()] = parse_yaml_scalar(value)
            continue

        if current_entry is None or entry_indent is None:
            continue

        if indent <= entry_indent:
            entries.append(current_entry)
            current_entry = None
            entry_indent = None
            continue

        key, separator, value = stripped.partition(":")
        if not separator:
            continue
        current_entry[key.strip()] = parse_yaml_scalar(value)

    if current_entry is not None:
        entries.append(current_entry)

    matrix_entries = []
    for entry in entries:
        runner = entry.get("runner")
        target = entry.get("target")
        if not runner or not target:
            continue
        matrix_entries.append(entry)
    return matrix_entries


def parse_yaml_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        value = value[1:-1]
    return value.strip()


def load_module(path: Path, module_name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"Unable to load module at {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def verify_artifact_lookup_coverage(
    *,
    installer: ModuleType,
    binary_targets: set[str],
    expected_targets: set[str],
    bins_by_target: dict[str, set[str]],
    unsplit_targets: set[str],
    expected_components: set[str],
) -> None:
    lookup_fn = getattr(installer, "artifact_subdirs_for_target", None)
    if not callable(lookup_fn):
        raise SystemExit(
            "install_native_deps.py missing artifact_subdirs_for_target helper for split artifact lookup."
        )

    probe_root = Path("/tmp/xcodex-preflight-artifacts")
    for component_name in sorted(expected_components):
        component = installer.BINARY_COMPONENTS[component_name]
        component_targets = set(component.targets or tuple(binary_targets))
        component_targets &= expected_targets
        workflow_job_bin = component.windows_job_bin or component.binary_basename

        for target in sorted(component_targets):
            subdir_names = {
                path.name for path in lookup_fn(probe_root, target, component)
            }
            if target not in subdir_names:
                raise SystemExit(
                    "install_native_deps.py must probe the base artifact directory "
                    f"for target {target} and component {component_name}."
                )

            target_bins = bins_by_target.get(target, set())
            if target_bins and target not in unsplit_targets:
                if workflow_job_bin not in target_bins:
                    raise SystemExit(
                        "Component to workflow split-bin mismatch for "
                        f"{component_name} on {target}: needs '{workflow_job_bin}', "
                        f"available bins={sorted(target_bins)}"
                    )

                split_subdir = f"{target}-{workflow_job_bin}"
                if split_subdir not in subdir_names:
                    raise SystemExit(
                        "install_native_deps.py does not probe split artifact folder "
                        f"'{split_subdir}' for component {component_name}."
                    )


if __name__ == "__main__":
    raise SystemExit(main())
