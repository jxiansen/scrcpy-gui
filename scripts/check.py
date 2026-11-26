#!/usr/bin/env python3
"""One entry point for every local and CI check.

The same script runs on a developer machine and in the PR workflow, so the gate
cannot drift between the two. CI uses `--no-golden` on Linux because PNG goldens
are font-rasterisation dependent and were captured on Windows.

Usage:
  python scripts/check.py                       # full gate (Windows CI / local)
  python scripts/check.py --no-golden           # skip PNG goldens (Linux CI)
  python scripts/check.py --fast                # Rust + consistency checks only
  python scripts/check.py --visual-only         # only the two visual layers
  python scripts/check.py --scene compare-dark  # limit structural fixtures
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "scripts"
WORKFLOWS = ROOT / ".github" / "workflows"
MANIFEST = ROOT / "rune-app" / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"
TOOLCHAIN = ROOT / "rust-toolchain.toml"
WORKSPACE = ROOT / "Cargo.toml"


def consistency_problems() -> list[str]:
    """Static invariants that are easy to break and annoying to debug in CI."""
    problems: list[str] = []

    # 1. CI must install exactly the toolchain pinned in rust-toolchain.toml.
    channel_match = re.search(
        r'^channel\s*=\s*"([^"]+)"', TOOLCHAIN.read_text(encoding="utf-8"), re.M
    )
    if channel_match is None:
        return ["rust-toolchain.toml: no `channel` found"]
    channel = channel_match.group(1)
    pins = 0
    for workflow in sorted(WORKFLOWS.glob("*.yml")):
        text = workflow.read_text(encoding="utf-8")
        for match in re.finditer(r'^\s*toolchain:\s*([^\s#]+)', text, re.M):
            pins += 1
            if match.group(1) != channel:
                problems.append(
                    f"{workflow.name}: toolchain {match.group(1)} != "
                    f"rust-toolchain.toml ({channel})"
                )
    if pins == 0:
        problems.append("no workflow pins the Rust toolchain")

    # 2. Rune's git deps and its eight [patch.crates-io] forks share one rev.
    revs = re.findall(
        r'\brev\s*=\s*"([0-9a-f]{7,40})"', WORKSPACE.read_text(encoding="utf-8")
    )
    if not revs:
        problems.append("Cargo.toml: no Rune `rev` pins found")
    elif len(set(revs)) != 1:
        problems.append(
            f"Cargo.toml: {len(set(revs))} different Rune revisions: {sorted(set(revs))}"
        )

    # 3. A version bump must not leave Cargo.lock behind.
    manifest_version = re.search(
        r'^version = "(\d+\.\d+\.\d+)"', MANIFEST.read_text(encoding="utf-8"), re.M
    )
    lock_version = re.search(
        r'\[\[package\]\]\nname = "scrcpy-gui"\nversion = "(\d+\.\d+\.\d+)"',
        LOCK.read_text(encoding="utf-8"),
    )
    if manifest_version is None or lock_version is None:
        problems.append("could not read the package version from Cargo.toml / Cargo.lock")
    elif manifest_version.group(1) != lock_version.group(1):
        problems.append(
            f"version mismatch: Cargo.toml {manifest_version.group(1)} vs "
            f"Cargo.lock {lock_version.group(1)} (run scripts/bump_version.py)"
        )

    return problems


def run(name: str, cmd: list[str]) -> bool:
    print(f"\n→ {name}: {' '.join(cmd)}", flush=True)
    started = time.monotonic()
    result = subprocess.run(cmd, cwd=ROOT)
    elapsed = time.monotonic() - started
    if result.returncode != 0:
        print(f"FAIL {name} (exit {result.returncode}, {elapsed:.1f}s)")
        print(f"     reproduce: {' '.join(cmd)}")
        return False
    print(f"PASS {name} ({elapsed:.1f}s)")
    return True


def build_steps(args: argparse.Namespace) -> list[tuple[str, list[str]]]:
    steps: list[tuple[str, list[str]]] = []
    if not args.visual_only:
        steps += [
            ("formatting", ["cargo", "fmt", "--all", "--", "--check"]),
            (
                "clippy",
                ["cargo", "clippy", "--all-targets", "--locked", "--", "-D", "warnings"],
            ),
            ("unit tests", ["cargo", "test", "--all-targets", "--locked"]),
            ("build", ["cargo", "build", "--all-targets", "--locked"]),
            ("i18n catalogs", [sys.executable, str(SCRIPTS / "check_i18n.py")]),
        ]

    if not args.fast:
        tree = [sys.executable, str(SCRIPTS / "visual_tree_check.py")]
        if args.scene:
            tree += args.scene
        steps.append(("structural fixtures", tree))
        if not args.no_golden:
            steps.append(
                (
                    "PNG goldens",
                    [
                        sys.executable,
                        str(SCRIPTS / "visual_image_check.py"),
                        "--max-mae",
                        "8",
                    ],
                )
            )
    return steps


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fast", action="store_true", help="skip both visual layers")
    parser.add_argument("--no-golden", action="store_true", help="skip the PNG goldens")
    parser.add_argument("--visual-only", action="store_true", help="skip all Rust checks")
    parser.add_argument(
        "--scene",
        action="append",
        help="run only this structural scene (repeatable; default: all seven)",
    )
    args = parser.parse_args()
    if args.fast and args.visual_only:
        parser.error("--fast and --visual-only are mutually exclusive")
    if args.fast and (args.scene or args.no_golden):
        parser.error("--fast already skips the visual layers")

    print("=== Consistency checks ===", flush=True)
    problems = consistency_problems()
    if problems:
        for problem in problems:
            print("  -", problem)
        print("FAIL consistency")
        return 1
    print("PASS consistency (toolchain, Rune rev pins, manifest/lock version)")

    for name, cmd in build_steps(args):
        if not run(name, cmd):
            print("\nGATE FAILED")
            return 1
    print("\nGATE PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
