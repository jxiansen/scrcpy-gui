#!/usr/bin/env python3
"""Bump the package version in both rune-app/Cargo.toml and Cargo.lock.

Keeping the two files in sync here avoids a version-bump PR that leaves the
lockfile stale (which breaks every `--locked` build). It does not run cargo, so
unrelated dependency versions are never refreshed.

stdout carries exactly one line, `version=<new>`, for GitHub Actions'
$GITHUB_OUTPUT. Human-readable output goes to stderr.

Usage:
  python scripts/bump_version.py [patch|minor|major]
  python scripts/bump_version.py minor --dry-run
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "rune-app" / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"

MANIFEST_VERSION = re.compile(r'^version = "(\d+\.\d+\.\d+)"[ \t]*$', re.M)
LOCK_VERSION = re.compile(
    r'(\[\[package\]\]\nname = "scrcpy-gui"\nversion = ")(\d+\.\d+\.\d+)(")',
    re.M,
)


def write_text(path: Path, text: str) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)


def fail(message: str) -> int:
    print(f"FAIL bump_version: {message}", file=sys.stderr)
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bump", nargs="?", default="patch", choices=["patch", "minor", "major"])
    parser.add_argument("--dry-run", action="store_true", help="print the result, write nothing")
    args = parser.parse_args()

    manifest = MANIFEST.read_text(encoding="utf-8")
    manifest_match = MANIFEST_VERSION.search(manifest)
    if manifest_match is None:
        return fail(f"no version line found in {MANIFEST.name}")

    old = manifest_match.group(1)
    major, minor, patch = (int(part) for part in old.split("."))
    if args.bump == "major":
        major, minor, patch = major + 1, 0, 0
    elif args.bump == "minor":
        minor, patch = minor + 1, 0
    else:
        patch += 1
    new = f"{major}.{minor}.{patch}"

    lock = LOCK.read_text(encoding="utf-8")
    lock_match = LOCK_VERSION.search(lock)
    if lock_match is None:
        return fail(f"no scrcpy-gui package entry found in {LOCK.name}")
    if lock_match.group(2) != old:
        return fail(
            f"Cargo.lock has {lock_match.group(2)} but the manifest has {old}; "
            "fix the lockfile before bumping"
        )

    new_manifest = (
        manifest[: manifest_match.start(1)] + new + manifest[manifest_match.end(1) :]
    )
    new_lock = lock[: lock_match.start(2)] + new + lock[lock_match.end(2) :]

    if not args.dry_run:
        write_text(MANIFEST, new_manifest)
        write_text(LOCK, new_lock)
        # Read back: a partial write must never be reported as a successful bump.
        for path, version_re, expected in (
            (MANIFEST, MANIFEST_VERSION, new),
            (LOCK, LOCK_VERSION, new),
        ):
            match = version_re.search(path.read_text(encoding="utf-8"))
            if match is None:
                return fail(f"write verification failed for {path.name}")
            group = match.group(1) if version_re is MANIFEST_VERSION else match.group(2)
            if group != expected:
                return fail(f"write verification failed for {path.name}")

    # stdout is redirected into $GITHUB_OUTPUT by the workflow; keep it clean.
    print(f"version={new}")
    suffix = " (dry run)" if args.dry_run else ""
    print(
        f"bumped scrcpy-gui {old} -> {new} in Cargo.toml and Cargo.lock{suffix}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
