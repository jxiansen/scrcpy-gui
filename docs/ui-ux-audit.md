# UI / UX audit — 2026-11-25

Audit of the Rune native shell (branch `ui-kit-refactor`, Rune dev HEAD
`ecbe830b`). Findings are ordered by severity; the last column is how each fix
is verified. A shared fix contract lives outside the repo; the work was split
across parallel agents by file ownership (UI shell + icons, state + main,
locale catalogs, and docs/CI configuration), with integration and baseline
regeneration done by the main agent.

## Method

- Real launch: `target/debug/scrcpy-gui.exe`, window captured with `PrintWindow`.
- Headless: six `--scene` fixtures at audit time (`compare-ar` was added by the
  resolution, making seven) rendered with `--snap` + `--tree`, plus six
  `--script --at mark:state` interaction states (drawer, advanced, console,
  connect, empty-connect, Escape x2).
- Structure: element roles, focus order, duplicate keys, `inert`, the
  accessibility tree and offscreen rectangles from `--tree`.
- Code: `ui/kit.rs`, `ui/state.rs`, `main.rs`, `icons.rs`, i18n catalogs.

## Findings

| # | Severity | Problem | Fix owner | Verification |
| --- | --- | --- | --- | --- |
| 1 | P0 | Connect dialog's USB / Wi-Fi segmented control is inert: `transport` is only assigned, never read, and the form does not depend on it | A + B | `--script` switching segment changes the form element set |
| 2 | P0 | Escape closes no modal layer (drawer, connect, about, alert, …) | B | `--script` with `physical: Escape` removes `settings-drawer` / `connect-submit` |
| 3 | P0 | Mirror mode shows two "codec" selects (base + advanced), both keyed `codec-trigger` and bound to `config.codec` | A | `--tree` after expanding advanced has exactly one `codec-trigger` |
| 4 | P0 | "Disconnect" stops a running session and clears the device with no confirmation, while the less destructive "End ADB" has one | A + B | `--script` click shows a confirm dialog and does not stop the session |
| 5 | P1 | `console-toggle` is the only focusable button with no label / text / help / `expanded`; `LiveRegion` exists only when the console is expanded | A | `--tree` has no unlabeled button; collapsed preview is live |
| 6 | P1 | "Changes apply next session" hint is always shown, contradicts instantly-applied settings, and long locales are truncated without a tooltip | A + C | hint absent when no session runs; all locales readable |
| 7 | P1 | `icons::GEAR` draws a radiating star (looks like brightness/theme), not a gear | A | screenshot review + golden update |
| 8 | P1 | Shortcuts drawer lists only in-session `Alt+…` keys; the OS-level `Ctrl+Alt+Shift+C` / `+W` shortcuts are undocumented | A + C | drawer text contains both groups |
| 9 | P1 | Connecting has no cancel / timeout: `connecting` only clears on `refresh-done` | B + A | unreachable address shows cancel + timeout toast |
| 10 | P1 | Toasts are single-instance; a newer one replaces the older | B + A | `toasts: Vec<_>` renders up to three |
| 11 | P2 | Main content is top-aligned in a tall window: roughly 250–300 px of dead space above the action bar, and an empty rail middle | A | content vertically centred; baselines regenerated |
| 12 | P2 | Two identical "Connect" entry points; the empty rail row uses "Refresh" as its subtitle and is also clickable | A + C | distinct copy, no duplicate refresh action |
| 13 | P2 | The settings card subtitle stays "Display, video and input controls" in Camera / Desktop modes | A + C | subtitle follows the selected mode |
| 14 | P2 | `start-stop` has a fixed `min_w(148)`; long locale labels (fr, id) can be clipped | A + C | button width follows content; extra locale scenes |
| 15 | P2 | Copy leftovers: "2. Pair device" has no "1.", "Flash / Push Files" implies flashing, five catalogs have an empty hardware-decoding tooltip, English "Start Mission" clashes with the terse Chinese | C | `check_i18n.py` passes with no empty translations |
| 16 | P2 | Real launch logs 46 `ICU4X … No segmentation model for complex script: Chinese/Japanese` lines; upstream `rune-text` excludes Windows from `parley/complex-scripts` | main | upstream note + Chinese wrapping smoke test |
| 17 | P2 | Arabic catalog exists but there is no RTL handling nor an `ar` fixture scene; `advanced_count` is hard-coded | B + A + main | new `compare-ar` scene in the structural + golden gates |

## What already worked

Modal focus trapping (192 / 193 background elements become `inert`, all
focusable elements stay non-inert), no `--tree` warnings or offscreen elements
in any of the six scenes audited, correct disabled state and label swap on the
empty state's primary button, labelled selects / switches / sliders, tooltips on
the status text, a close button and scrim click-to-close on the drawer, and a
confirmation dialog for "End ADB".

## Acceptance gate

The single entry point is `python scripts/check.py`: formatting, clippy,
`--locked` tests and build, repo consistency invariants, i18n catalogs, all
seven structural fixture scenes, and the PNG goldens. Layout / copy changes
require regenerating `BASE_RECTS` in `scripts/visual_tree_check.py` and
re-blessing `tests/visual/golden/*.png` from the new `--tree` / `--snap`
output; the two must land in the same commit.

## Resolution — 2026-11-26

All 17 findings were addressed. The work was split across parallel agents
(UI shell + icons, state + main, eight locale catalogs, docs/CI configuration)
against a shared interface contract; the main agent integrated, regenerated the
structural baseline and re-blessed the goldens.

Notable implementation details:

- `AppSpec.on_key` handles `Key::Escape` through `Ui::close_top_layer()`, which
  closes the topmost layer (`open_select` → toasts → dialogs → drawer →
  console → advanced).
- Every dropdown goes through `kit.rs::select_control`: a light anchored panel
  below its trigger, no scrim and no focus steal, dismissed by an outside
  click, the trigger or Escape. It is deliberately not a modal `Dialog` (and
  not rune-kit's `Select`, whose `anchored_fit` clips the list).
- `Ui::toasts` is a bounded queue (three entries, `seq`-keyed removal), and
  `connect()` now has a 15 s timeout plus a cancellable token.
- The connect dialog branches on `transport`: USB shows setup guidance and a
  refresh action, Wi-Fi keeps the IP / mDNS / pairing form.
- The duplicated `codec` select is gone; `advanced_count` follows the tab.
- `rune-app/Cargo.toml` depends on `parley` with `complex-scripts` so Cargo's
  feature unification supplies the CJK segmentation data on Windows too. Real
  launch stderr went from 46 `ICU4X … No segmentation model` lines to zero.
- A seventh fixture, `compare-ar`, exercises `.dir(Direction::Rtl)`; it is
  checked for offscreen elements, raw i18n keys, unlabeled buttons and a
  non-empty tree rather than for zh/en copy.

### Acceptance results

| Check | Result |
| --- | --- |
| `python scripts/check.py` (full gate) | PASS |
| `cargo fmt --all -- --check` / `clippy --all-targets --locked -D warnings` | PASS |
| `cargo test --all-targets --locked` | PASS (commands + UI unit tests) |
| `cargo build --all-targets --locked` | PASS |
| `check_i18n.py` | 8 catalogs, all keys present, 0 empty translations |
| Structural fixtures | 7 scenes, 522+ assertions |
| PNG goldens | 7 scenes, MAE = 0.000 |
| Real launch stderr | 0 ICU4X warnings (was 46) |

### Known residual items

- `rune-kit`'s `SegmentedControl` hard-codes child keys as `segment{i}`, so the
  connect dialog and the main mode switch share `segment0` / `segment1`. Real
  pointer input is unaffected; scripted checks address the dialog by coordinates.
- "Disconnect" asks for confirmation only when a session is running; when idle
  it clears the selection directly (a smaller, recoverable action).
- `compare-ar` intentionally validates safety invariants only, not RTL copy,
  and the debug build still prints Rune's first-frame timing hint.

## Follow-up reliability pass — 2026-11-26

A second review widened the scope from visual findings to runtime and
repository hygiene. The same unreleased branch now also carries:

- **Generation-scoped sessions.** `commands::next_generation` issues a
  per-device counter; a spawn is accepted only while its generation is still
  current, before and after the process starts, so a stale start can neither
  overwrite a newer process nor become an orphan. `stop_scrcpy` bumps the
  generation, sets the device's shutdown token, kills the whole tree
  (`taskkill /F /T` on Windows) and verifies the exit. `shutdown_all()` runs
  from `on_quit` and blocks late inserts. Start failures surface as an alert
  instead of being swallowed, and the UI queries `commands::is_running`.
- **One download-progress event.** Progress is now `scrcpy-status` with
  `{type:"download-progress",percent}`, throttled to at most one event per 1 %
  or 100 ms; the legacy top-level `download-progress` name is still accepted.
  Downloads use a unique temporary directory, check the HTTP status and the
  SHA-256, and only then replace the previous install, so a failure leaves the
  working binary in place.
- **Device refresh keeps the last good list.** The custom binary folder is
  passed to both `adb devices` and `getprop`, mDNS serials are matched by
  address rather than by an exact `ip:port` string, a transient failure no
  longer clears the list, and a 5 s poll keeps devices and mDNS entries fresh.
- **Preferences apply at the right time.** Sliders write to disk only on
  release, the selected session tab is persisted as `config.session_mode` and
  restored on start, and confirming a choice closes its dropdown.
- **Three icon surfaces share one glyph.** The tray
  (`platform/tray-icon.rgba`), the Windows executable resource
  (`icons/app.ico` via `build.rs`) and the README (`icon.svg` / `icon.png`) are
  renderings of the same phone → screen arrow as `icons::BRAND`.
- **Tests cover the contracts.** Unit tests beside `commands.rs`, `state.rs`,
  `kit.rs` and `i18n.rs` assert camera-facing and argument mapping, ADB/mDNS
  parsing, window-position maths, download filename sanitising, generation and
  shutdown helpers, event-to-state mapping, the layer stack and the i18n
  fallback. The visual gates stay at seven scenes and 522+ assertions.

