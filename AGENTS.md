# AGENTS.md

ScrcpyGUI is a Tauri v2 desktop GUI (React 19 + TypeScript + Vite) that drives external `scrcpy` and `adb` binaries. There is no mocked backend; frontend-only runs cannot exercise real behavior.

## Commands

- Use **npm**: `package-lock.json` is committed and CI uses `npm ci`. Don't switch to bun/pnpm.
- Node 24 in CI; `package.json` engines require `>=22.12` (Vite 8). `npm ci` is the clean install and avoids lockfile churn.
- `npm run tauri dev` — run the real app. It starts Vite itself on fixed port 1430 (`strictPort`; 1420 is excluded by Windows/WinNAT on some machines); don't pre-start Vite.
- `npm run dev` — frontend only. Every device/binary call is a Tauri `invoke`, so devices, scrcpy detection and settings loading fail. Not a valid app smoke test.
- `npm run lint` — only `tsc --noEmit`. There is no ESLint or Prettier config in this repo.
- `npm test` — Vitest + jsdom; no device or scrcpy needed.
  - one file: `npm test -- src/components/__tests__/Sidebar.test.tsx`
  - one test: `npm test -- -t "renders device list correctly"`
- `npm run build` — `tsc && vite build`; `npm run tauri build` adds the desktop bundle.
- Linux bundle: `npm run build:linux` (AppImage env workarounds). CI build check: `npm run tauri build -- --no-bundle`.
- Rust, from `src-tauri/`: `cargo clippy --all-targets` and `cargo test`; PR CI runs both on Linux and Windows.
  - Rust tests live only in `commands.rs`. When adding a `ScrcpyConfig` field, update the `base_config` fixture too or `cargo test` stops compiling.

## Architecture

- `src/hooks/useScrcpy.ts` is the frontend state/backend hub: `ScrcpyConfig` type and defaults, all `invoke` calls, Tauri event subscriptions, localStorage persistence, device polling.
- `src-tauri/src/commands.rs` holds almost all backend logic: binary resolution, ADB, `build_scrcpy_args`, process supervision, downloads, window capture. It is large and behavior-dense; change it in small patches.
- `ScrcpyConfig` is duplicated: camelCase TS interface in `useScrcpy.ts` and Rust struct in `commands.rs` (`#[serde(rename_all = "camelCase")]`). Add fields to both; serde silently ignores unknown JS fields.
- New Tauri commands must be registered in the `src-tauri/src/lib.rs` `invoke_handler`. New plugin permissions or windows go in `src-tauri/capabilities/default.json` (current labels: `main`, `splashscreen`).
- scrcpy launch arguments are generated only in `build_scrcpy_args`, split by `sessionMode` (`mirror` / `camera` / `desktop`). Change flags there and cover them with Rust unit tests.
- Renderer/option lists are discovered at runtime from `scrcpy --help` and OS-filtered; don't hardcode them in React. `--hwdec` is exposed only when the installed binary advertises it (`supportsHwdec`), and values live in `HWDEC_VALUES` on the Rust side.
- Binary lookup order: custom folder → `scrcpy-bin/` beside the executable → `scrcpy-bin/` in the working directory → `PATH`. `adb` and `scrcpy-server` must sit beside `scrcpy`.
- `download_scrcpy` picks the platform archive (including `winarm64`), verifies the release's `SHA256SUMS.txt`, and fails closed before extraction; keep that check when changing download logic.
- Rust → frontend events: `scrcpy-log`, `scrcpy-status`, `scrcpy-window-pos`, `download-progress`. Reuse these rather than adding parallel lifecycle channels.
- Settings live in webview localStorage, not a Rust store: `scrcpy_config`, `scrcpy_theme`, `scrcpy_color_mode`, `scrcpy_window_positions`, `scrcpy_history`, `scrcpy_locale`, `scrcpy_onboarding_done`.
- The main window starts hidden and is revealed by `close_splashscreen` from `App.tsx`; preserve that startup flow when changing windows.

## UI conventions

- User-visible copy goes through `t()` from `src/i18n`. `locales/en.ts` is the typed source of truth; all 8 locale files are typed as `Translations = typeof en`, so a new key must be added to every locale or `npm run lint` fails. Empty/missing values fall back to English at runtime.
- Tests render components without `I18nProvider`; `t()` falls back to English, so tests assert English text.
- Tailwind v4 via PostCSS is config-free: theme tokens are in `src/index.css` under `@theme` and CSS variables. Themes use `data-theme` + `data-mode` on `<html>`; prefer `bg-primary` / `text-on-primary` / `var(--...)` and check both light and dark (light mode remaps many zinc/black utilities in `index.css`).

## Platform and release notes

- Windows-specific code (`grab.rs`, Win32 window capture) is `#[cfg(target_os = "windows")]`; Linux uses `xdotool`, macOS `osascript`. Keep other targets compiling.
- The `windows` crate is pinned to 0.62 to match Tauri 2.12's tao/wry; don't bump it independently.
- Global shortcuts are OS-level and share one plugin instance in `shortcuts.rs` (Ctrl+Alt+Shift+C recenter; Ctrl+Alt+Shift+W drag on Windows). Register new shortcuts there.
- Version is duplicated in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`; bump all three. `bump_version.yml` does this, and merging its `chore/version-bump-v*` PR creates the `v*` tag that triggers the draft release build.
- `flake.nix` still pins version 4.0.0 and its own hashes; the version-bump workflow does not update it, and refreshing it requires Nix.
- Security baseline: Tauri CSP is enabled (prod + dev), fonts/noise are self-hosted, `withGlobalTauri` is off, and the unused `tauri-plugin-shell` is not installed. Don't reintroduce external asset URLs.
- `scrcpy-gui-v3`, `scrcpy_gui_v3_lib`, and `com.kb.scrcpy-gui-v3` are legacy, intentional names; don't rename them casually.
