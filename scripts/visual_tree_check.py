#!/usr/bin/env python3
"""Structural visual regression check for the rune-kit shell UI.

Renders a deterministic `--scene` and asserts the shell/sidebar/main/inspector
geometry and the key controls from Rune's `--tree` JSON. This is the hard CI
gate; the PNG check is the softer pixel-regression layer.

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
SHELL = "rune-frame/kit-root/shell/split"

EXPECTED_RECTS = {
    "rune-frame": [0, 0, 1200, 890],
    SHELL: [0, 0, 1200, 890],
    f"{SHELL}/sidebar-pane": [0, 0, 286, 890],
    f"{SHELL}/sidebar-pane/pane-content/pane-body/sidebar": [0, 48, 286, 842],
    f"{SHELL}/main-column/toolbar": [286, 0, 914, 48],
    f"{SHELL}/main-column/main-row/main-pane": [286, 48, 606, 834],
    f"{SHELL}/main-column/main-row/main-pane/main-content": [286, 48, 606, 834],
    f"{SHELL}/main-column/main-row/main-pane/main-content/engine": [302, 108, 574, 397],
    f"{SHELL}/main-column/main-row/main-pane/main-content/start-stop": [302, 521, 574, 36],
    f"{SHELL}/main-column/main-row/main-pane/main-content/console": [302, 573, 574, 233],
    f"{SHELL}/main-column/main-row/inspector-pane": [892, 48, 308, 842],
    f"{SHELL}/main-column/main-row/inspector-pane/pane-content/pane-body/inspector/appearance": [908, 64, 276, 133],
    f"{SHELL}/main-column/main-row/inspector-pane/pane-content/pane-body/inspector/behavior": [908, 213, 276, 257],
    f"{SHELL}/main-column/main-row/inspector-pane/pane-content/pane-body/inspector/shortcuts": [908, 486, 276, 189],
}

EXPECTED_TEXT_ZH = (
    "ScrcpyGUI",
    "屏幕",
    "摄像头",
    "桌面",
    "系统控制台",
    "设备中心",
    "主题",
    "行为设置",
    "快捷键",
)

EXPECTED_TEXT_EN = (
    "ScrcpyGUI",
    "Screen",
    "Camera",
    "Desktop",
    "System Console",
    "Device Hub",
    "Theme",
    "Session Behavior",
    "Shortcuts",
)

# Camera and desktop carry extra configuration rows, so their cards push the
# session button and console further down the scrollable main column.
SCENE_RECTS = {
    "compare-camera": {
        f"{SHELL}/main-column/main-row/main-pane/main-content/engine": [302, 108, 574, 453],
        f"{SHELL}/main-column/main-row/main-pane/main-content/start-stop": [302, 577, 574, 36],
        f"{SHELL}/main-column/main-row/main-pane/main-content/console": [302, 629, 574, 233],
    },
    "compare-desktop": {
        f"{SHELL}/main-column/main-row/main-pane/main-content/engine": [302, 108, 574, 445],
        f"{SHELL}/main-column/main-row/main-pane/main-content/start-stop": [302, 569, 574, 36],
        f"{SHELL}/main-column/main-row/main-pane/main-content/console": [302, 621, 574, 233],
    },
}

SCENE_START_TEXT = {
    "compare-camera": ("启用摄像头", "Enable Camera"),
    "compare-desktop": ("投射到桌面", "Cast to Desktop"),
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
    check("tree warnings", tree.get("warnings"), [])
    for path, rect in {**EXPECTED_RECTS, **SCENE_RECTS.get(scene, {})}.items():
        check_rect(path, rect)

    start = by_path.get(f"{SHELL}/main-column/main-row/main-pane/main-content/start-stop")
    if start is None:
        failures.append("missing start-stop")
    else:
        check("start-stop.role", start.get("role"), "button")

    texts = [e.get("text") for e in elements if isinstance(e.get("text"), str)]
    english = scene == "compare-en"
    expected_text = EXPECTED_TEXT_EN if english else EXPECTED_TEXT_ZH
    for expected in expected_text:
        check(f"text[{expected}]", expected in texts, True)
    zh_start, en_start = SCENE_START_TEXT.get(scene, ("开始", "Start Mission"))
    start_text = en_start if english else zh_start
    check(f"text[{start_text}]", start_text in texts, True)
    check("session target line present", "706fc5a1 · USB" in texts, scene != "compare-empty")

    leaked = [
        t for t in texts
        if t == "?" or t.startswith(RAW_KEY_PREFIXES)
    ]
    check("no untranslated/raw-key text", leaked, [])

    if scene == "compare-en":
        cjk = [
            t for t in texts
            if any("\u4e00" <= ch <= "\u9fff" for ch in t)
        ]
        check("English scene has no Chinese text", cjk, [])

    if scene == "compare-empty":
        if start is None:
            failures.append("missing start-stop for empty scene")
        else:
            check("start-stop disabled with no device", start.get("focusable"), False)

    if failures:
        print(f"FAIL [{scene}] {len(failures)} of {checked} checks failed:")
        for line in failures:
            print("  -", line)
        return 1
    print(f"PASS [{scene}] {checked} structural checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
