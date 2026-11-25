#!/usr/bin/env python3
"""Structural regression check for the rune-kit ScrcpyGUI shell.

Renders deterministic scenes and asserts the flat shell geometry, the device
head, the settings card, the action bar, the collapsed console and the drawer
states from Rune's `--tree` JSON.

Usage:
  python scripts/visual_tree_check.py                 # all six fixture scenes
  python scripts/visual_tree_check.py compare-dark   # one scene (CI/legacy form)
  python scripts/visual_tree_check.py --list         # list available scenes
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOL = 0.5

SCENES = [
    "compare-dark",
    "compare-light",
    "compare-camera",
    "compare-desktop",
    "compare-en",
    "compare-empty",
]

BASE_RECTS = {
    "topbar": [0, 0, 1200, 48],
    "rail": [0, 48, 264, 842],
    "main": [265, 48, 935, 842],
    "mode-row": [372.5, 72, 720, 28],
    "device-head": [372.5, 116, 720, 51],
    "settings-card": [372.5, 183, 720, 301],
    "advanced-toggle": [382.5, 450, 698, 24],
    "actionbar": [265, 784, 935, 61],
    "start-stop": [944.5, 797, 148, 36],
    "console": [265, 845, 935, 45],
    "open-settings": [1030, 11.5, 24, 24],
    "segment0": [372.5, 72, 120, 28],
    "segment1": [492.5, 72, 120, 28],
    "segment2": [612.5, 72, 120, 28],
}

SCENE_START = {
    "compare-camera": ("启用摄像头", "Engage Camera"),
    "compare-desktop": ("投射到桌面", "Eject to Desktop"),
    "compare-empty": ("选择设备", "Select device"),
}

SCENE_ADVANCED_COUNT = {
    "compare-camera": 7,
    "compare-desktop": 10,
}

COMMON_TEXTS_ZH = (
    "ScrcpyGUI",
    "设备中心",
    "设备",
    "引擎配置",
    "画面、编码与输入控制。",
    "输入与音频",
    "系统控制台",
    "改动将在下次启动时生效。",
)

COMMON_TEXTS_EN = (
    "ScrcpyGUI",
    "Device Hub",
    "Devices",
    "Engine Configuration",
    "Display, video and input controls.",
    "Input & audio",
    "System Console",
    "Changes apply the next time you start a session.",
)

RAW_KEY_PREFIXES = (
    "header.",
    "sidebar.",
    "controlPanel.",
    "sessionBehavior.",
    "logPanel.",
    "onboarding.",
    "footer.",
    "alerts.",
    "logs.",
    "common.",
    "shortcuts.",
    "device.",
    "advanced.",
    "settings.",
    "errorBoundary.",
)


def render(scene: str, script: list[dict] | None = None) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp) / "tree.json"
        argv = [
            "cargo",
            "run",
            "-q",
            "-p",
            "scrcpy-gui",
            "--",
            "--scene",
            scene,
            "--tree",
            str(tree),
        ]
        if script:
            script_path = Path(tmp) / "script.json"
            script_path.write_text(json.dumps(script), encoding="utf-8")
            argv += ["--script", str(script_path), "--at", "mark:state"]
        proc = subprocess.run(
            argv,
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if proc.returncode != 0:
            print("FAILED to render scene:", proc.stderr[-2000:], file=sys.stderr)
            raise SystemExit(2)
        return json.loads(tree.read_text(encoding="utf-8"))


def state_script(key: str) -> list[dict]:
    return [
        {"t": 0.02, "focus": True},
        {"t": 0.1, "type": "tap", "key": key},
        {"t": 0.7, "type": "mark", "name": "state"},
    ]


def check_scene(scene: str) -> tuple[int, list[str]]:
    tree = render(scene)
    english = scene == "compare-en"

    elements = tree.get("elements", [])
    by_key = {}
    for el in elements:
        key = el.get("key")
        if key and key not in by_key:
            by_key[key] = el

    failures: list[str] = []
    checked = 0

    def check(name: str, actual, expected) -> None:
        nonlocal checked
        checked += 1
        if actual != expected:
            failures.append(f"{name}: expected {expected!r}, got {actual!r}")

    def check_rect(key: str, expected: list[float]) -> None:
        el = by_key.get(key)
        if el is None:
            failures.append(f"missing element: {key}")
            return
        actual = el.get("rect")
        if actual is None or len(actual) != 4:
            failures.append(f"{key}: no rect")
            return
        for i, (a, e) in enumerate(zip(actual, expected)):
            check(f"{key}.rect[{i}]", abs(float(a) - float(e)) <= TOL, True)

    check("viewport", tree.get("viewport"), [1200.0, 890.0])
    check("tree warnings", tree.get("warnings"), [])
    for key, rect in BASE_RECTS.items():
        check_rect(key, rect)

    texts = [e.get("text") for e in elements if isinstance(e.get("text"), str)]
    labels = [e.get("label") for e in elements if isinstance(e.get("label"), str)]
    all_text = texts + labels

    for expected in COMMON_TEXTS_EN if english else COMMON_TEXTS_ZH:
        check(f"text[{expected}]", expected in texts, True)

    start = by_key.get("start-stop")
    if start is None:
        failures.append("missing start-stop")
    else:
        check("start-stop.role", start.get("role"), "button")
        zh_start, en_start = SCENE_START.get(scene, ("开始", "Start Mission"))
        check("start-stop.label", start.get("label"), en_start if english else zh_start)

    advanced = by_key.get("advanced-toggle")
    if advanced is None:
        failures.append("missing advanced-toggle")
    else:
        count = SCENE_ADVANCED_COUNT.get(scene, 6)
        expected = (f"Advanced settings · {count}" if english else f"高级设置 · {count}")
        check("advanced-toggle.label", advanced.get("label"), expected)
        check("advanced-toggle.collapsed", advanced.get("expanded"), False)

    if scene != "compare-empty":
        for expected in ("PJA110", "706fc5a1 · USB", "Recent Devices" if english else "最近设备"):
            check(f"connected text[{expected}]", expected in texts, True)

    if scene == "compare-empty":
        for expected in (
            "Connect your phone",
            "No device selected",
            "No devices detected",
        ) if english else (
            "连接你的手机",
            "未选择设备",
            "未检测到设备",
        ):
            check(f"empty text[{expected}]", expected in texts, True)
        if start is None:
            failures.append("missing start-stop for empty scene")
        else:
            check("start-stop disabled with no device", start.get("focusable"), False)
        check("empty connect CTA", "device-connect" in by_key, True)
        check("empty rail row", "no-devices" in by_key, True)
        check("empty scene has no history", "192.168.1.42:5555" in texts, False)
    else:
        check("device disconnect", "device-disconnect" in by_key, True)
        check("history row", "192.168.1.42:5555" in texts, True)

    check("console collapsed", by_key.get("console", {}).get("rect", [0, 0, 0, 0])[3], 45)
    check("console body absent", "console-lines" in by_key, False)
    check("advanced fields absent", "rotation-trigger" in by_key, False)
    check("settings gear", "open-settings" in by_key, True)

    leaked = [
        text
        for text in all_text
        if text == "?" or text.startswith(RAW_KEY_PREFIXES)
    ]
    check("no untranslated/raw-key text", leaked, [])

    if english:
        cjk = [
            text
            for text in all_text
            if any("\u4e00" <= ch <= "\u9fff" for ch in text)
        ]
        check("English scene has no Chinese text", cjk, [])

    # The settings drawer is an overlay below the titlebar, above a scrim.
    if scene == "compare-dark":
        drawer_tree = render(scene, state_script("open-settings"))
        drawer_by_key = {
            el.get("key"): el
            for el in drawer_tree.get("elements", [])
            if el.get("key")
        }
        drawer_texts = [
            el.get("text")
            for el in drawer_tree.get("elements", [])
            if isinstance(el.get("text"), str)
        ]
        check(
            "settings-scrim",
            drawer_by_key.get("settings-scrim", {}).get("rect"),
            [0.0, 48.0, 1200.0, 842.0],
        )
        check(
            "settings-drawer",
            drawer_by_key.get("settings-drawer", {}).get("rect"),
            [820.0, 48.0, 380.0, 842.0],
        )
        for expected in ("设置", "外观", "行为设置", "快捷键", "核心", "结束 ADB", "选择文件夹"):
            check(f"drawer text[{expected}]", expected in drawer_texts, True)
        check("drawer close button", "close-settings" in drawer_by_key, True)

        advanced_tree = render(scene, state_script("advanced-toggle"))
        advanced_by_key = {
            el.get("key"): el
            for el in advanced_tree.get("elements", [])
            if el.get("key")
        }
        advanced_texts = [
            el.get("text")
            for el in advanced_tree.get("elements", [])
            if isinstance(el.get("text"), str)
        ]
        check(
            "advanced-toggle.expanded",
            advanced_by_key.get("advanced-toggle", {}).get("expanded"),
            True,
        )
        check("advanced fields built", "rotation-trigger" in advanced_by_key, True)
        check("advanced vsync row", "垂直同步" in advanced_texts, True)
        check(
            "settings-card grows",
            advanced_by_key.get("settings-card", {}).get("rect", [0, 0, 0, 0])[3] > 301,
            True,
        )

        console_tree = render(scene, state_script("console-toggle"))
        console_by_key = {
            el.get("key"): el
            for el in console_tree.get("elements", [])
            if el.get("key")
        }
        check(
            "console expands",
            console_by_key.get("console", {}).get("rect", [0, 0, 0, 0])[3] > 100,
            True,
        )
        check("console body built", "console-lines" in console_by_key, True)
        for key in ("console-input", "console-export", "console-clear"):
            check(f"console key[{key}]", key in console_by_key, True)

    return checked, failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenes",
        nargs="*",
        help=f"one or more scenes (default: all six): {', '.join(SCENES)}",
    )
    parser.add_argument("--list", action="store_true", help="list scenes and exit")
    args = parser.parse_args()

    if args.list:
        for scene in SCENES:
            print(scene)
        return 0

    scenes = args.scenes or SCENES
    unknown = [scene for scene in scenes if scene not in SCENES]
    if unknown:
        parser.error(f"unknown scene(s): {', '.join(unknown)}")

    failed = 0
    total = 0
    for scene in scenes:
        checked, failures = check_scene(scene)
        total += checked
        if failures:
            failed += 1
            print(f"FAIL [{scene}] {len(failures)} of {checked} checks failed:")
            for line in failures:
                print("  -", line)
        else:
            print(f"PASS [{scene}] {checked} structural checks")

    if failed:
        print(f"\nFAIL {failed} of {len(scenes)} scene(s)")
        return 1
    print(f"\nPASS {len(scenes)} scene(s), {total} structural checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
