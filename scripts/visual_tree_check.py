#!/usr/bin/env python3
"""Structural visual regression check for the Rune UI.

Renders a deterministic `--scene` and asserts key geometry, colours and type
from Rune's `--tree` JSON. Unlike a PNG diff this is immune to font rasteriser
differences, so it is suitable as a hard CI gate.

Usage:  python scripts/visual_tree_check.py [scene]   (default: compare-dark)
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOL = 0.5

EXPECTED_RECTS = {
    "kit-root/window": [0, 0, 1200, 890],
    "kit-root/window/header": [0, 0, 1200, 84],
    "kit-root/window/workspace": [0, 84, 1200, 734],
    "kit-root/window/footer": [0, 818, 1200, 72],
    "kit-root/window/workspace/sidebar": [24, 100, 270, 710],
    "kit-root/window/workspace/center": [318, 100, 564, 710],
    "kit-root/window/workspace/right": [906, 100, 270, 710],
    "kit-root/window/workspace/center/console": [318, 600, 564, 220],
    "kit-root/window/workspace/center/engine/#1/start-stop": [328, 528, 544, 52],
}

EXPECTED_FILLS = {
    "kit-root/window": "#06040aff",       # ultraviolet dark bg
    "kit-root/window/workspace/center/console": "#09090bf0",
    "kit-root/window/workspace/center/engine/#1/start-stop": "#8b5cf6ff",
}

EXPECTED_RADII = {
    "kit-root/window/workspace/center/console": 16.0,
    "kit-root/window/workspace/center/engine/#1/start-stop": 16.0,
}

EXPECTED_TEXT = {
    "SCRCPY": {"size": 30.0, "weight": 900.0, "tracking": -1.5, "italic": True},
    "GUI": {"size": 30.0, "weight": 900.0, "tracking": -1.5, "italic": False},
    "V4.1.0": {"size": 10.0, "weight": 900.0, "tracking": 0.6},
}

RAW_KEY_PREFIXES = (
    "header.", "sidebar.", "controlPanel.", "sessionBehavior.", "logPanel.",
    "onboarding.", "footer.", "alerts.", "logs.", "common.", "shortcuts.",
    "errorBoundary.",
)


def main() -> int:
    scene = sys.argv[1] if len(sys.argv) > 1 else "compare-dark"
    with tempfile.TemporaryDirectory() as tmp:
        tree_path = Path(tmp) / "tree.json"
        proc = subprocess.run(
            ["cargo", "run", "-q", "-p", "scrcpy-gui", "--",
             "--scene", scene, "--tree", str(tree_path)],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            print("FAILED to render scene:", proc.stderr[-2000:], file=sys.stderr)
            return 2
        tree = json.loads(tree_path.read_text(encoding="utf-8"))

    elements = tree.get("elements", [])
    by_path = {}
    for el in elements:
        by_path.setdefault(el.get("path"), el)

    failures: list[str] = []
    checked = 0

    def check(name: str, actual, expected) -> None:
        nonlocal checked
        checked += 1
        if actual != expected:
            failures.append(f"{name}: expected {expected!r}, got {actual!r}")

    def check_rect(path: str, expected: list[float]) -> None:
        el = by_path.get(path)
        if el is None:
            failures.append(f"missing element: {path}")
            return
        actual = el.get("rect")
        if actual is None or len(actual) != 4:
            failures.append(f"{path}: no rect")
            return
        for i, (a, e) in enumerate(zip(actual, expected)):
            check(f"{path}.rect[{i}]", abs(float(a) - float(e)) <= TOL, True)

    check("viewport", tree.get("viewport"), [1200.0, 890.0])
    for path, rect in EXPECTED_RECTS.items():
        check_rect(path, rect)
    for path, fill in EXPECTED_FILLS.items():
        el = by_path.get(path)
        if el is None:
            failures.append(f"missing element: {path}")
        else:
            check(f"{path}.fill", el.get("fill"), fill)
    for path, radius in EXPECTED_RADII.items():
        el = by_path.get(path)
        if el is None:
            failures.append(f"missing element: {path}")
        else:
            check(f"{path}.radius", el.get("radius"), radius)

    for text, expected_style in EXPECTED_TEXT.items():
        candidates = [e for e in elements if e.get("text") == text]
        if not candidates:
            failures.append(f"missing text: {text}")
            continue
        style = candidates[0].get("textStyle") or {}
        for key, value in expected_style.items():
            check(f"text[{text}].{key}", style.get(key), value)

    leaked = [
        e.get("text")
        for e in elements
        if isinstance(e.get("text"), str)
        and (e["text"] == "?" or e["text"].startswith(RAW_KEY_PREFIXES))
    ]
    check("no untranslated/raw-key text", leaked, [])

    diamonds = [e.get("text") for e in elements if e.get("text") == "◆"]
    check("no missing-glyph diamonds in shortcut grid", diamonds, [])
    targets = [e for e in elements if e.get("text") == "706fc5a1 · USB"]
    check("session target line present", len(targets) > 0, True)

    if failures:
        print(f"FAIL [{scene}] {len(failures)} of {checked} checks failed:")
        for line in failures:
            print("  -", line)
        return 1
    print(f"PASS [{scene}] {checked} structural checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
