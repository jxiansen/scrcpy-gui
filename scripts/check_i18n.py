#!/usr/bin/env python3
"""Validate the eight locale catalogs against the English source of truth.

Checks that every catalog parses, has exactly the same key set as en.json,
holds string values, and uses the same {var} placeholders. Empty translations
are reported as warnings rather than errors: the runtime falls back to English,
but the gap is worth seeing.

Usage: python scripts/check_i18n.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CATALOGS = ROOT / "rune-app" / "src" / "i18n" / "catalogs"
REFERENCE = "en.json"
PLACEHOLDER = re.compile(r"\{[A-Za-z0-9_]+\}")


def load(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot parse {path.name}: {exc}") from exc
    if not isinstance(data, dict):
        raise ValueError(f"{path.name}: top level must be a JSON object")
    return data


def main() -> int:
    files = sorted(CATALOGS.glob("*.json"))
    if REFERENCE not in [path.name for path in files]:
        print(f"FAIL i18n: missing reference catalog {REFERENCE}")
        return 1

    try:
        reference = load(CATALOGS / REFERENCE)
    except ValueError as exc:
        print(f"FAIL i18n: {exc}")
        return 1

    ref_keys = set(reference)
    failures: list[str] = []
    warnings: list[str] = []

    for path in files:
        try:
            data = load(path)
        except ValueError as exc:
            failures.append(str(exc))
            continue

        keys = set(data)
        missing = sorted(ref_keys - keys)
        extra = sorted(keys - ref_keys)
        if missing:
            failures.append(
                f"{path.name}: missing {len(missing)} key(s): {', '.join(missing[:5])}"
            )
        if extra:
            failures.append(
                f"{path.name}: {len(extra)} key(s) absent from {REFERENCE}: "
                f"{', '.join(extra[:5])}"
            )

        for key, value in data.items():
            if not isinstance(value, str):
                failures.append(f"{path.name}: {key} is not a string")
                continue
            if not value.strip():
                warnings.append(
                    f"{path.name}: {key} is empty (runtime falls back to English)"
                )
                continue
            expected = reference.get(key)
            if isinstance(expected, str) and PLACEHOLDER.findall(value) != PLACEHOLDER.findall(
                expected
            ):
                failures.append(
                    f"{path.name}: {key} has different {{var}} placeholders than {REFERENCE}"
                )

    for warning in warnings:
        print(f"WARN i18n: {warning}")
    if failures:
        print(f"FAIL i18n: {len(failures)} problem(s):")
        for failure in failures:
            print("  -", failure)
        return 1

    print(
        f"PASS i18n: {len(files)} catalogs, {len(ref_keys)} keys each, "
        f"{len(warnings)} empty translation(s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
