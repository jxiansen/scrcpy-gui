# AGENTS.md — ScrcpyGUI (Rune edition)

This repository is a native Rust desktop app. The former Tauri v2 + React 19
frontend was removed; the GUI now runs on [Rune](https://github.com/anomalyco/rune)
(`rune` + `rune-kit`), a wgpu/winit-based UI framework. There is no webview, no
Node toolchain, and no mocked backend.

## Commands

- `cargo run -p scrcpy-gui` — run the real app (opens a native window).
- `cargo run -q -p scrcpy-gui -- --snap out.png --tree tree.json` — Rune headless
  render; use it to check layout without a display.
- `cargo test -p scrcpy-gui` — Rust unit tests (scrcpy argument building, ADB
  parsing, download helpers).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` —
  CI checks.
- `cargo build --release -p scrcpy-gui` — release binary.
- `python scripts/visual_tree_check.py compare-dark` — structural visual gate:
  renders a deterministic fixture scene and asserts key rects, colours and type
  from Rune's `--tree`. Scenes: `compare-dark`, `compare-light`,
  `compare-camera`, `compare-desktop`.

Rust 1.99.1 is pinned in `rust-toolchain.toml` (Rune requires it). Do not bump
the `windows` crate independently of Rune's patched dependency set.

## Architecture

- `Cargo.toml` (workspace root): Rune dependency pinned to a `dev` revision and
  the eight `[patch.crates-io]` forks Rune requires. Consumers must repeat those
  patches at the workspace root; Cargo does not inherit them from Rune.
- `rune-app/src/main.rs`: `AppSpec` with `setup`/`poll`, plus the two OS shortcuts
  registered through `rune::App::global_shortcut`.
- `rune-app/src/backend/commands.rs`: the old `src-tauri/src/commands.rs` with the
  Tauri attribute/parameter plumbing removed. Binary lookup, ADB commands,
  `build_scrcpy_args`, downloads, process supervision and Win32 window capture
  are unchanged. All ~35 unit tests live here.
- `rune-app/src/backend/mod.rs`: `ScrcpyState` (was `app.manage(...)`), `EventSink`
  (was `Window::emit`) and app-directory helpers.
- `rune-app/src/ui/state.rs`: `Ui` state + async bridge. Async commands run on a
  tokio runtime; results and backend events go through an unbounded channel,
  `EventSink` wakes Rune, and `Ui::poll` applies changes on the UI thread.
- `rune-app/src/ui/views.rs`: the Rune view tree (Header / Sidebar / ControlPanel
  / SessionBehavior / LogPanel / Footer / onboarding overlay) built with
  `rune-kit` components.
- `rune-app/src/platform/`: global shortcut handlers and the Windows-only
  borderless-window drag hook (previously `grab.rs` + Tauri shortcuts plugin).
- `rune-app/src/i18n.rs` + `catalogs/`: all eight locales exported verbatim from
  the old TS bundles, with English fallback and `{var}` interpolation.

Settings live in `rune::prefs::Prefs` (`com.kb.scrcpy-gui`), not localStorage.
Settings keys: `config`, `dark`, `theme`, `locale`, `history`.

## Event contract

Backend events keep their old names so the ported logic stays recognisable:
`scrcpy-log` (String), `scrcpy-status` (`{device,running}` or
`{type,message,percent}`), `scrcpy-window-pos`, plus local bridge events
(`device-list`, `mdns-list`, `scrcpy-check`, `adb-killed`, `update-available`).
`Ui::handle_event` is the only place that turns them into state.

## Platform notes

- `grab.rs` is `#[cfg(target_os = "windows")]`; Linux/macOS use xdotool/osascript
  inside `commands.rs` for window position/recenter.
- Binary lookup order: custom folder → `scrcpy-bin/` beside the exe → `scrcpy-bin/`
  in the working directory → `PATH`. `adb` and `scrcpy-server` sit beside `scrcpy`.
- `download_scrcpy` still verifies `SHA256SUMS.txt` before extracting. Keep it.

## Ported behaviour

- Friendly device names (`adb shell getprop`, emulator AVD rule, display priority).
- Wireless history: 10 newest-first `ip:port` entries, 25 s reconnect loop.
- Update check (once per path), ThemedModal-style update dialog, report export.
- 9-pass / 2 s device+mDNS settle polling with a shared busy guard.
- Full 8-locale catalogs, five theme palettes, dark/light tokens, noise + glow,
  window-wide file drag-and-drop, terminal timestamps and the 220 px console.

## Remaining differences

- The pairing modal's 2 s polling while waiting for an mDNS pairing broadcast
  is not reproduced; the inline pair form works but has no waiting dialog.
- The crash `ErrorBoundary` screen is gone (Rust panics are not caught in-view).
- Noise is a generated pixel layer rather than the original tiled SVG; light
  mode does not reproduce the old `.glass` CSS cascade asymmetry exactly.
- A real device/scrcpy session has not been exercised end-to-end here.
