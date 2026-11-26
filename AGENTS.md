# AGENTS.md — ScrcpyGUI (Rune edition)

This repository is a native Rust desktop app. The former Tauri v2 + React 19
frontend was removed; the GUI now runs on [Rune](https://github.com/anomalyco/rune)
(`rune` + `rune-kit`), a wgpu/winit-based UI framework. There is no webview, no
Node toolchain, and no mocked backend.

## Commands

- `python scripts/check.py` — the single full gate: fmt, clippy, `--locked`
  tests/build, repo consistency checks, i18n catalogs, all seven structural
  fixture scenes and PNG goldens. CI runs this on Windows and
  `check.py --no-golden` on Linux. `--fast` skips visual layers,
  `--visual-only` skips Rust checks, `--scene NAME` limits the structural run.
- `cargo run -p scrcpy-gui` — run the real app (opens a native window).
- `cargo run -q -p scrcpy-gui -- --snap out.png --tree tree.json` — Rune headless
  render; use it to check layout without a display.
- `cargo test -p scrcpy-gui` — Rust unit tests; they live beside the code
  (scrcpy arguments, ADB parsing, downloads, session lifecycle, event and
  layer mapping).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` —
  CI checks (also covered by `check.py`).
- `cargo build --release -p scrcpy-gui` — release binary.
- `python scripts/visual_tree_check.py [scene]` — structural visual gate;
  defaults to all seven scenes (`compare-dark`, `compare-light`,
  `compare-camera`, `compare-desktop`, `compare-en`, `compare-empty`,
  `compare-ar`), `--list` lists them. Asserts the flat shell, device head,
  settings card, action bar, console and the settings/advanced/console drawer
  states from Rune's `--tree`.
- `python scripts/visual_image_check.py` — golden PNG regression (use `--bless`
  on Windows to update goldens); `python scripts/visual_diff.py A.png B.png` —
  generic PNG diff. See `tests/visual/README.md` for the old-vs-new procedure.
- `python scripts/check_i18n.py` — locale catalog consistency against `en.json`.
- `python scripts/bump_version.py patch|minor|major` — bump the version in both
  `Cargo.toml` and `Cargo.lock`; `--dry-run` previews it.

Rust 1.99.1 is pinned in `rust-toolchain.toml` (Rune requires it). Do not bump
the `windows` crate independently of Rune's patched dependency set. Engineering
conventions, CI layout and release steps are in `CONTRIBUTING.md`; add new
checks to `scripts/check.py`, never to a workflow alone.

## Architecture

- `Cargo.toml` (workspace root): Rune dependency pinned to a `dev` revision and
  the eight `[patch.crates-io]` forks Rune requires. Consumers must repeat those
  patches at the workspace root; Cargo does not inherit them from Rune.
- `rune-app/src/main.rs`: `AppSpec` with `setup`/`poll`, plus the two OS shortcuts
  registered through `rune::App::global_shortcut`; `on_quit` calls
  `commands::shutdown_all()`.
- `rune-app/src/backend/commands.rs`: the old `src-tauri/src/commands.rs` with the
  Tauri attribute/parameter plumbing removed. Binary lookup, ADB commands,
  `build_scrcpy_args`, downloads, generation-scoped process supervision and
  Win32 window capture live here.
- `rune-app/src/backend/mod.rs`: `ScrcpyState` (was `app.manage(...)`), `EventSink`
  (was `Window::emit`) and app-directory helpers.
- `rune-app/src/ui/state.rs`: `Ui` state + async bridge. Async commands run on a
  tokio runtime; results and backend events go through an unbounded channel,
  `EventSink` wakes Rune, and `Ui::poll` applies changes on the UI thread.
- `rune-app/src/ui/kit.rs`: the flat shell (custom titlebar row, fixed `Sidebar`
  rail, scrollable main, right settings drawer), the official OpenCode theme and
  the rune-kit fields/buttons. Every dropdown uses `select_control`: a light
  anchored panel under its trigger with outside-click and Escape dismissal, no
  scrim and no focus steal (not a modal `Dialog`). Custom pieces are the action
  bar, console, file-drop target and scrcpy icons.
- `rune-app/src/ui/state.rs` holds the selected device, appearance
  (`color_mode`), `panel_on_top`, `connect_open`/`about_open`,
  `settings_open`/`advanced_open`/`console_open`, the open dropdown
  (`open_select`), and the async bridge.
- `rune-app/src/platform/`: global shortcut handlers and the Windows-only
  borderless-window drag hook (previously `grab.rs` + Tauri shortcuts plugin).
- `rune-app/src/i18n.rs` + `catalogs/`: all eight locales exported verbatim from
  the old TS bundles, with English fallback and `{var}` interpolation. Missing
  keys resolve through a fallback path that never leaks a new `String` per
  lookup.

Settings live in `rune::prefs::Prefs` (`com.kb.scrcpy-gui`), not localStorage.
Settings keys: `config` (including `session_mode`), `color_mode` (legacy `dark`
still readable), `panel_on_top`, `window`, `locale`, `history`.

## Event contract

Backend events keep their old names so the ported logic stays recognisable:

- `scrcpy-log` — one `String` per console line.
- `scrcpy-status` — either `{device,running}` or a status object
  `{type,message,percent}`. Download progress is
  `{type:"download-progress",percent}`, throttled to at most one event per 1 %
  or 100 ms; the legacy top-level `download-progress` name is still accepted.
- `scrcpy-window-pos` — the sampled mirror-window position.
- Local bridge events: `device-list`, `mdns-list`, `scrcpy-check`, `adb-killed`,
  `update-available`, `app-toast` (`{title,description}`) and `toast-expire`
  (`{seq}`).

`Ui::handle_event` is the only place that turns them into state.

## Session lifecycle

Sessions are keyed by device and guarded by a generation counter:

- `commands::next_generation` increments a per-device counter. A start is
  accepted only if its generation is still current before and after the spawn;
  a stale start exits without touching the process map. Start failures are
  returned as `Err` and surfaced by `Ui` as an alert, never swallowed.
- `commands::stop_scrcpy` increments the generation, sets the device's shutdown
  token, runs `taskkill /F /T` (absolute System32 path on Windows), waits for
  the process to exit and verifies the result. The mirror window's last
  position is sampled before its process entry is removed.
- `commands::shutdown_all()` is called from `on_quit`; it sets the global
  shutdown flag, drains and kills every child, and prevents an in-flight spawn
  from inserting a new process afterwards.
- `commands::is_running(device)` lets the UI query the authoritative process
  state; `Ui::start_session` asks for a fresh generation before launching, and
  `Ui::stop_session` awaits `stop_scrcpy`.

## Tests

Rust tests sit beside the code they cover: `backend/commands.rs` (scrcpy
argument and camera-facing building, ADB/mDNS parsing, window-position maths,
download filename sanitising, session generation and stop helpers),
`ui/state.rs` (event mapping, running table, layer closing), `ui/kit.rs`
(camera-facing option mapping) and `i18n.rs` (missing-key fallback).
`cargo test --all-targets --locked` runs them all; `scripts/check.py` then runs
the visual gates: seven fixture scenes, 522+ structural assertions and the PNG
goldens. Add a unit test with every behavioural change; a visual change also
needs its re-blessed golden in the same commit.

## Platform notes

- `grab.rs` is `#[cfg(target_os = "windows")]`; Linux/macOS use xdotool/osascript
  inside `commands.rs` for window position/recenter.
- `single_instance` is skipped for headless runs (`--scene/--snap/--tree/--script`)
  so visual checks can run while the GUI is open. Tray creation is skipped there too.
- Windows starts a tray (`platform/tray.rs`) and uses `keep_running` when it exists:
  closing the window leaves the process in the notification area; the tray Quit
  calls `on_quit`, which runs `commands::shutdown_all()`: it kills every scrcpy
  child synchronously and blocks new spawns. macOS has no tray in this Rune rev;
  Linux gets one only where a tray host runs.
- The Settings > Appearance pin sets the GUI window's `panel_on_top` via
  `WindowHandle::set_always_on_top`; it is unrelated to scrcpy's
  `config.always_on_top` (`--always-on-top`). Taskbar Busy/badge state is pushed
  from `Ui::poll` through `WindowHandle::set_taskbar`.
- The GUI window's place is saved under the `window` Prefs key on `on_close` /
  `on_quit` and restored in `on_ready`; sizes are physical, `resize_instant`
  takes the logical size back. `LiveRegion::Polite` is on the newest log line.
- Binary lookup order: custom folder → `scrcpy-bin/` beside the exe → `scrcpy-bin/`
  in the working directory → `PATH`. `adb` and `scrcpy-server` sit beside `scrcpy`.
- One brand glyph feeds three surfaces: the tray
  (`platform/tray-icon.rgba`), the Windows executable resource
  (`rune-app/icons/app.ico` via `build.rs`) and the README (`icon.svg` /
  `icon.png`). `icons::BRAND` carries the same phone → screen arrow in-app;
  change the source SVG and regenerate all three together.
- `download_scrcpy` still verifies `SHA256SUMS.txt` before extracting. Keep it.

## Ported behaviour

- Friendly device names (`adb shell getprop`, emulator AVD rule, display priority).
- Wireless history: 10 newest-first `ip:port` entries, 25 s reconnect loop.
- Update check (once per path), ThemedModal-style update dialog, report export.
- 9-pass / 2 s device+mDNS settle polling with a shared busy guard, plus a 5 s
  background poll that keeps both lists current and does not clear them on a
  transient failure.
- Full 8-locale catalogs, the official OpenCode light/dark/system appearance,
  window-wide file drag-and-drop, terminal timestamps and the console card.

## Remaining differences

- The pairing dialog has no 2 s mDNS waiting state; the inline pair form works.
- The crash `ErrorBoundary` screen is gone (Rust panics are not caught in-view).
- The old noise/glass/glow layers and five palettes are gone with the official
  theme; the drop target has no hover highlight (the window-wide drop works).
- A real device/scrcpy session has not been exercised end-to-end here.
