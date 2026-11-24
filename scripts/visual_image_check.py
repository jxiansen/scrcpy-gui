#!/usr/bin/env python3
"""Golden PNG regression for the Rune UI (layer 2).

Renders deterministic `--scene` fixtures, compares each PNG with a committed
golden, and writes a diff heatmap + report. Use --bless to update goldens after
a deliberate visual change.

Usage:
  python scripts/visual_image_check.py --bless
  python scripts/visual_image_check.py --max-mae 8
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from visual_diff import compare

ROOT = Path(__file__).resolve().parents[1]
GOLDEN = ROOT / "tests" / "visual" / "golden"
OUT = ROOT / "tests" / "visual" / "out"
SCENES = ["compare-dark", "compare-light", "compare-camera", "compare-desktop"]


def render(scene: str, target: Path) -> bool:
    proc = subprocess.run(
        ["cargo", "run", "-q", "-p", "scrcpy-gui", "--",
         "--scene", scene, "--snap", str(target)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        print(f"FAILED to render {scene}:\n{proc.stderr[-2000:]}", file=sys.stderr)
        return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bless", action="store_true", help="overwrite goldens with current renders")
    parser.add_argument("--max-mae", type=float, default=8.0)
    parser.add_argument("--threshold", type=int, default=16)
    args = parser.parse_args()

    GOLDEN.mkdir(parents=True, exist_ok=True)
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        for scene in SCENES:
            shot = tmpdir / f"{scene}.png"
            if not render(scene, shot):
                failures += 1
                continue
            golden = GOLDEN / f"{scene}.png"
            if args.bless:
                shutil.copyfile(shot, golden)
                print(f"BLESSED {scene} -> {golden.relative_to(ROOT)}")
                continue
            if not golden.exists():
                print(f"MISSING golden {golden.relative_to(ROOT)} (run with --bless)")
                failures += 1
                continue
            report = compare(shot, golden, OUT / scene, args.threshold)
            status = "PASS" if report["mae"] <= args.max_mae else "FAIL"
            print(
                f"{status} {scene}: MAE={report['mae']:.3f} "
                f"changed={report['changed_ratio'] * 100:.2f}% "
                f"size={report['size']}"
            )
            if status == "FAIL":
                failures += 1
                for region in report["top_regions"][:5]:
                    print(f"    cell({region['col']},{region['row']}) MAE={region['mae']}")

    if args.bless:
        return 0
    if failures:
        print(f"{failures} scene(s) failed; see {OUT.relative_to(ROOT)} for diff.png")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
