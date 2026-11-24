# Visual verification layers

Three complementary checks, from deterministic to human-in-the-loop.

## Layer 1 — structural assertions (CI gate)

```bash
python scripts/visual_tree_check.py compare-dark
```

Renders a deterministic fixture scene and asserts key rects, fills, radii, font
styles and text from Rune's `--tree` JSON. This is the hard gate because it is
immune to font rasterisation differences.

Fixtures (set with `--scene`): `compare-dark`, `compare-light`, `compare-camera`,
`compare-desktop`. They freeze theme, mode, locale, device/mDNS lists, logs and
the terminal timestamp, and skip all ADB/Prefs work.

## Layer 2 — golden PNG regression

```bash
python scripts/visual_image_check.py            # compare with committed goldens
python scripts/visual_image_check.py --bless    # update goldens after a deliberate change
```

Renders the same four fixtures with `--snap`, compares each 2400×1780 PNG with
`tests/visual/golden/<scene>.png`, and writes a heatmap + JSON report to
`tests/visual/out/<scene>/`. Threshold: MAE ≤ 8 (default). `scripts/visual_diff.py`
is a dependency-free PNG decoder/differ (stdlib only) and can compare any two
PNG files:

```bash
python scripts/visual_diff.py A.png B.png --out-dir out/ --max-mae 8
```

Goldens were captured on Windows; CI runs this step only on `windows-latest`.

## Layer 3 — old-vs-new differential

`reference/old-ui-dark.png` is a capture of the **old Tauri/React UI** at commit
`c4e8721`, rendered in a plain browser at 1200×890 / deviceScaleFactor 2 with
the same fixture data. It was produced by:

1. `git worktree add <tmp> c4e8721 --detach` and `npm ci` there.
2. Copying `old-ui-stub.ts` into the worktree's `src/` and importing it first in
   `src/main.tsx` — it fakes Tauri IPC (`invoke`, event listen, window metadata)
   so the UI renders without the Tauri shell.
3. `npm run dev -- --port 1431 --strictPort`, then in the browser: set viewport
   1200×890 @2x, `colorScheme: dark`, inject the same localStorage fixture
   (`scrcpy_theme=ultraviolet`, `scrcpy_color_mode=dark`, `scrcpy_locale=zh-CN`,
   `scrcpy_config` with the fixed device), reload and screenshot at device scale.

Reproduce the differential:

```bash
python scripts/visual_diff.py tests/visual/reference/old-ui-dark.png \
  tests/visual/golden/compare-dark.png --out-dir tests/visual/out/old-vs-new
```

The latest run: MAE **18.20**, 26.6% of pixels over threshold. Hot cells are the
engine-configuration area (start button inside vs. outside the card, `hwdec`
visibility) and the console/right rail. These are known structural differences,
not regressions; use the heatmap in `out/old-vs-new/diff.png` to drive fixes.

Note: the reference contains a real-time log timestamp, so one line will always
differ from the fixed `12:34:56` fixture timestamp.
