#!/usr/bin/env python3
"""Verify Bazel `compile_data` covers files referenced via include_str!/include_bytes!.

This protects against drift where Cargo builds work (unsandboxed file reads) but Bazel
sandboxed builds fail because referenced non-Rust files are not declared as inputs.

Scope: only handles literal include paths like include_str!("path").
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import fnmatch
import re
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]

INCLUDE_RE = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')


@dataclass(frozen=True)
class IncludeRef:
    src_file: Path
    include_path: str
    resolved_path: Path


def find_repo_root(start: Path) -> Path:
    cur = start
    while True:
        if (cur / ".git").exists():
            return cur
        if cur.parent == cur:
            raise RuntimeError("failed to find repo root")
        cur = cur.parent


def nearest_bazel_package_dir(path: Path) -> Path | None:
    cur = path
    while True:
        if (cur / "BUILD.bazel").exists():
            return cur
        if cur == REPO_ROOT:
            return None
        if cur.parent == cur:
            return None
        cur = cur.parent




def extract_compile_data_globs(build_text: str) -> list[str]:
    # Heuristic: capture compile_data = glob(["..."]) and compile_data = glob([ ... ]).
    m = re.search(r"compile_data\s*=\s*glob\(\s*\[([^\]]*)\]", build_text, flags=re.DOTALL)
    if not m:
        return []
    body = m.group(1)
    return re.findall(r'"([^"]+)"', body)


def globs_match_path(globs: list[str], rel_path: str) -> bool:
    return any(fnmatch.fnmatch(rel_path, g) for g in globs)
def bazel_build_has_all_files_glob(build_text: str) -> bool:
    # Heuristic: accept compile_data = glob(include = ["**"], ...)
    return (
        "compile_data = glob(" in build_text
        and 'include = ["**"]' in build_text
        and "compile_data" in build_text
    )


def bazel_build_mentions_file(build_text: str, rel_path: str) -> bool:
    # Heuristic: accept explicit list entries.
    return f'"{rel_path}"' in build_text


def collect_includes() -> list[IncludeRef]:
    includes: list[IncludeRef] = []
    for src_file in (REPO_ROOT / "codex-rs").rglob("*.rs"):
        try:
            text = src_file.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue

        for match in INCLUDE_RE.finditer(text):
            include_path = match.group(1)
            # Skip non-literal or macro-generated paths.
            if "{" in include_path or "}" in include_path:
                continue
            resolved = (src_file.parent / include_path).resolve()
            includes.append(
                IncludeRef(
                    src_file=src_file,
                    include_path=include_path,
                    resolved_path=resolved,
                )
            )
    return includes


def main() -> int:
    global REPO_ROOT
    REPO_ROOT = find_repo_root(Path.cwd())

    problems: list[str] = []

    for inc in collect_includes():
        if "tests" in inc.src_file.parts:
            continue

        if not inc.resolved_path.exists():
            problems.append(
                f"missing included file: {inc.src_file.relative_to(REPO_ROOT)} includes {inc.include_path} (resolved {inc.resolved_path.relative_to(REPO_ROOT)})"
            )
            continue

        pkg_dir = nearest_bazel_package_dir(inc.src_file.parent)
        if pkg_dir is None:
            problems.append(
                f"no BUILD.bazel found for {inc.src_file.relative_to(REPO_ROOT)} (needed for include {inc.include_path})"
            )
            continue

        try:
            rel_to_pkg = inc.resolved_path.relative_to(pkg_dir)
        except ValueError:
            problems.append(
                f"included file escapes Bazel package: {inc.src_file.relative_to(REPO_ROOT)} includes {inc.include_path} -> {inc.resolved_path.relative_to(REPO_ROOT)} (package {pkg_dir.relative_to(REPO_ROOT)})"
            )
            continue

        build_text = (pkg_dir / "BUILD.bazel").read_text(encoding="utf-8")

        if bazel_build_has_all_files_glob(build_text):
            continue

        rel_str = rel_to_pkg.as_posix()
        if bazel_build_mentions_file(build_text, rel_str):
            continue

        compile_globs = extract_compile_data_globs(build_text)
        if compile_globs and globs_match_path(compile_globs, rel_str):
            continue

        problems.append(
            f"Bazel compile_data may be missing {rel_str} for package {pkg_dir.relative_to(REPO_ROOT)} (referenced by {inc.src_file.relative_to(REPO_ROOT)} via include_{'str' if 'include_str!' in inc.include_path else '...'}(\"{inc.include_path}\"))"
        )

    if problems:
        print("Bazel compile_data check failed:")
        for p in problems:
            print(f"- {p}")
        return 1

    print("Bazel compile_data check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
