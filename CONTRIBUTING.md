# Contributing / engineering guide

This project is a single-crate Rust workspace (`rune-app`) with a small Python
toolchain for visual regression. There is no Node.js build step. This document
is the human counterpart to `AGENTS.md` (which describes the architecture).

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | `1.99.1` | Pinned in `rust-toolchain.toml`; `clippy` + `rustfmt` are installed with it. |
| Python | `3.11+` | Only the standard library is used (no `pip install` needed). |
| C++ build tools | — | Windows: Visual Studio C++ build tools. Linux: `build-essential pkg-config libssl-dev libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev libudev-dev libasound2-dev`. |

Rust is pinned because Rune requires it. Do not bump the `windows` crate
independently of Rune's patched dependency set.

## Daily loop

```bash
cargo run -p scrcpy-gui                 # launch the native window
python scripts/check.py                 # the full gate, exactly what CI runs
python scripts/check.py --fast          # Rust + consistency checks only
python scripts/check.py --no-golden     # skip PNG goldens (Linux CI / quick local)
python scripts/check.py --visual-only   # only the two visual layers
cargo build --release -p scrcpy-gui     # release binary
```

`scripts/check.py` is the **single source of truth** for the gate. Always add a
new check there rather than only to a workflow, so local and CI cannot drift.
It runs, in order:

1. **Consistency checks** (static, no build): the toolchain pinned in
   `rust-toolchain.toml` matches every workflow's `toolchain:`; all eleven Rune
   `rev = "..."` pins in `Cargo.toml` are identical; `Cargo.toml` and
   `Cargo.lock` carry the same package version.
2. `cargo fmt --all -- --check`
3. `cargo clippy --all-targets --locked -- -D warnings`
4. `cargo test --all-targets --locked`
5. `cargo build --all-targets --locked`
6. `scripts/check_i18n.py` — the eight locale catalogs
7. `scripts/visual_tree_check.py` — all six structural fixture scenes
8. `scripts/visual_image_check.py --max-mae 8` — PNG goldens (unless
   `--no-golden`)

`--locked` is used everywhere on purpose: a stale `Cargo.lock` is a CI failure,
not a silent auto-fix.

## CI

`pull_request.yml` runs the same `scripts/check.py` on `ubuntu-22.04` (with
`--no-golden`) and `windows-latest` (full gate). PNG goldens are captured on
Windows because font rasterisation differs per platform; the structural
`--tree` checks are platform-independent and therefore run on both legs. PRs
target `main`; pushes to an open PR cancel its previous run.

## Visual baselines

See `tests/visual/README.md` for the three verification layers. Rules:

- Structural assertions (`--tree`) are the hard gate. Run
  `python scripts/visual_tree_check.py` for all six scenes, or pass a single
  scene name while iterating. `python scripts/visual_tree_check.py --list`
  lists them.
- Golden PNGs are updated **only** with `python scripts/visual_image_check.py
  --bless` on Windows, after a deliberate visual change. Review the PNG diff
  before committing, and say in the commit message why the baseline changed.
  `--scene compare-dark` limits a run to one scene.
- `tests/visual/out/` is generated and ignored; never commit it.

Fixtures are deterministic: they freeze appearance, locale, device/mDNS lists,
session mode, logs and the terminal timestamp, and skip all ADB/Prefs work. If
you change a fixture, re-bless the goldens in the same commit.

## Internationalisation

Catalogs live in `rune-app/src/i18n/catalogs/`; `en.json` is the source of
truth. When adding a key, add it to **all eight** catalogs (English fallback is
a runtime safety net, not an excuse for missing keys). Then run:

```bash
python scripts/check_i18n.py
```

It validates the key set, value types and `{var}` placeholders. Empty
translations are reported as warnings; the runtime falls back to English, but
please fill them in when you touch a catalog.

`scripts/visual_tree_check.py` additionally asserts that no raw i18n key (e.g.
`device.title`) or `?` placeholder leaks into the UI in any scene.

## Upgrading Rune

`Cargo.toml` pins three Rune git dependencies and eight `[patch.crates-io]`
forks to **one** revision. Cargo does not inherit `[patch]` from a dependency,
so all eleven entries must move together. After editing:

```bash
cargo update -p rune            # refresh Cargo.lock
python scripts/check.py         # verifies the revs are still identical
```

Never bump only some of them; `scripts/check.py` will fail.

## Versioning and releases

Versions live in `rune-app/Cargo.toml` and `Cargo.lock`:

```bash
python scripts/bump_version.py patch        # or minor / major
python scripts/bump_version.py minor --dry-run
```

The script updates both files atomically and verifies the write. To release:

1. Run the **Bump Version** workflow (`workflow_dispatch`, choose
   patch/minor/major). It opens a `chore/version-bump-vX.Y.Z` PR containing
   both `Cargo.toml` and `Cargo.lock`.
2. Review and merge that PR into `main`. `tag_on_merge.yml` then creates and
   pushes `vX.Y.Z`. It refuses to overwrite an existing tag.
3. `release.yml` builds Linux, Windows and macOS binaries, uploads them as
   artifacts, and a single `publish` job creates one **draft** GitHub release.
4. A human reviews the draft and publishes it.

`tag_on_merge.yml` never force-pushes tags; a release tag is immutable history.

## Repository hygiene

- `.gitignore` covers `target/`, Python caches, headless render artefacts,
  scrcpy binaries/downloads, secrets and IDE files. Never commit binaries.
- `.gitattributes` marks PNG/ICO/EXE/ZIP assets as binary so they are never
  line-ending converted.
- `.editorconfig` fixes encoding, indentation and LF for newly created source
  and config files. The eight locale catalogs are historically CRLF — do not
  renormalise them as a side effect of an unrelated edit.
- `.vscode/extensions.json` recommends the Rust/TOML/Python/EditorConfig
  extensions; the old Tauri recommendation was removed with the frontend.
- `dependabot.yml` watches the workspace-root Cargo manifest and GitHub
  Actions. Rune's git pins are upgraded manually (see above).
