#!/usr/bin/env python3
"""Structural regression check for the rune-kit ScrcpyGUI shell.

Renders deterministic scenes and asserts the flat shell geometry, the device
head, the settings card, the action bar, the collapsed console and the drawer
states from Rune's `--tree` JSON.

Usage:
  python scripts/visual_tree_check.py                 # all seven fixture scenes
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
    "compare-ar",
]

# Regenerated from --tree after the main content became top-aligned and the
# settings card grew to fill the remaining height. See docs/ui-ux-audit.md.
BASE_RECTS = {
    "topbar": [0, 0, 1200, 48],
    "rail": [0, 48, 264, 842],
    "main": [265, 48, 935, 842],
    "mode-row": [372.5, 72, 720, 28],
    "device-head": [372.5, 116, 720, 51],
    "settings-card": [372.5, 183, 720, 585],
    "advanced-toggle": [380.5, 492, 700, 24],
    "actionbar": [265, 784, 935, 61],
    "start-stop": [972.5, 797, 120, 36],
    "console": [265, 845, 935, 45],
    "open-settings": [1030, 11.5, 24, 24],
    "segment0": [372.5, 72, 120, 28],
    "segment1": [492.5, 72, 120, 28],
    "segment2": [612.5, 72, 120, 28],
}

SCENE_START = {
    "compare-camera": ("启用摄像头", "Start Camera"),
    "compare-desktop": ("投射到桌面", "Start Desktop"),
    "compare-empty": ("选择设备", "Select device"),
}

SCENE_ADVANCED_COUNT = {
    "compare-camera": 7,
    "compare-desktop": 10,
}
# Mirror no longer duplicates "codec" in the advanced section.
DEFAULT_ADVANCED_COUNT = 5

COMMON_TEXTS_ZH = (
    "ScrcpyGUI",
    "设备中心",
    "设备",
    "引擎配置",
    "输入与音频",
    "系统控制台",
)

COMMON_TEXTS_EN = (
    "ScrcpyGUI",
    "Device Hub",
    "Devices",
    "Engine Configuration",
    "Input & audio",
    "System Console",
)

CARD_SUB_ZH = {
    "compare-camera": "摄像头设备、画面与音频控制。",
    "compare-desktop": "虚拟显示器尺寸与画面控制。",
}
CARD_SUB_EN = {
    "compare-camera": "Camera, video and audio controls.",
    "compare-desktop": "Virtual display size and video controls.",
}
CARD_SUB_MIRROR_ZH = "画面、编码与输入控制。"
CARD_SUB_MIRROR_EN = "Display, video and input controls."
MODE_NOTE_ZH = "会话参数改动将在下次启动时生效。"
MODE_NOTE_EN = "Session settings apply the next time you start a session."

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


def escape_script() -> list[dict]:
    """Open the settings drawer, then press Escape and capture the result."""
    return [
        {"t": 0.02, "focus": True},
        {"t": 0.1, "type": "tap", "key": "open-settings"},
        {"t": 0.4, "type": "key", "name": "Escape"},
        {"t": 0.8, "type": "mark", "name": "state"},
    ]


def check_scene(scene: str) -> tuple[int, list[str]]:
    tree = render(scene)
    english = scene == "compare-en"
    arabic = scene == "compare-ar"

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

    texts = [e.get("text") for e in elements if isinstance(e.get("text"), str)]
    labels = [e.get("label") for e in elements if isinstance(e.get("label"), str)]
    all_text = texts + labels

    if arabic:
        # compare-ar is an RTL locale scene. Its copy is Arabic and its geometry
        # may mirror, so the zh/en baselines do not apply: check safety only.
        check("viewport", tree.get("viewport"), [1200.0, 890.0])
        check("tree warnings", tree.get("warnings"), [])
        vw, vh = tree.get("viewport", [0.0, 0.0])
        offscreen = [
            e.get("key")
            for e in elements
            if (rect := e.get("rect"))
            and len(rect) == 4
            and (
                rect[0] < -1
                or rect[1] < -1
                or rect[0] + rect[2] > vw + 1
                or rect[1] + rect[3] > vh + 1
            )
        ]
        check("no offscreen elements", offscreen, [])
        leaked_ar = [
            text
            for text in all_text
            if text == "?" or text.startswith(RAW_KEY_PREFIXES)
        ]
        check("no untranslated/raw-key text", leaked_ar, [])
        unlabeled = [
            e.get("key")
            for e in elements
            if e.get("role") == "button" and not e.get("label") and not e.get("text")
        ]
        check("no unlabeled buttons", unlabeled, [])
        check("element count", len(elements) > 100, True)
        return checked, failures

    check("viewport", tree.get("viewport"), [1200.0, 890.0])
    check("tree warnings", tree.get("warnings"), [])
    rects = dict(BASE_RECTS)
    if scene == "compare-desktop":
        # Desktop's fields are sliders rather than select buttons, so the
        # advanced row sits one row-height higher than in other modes.
        rects["advanced-toggle"] = [380.5, 472, 700, 24]
    for key, rect in rects.items():
        check_rect(key, rect)

    common = list(COMMON_TEXTS_EN if english else COMMON_TEXTS_ZH)
    card_sub = (CARD_SUB_EN if english else CARD_SUB_ZH).get(
        scene, CARD_SUB_MIRROR_EN if english else CARD_SUB_MIRROR_ZH
    )
    common.append(card_sub)
    for expected in common:
        check(f"text[{expected}]", expected in texts, True)

    mode_note = MODE_NOTE_EN if english else MODE_NOTE_ZH
    check("session-settings hint is not stale", mode_note in texts, False)

    start = by_key.get("start-stop")
    if start is None:
        failures.append("missing start-stop")
    else:
        check("start-stop.role", start.get("role"), "button")
        zh_start, en_start = SCENE_START.get(scene, ("开始", "Start"))
        check("start-stop.label", start.get("label"), en_start if english else zh_start)

    advanced = by_key.get("advanced-toggle")
    if advanced is None:
        failures.append("missing advanced-toggle")
    else:
        count = SCENE_ADVANCED_COUNT.get(scene, DEFAULT_ADVANCED_COUNT)
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

    unlabeled = [
        e.get("key")
        for e in elements
        if e.get("role") == "button" and not e.get("label") and not e.get("text")
    ]
    check("no unlabeled buttons", unlabeled, [])

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
        for expected in ("设置", "外观", "行为设置", "核心", "结束 ADB", "选择文件夹"):
            check(f"drawer text[{expected}]", expected in drawer_texts, True)
        check("shortcuts no longer in the drawer", "快捷键" in drawer_texts, False)
        check("drawer close button", "close-settings" in drawer_by_key, True)

        about_tree = render(
            scene,
            [
                {"t": 0.02, "focus": True},
                {"t": 0.1, "type": "tap", "key": "about"},
                {"t": 0.8, "type": "mark", "name": "state"},
            ],
        )
        about_texts = [
            el.get("text")
            for el in about_tree.get("elements", [])
            if isinstance(el.get("text"), str)
        ]
        check("shortcuts moved into About", "快捷键" in about_texts, True)
        check(
            "About keeps the version line",
            any("ScrcpyGUI v" in text for text in about_texts),
            True,
        )

        locale_tree = render(
            scene,
            [
                {"t": 0.02, "focus": True},
                {"t": 0.1, "type": "tap", "key": "open-settings"},
                {"t": 0.35, "type": "tap", "key": "locale-trigger"},
                {"t": 0.8, "type": "mark", "name": "state"},
            ],
        )
        locale_texts = [
            el.get("text")
            for el in locale_tree.get("elements", [])
            if isinstance(el.get("text"), str)
        ]
        check("language dialog opens", "语言" in locale_texts, True)
        check("language options are readable", "English" in locale_texts, True)
        check("CJK locale option is readable", "简体中文" in locale_texts, True)

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

        esc_tree = render(scene, escape_script())
        esc_keys = {
            el.get("key")
            for el in esc_tree.get("elements", [])
            if el.get("key")
        }
        check("Escape closes the settings drawer", "settings-drawer" in esc_keys, False)

        connect_tree = render(scene, state_script("open-connect"))
        connect_keys = {
            el.get("key")
            for el in connect_tree.get("elements", [])
            if el.get("key")
        }
        check("USB page offers a refresh action", "connect-refresh" in connect_keys, True)
        check("USB page hides the IP form", "connect-ip" in connect_keys, False)

    return checked, failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "scenes",
        nargs="*",
        help=f"one or more scenes (default: all seven): {', '.join(SCENES)}",
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
