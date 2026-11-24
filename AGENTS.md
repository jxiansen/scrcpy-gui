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
- `rune-app/src/i18n.rs`: compact zh-CN/en catalogs. Other locales fall back to
  English; the eight full TS catalogs were not ported.

Settings live in `rune::prefs::Prefs` (`com.kb.scrcpy-gui`), not localStorage.
Settings keys: `config`, `dark`, `locale`.

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

## Known gaps versus the old app

- Only zh-CN and en are translated; the other six locales need catalogs.
- Device friendly-name lookup, wireless history reconnect, update-check modal,
  report export and the precise 2 s mDNS settle loop are not ported yet.
- Default `rune-kit` theme replaces the five custom CSS themes; accent swatches
  currently change the accent only.
