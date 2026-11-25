//! ScrcpyGUI, rebuilt on Rune's native UI framework.
//!
//! The React/Tauri frontend and the tauri command layer are gone. Rune owns the
//! window and event loop; `backend::commands` keeps the scrcpy/adb logic and
//! emits events into a channel that `Ui::poll` drains on the UI thread.

// The command layer was ported wholesale; some helpers are kept for the next
// wiring pass (friendly names, update modal, report export).
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::items_after_test_module)]
#![allow(clippy::too_many_arguments)]

mod backend;
mod i18n;
mod icons;
mod platform;
mod theme;
mod ui;

use rune::{AppSpec, Tx, Waker};

use crate::ui::Ui;
use crate::ui::state::SessionKind;

fn scene_dark(s: &mut Ui) {
    s.apply_fixture(false, SessionKind::Mirror);
}

fn scene_light(s: &mut Ui) {
    s.apply_fixture(true, SessionKind::Mirror);
}

fn scene_camera(s: &mut Ui) {
    s.apply_fixture(false, SessionKind::Camera);
}

fn scene_desktop(s: &mut Ui) {
    s.apply_fixture(false, SessionKind::Desktop);
}

fn scene_bloodmoon(s: &mut Ui) {
    s.apply_fixture_theme(false, SessionKind::Mirror, "bloodmoon");
}

fn scene_emerald(s: &mut Ui) {
    s.apply_fixture_theme(false, SessionKind::Mirror, "emerald");
}

fn scene_astro(s: &mut Ui) {
    s.apply_fixture_theme(false, SessionKind::Mirror, "astro");
}

fn scene_carbon(s: &mut Ui) {
    s.apply_fixture_theme(false, SessionKind::Mirror, "carbon");
}

fn scene_en(s: &mut Ui) {
    s.apply_fixture(false, SessionKind::Mirror);
    s.set_locale("en");
}

fn setup(ui: &mut Ui, waker: Waker) {
    ui.setup(waker);
}

fn poll(ui: &mut Ui, _dt: f64, _tx: &mut Tx) -> bool {
    ui.poll()
}

fn main() {
    let spec = AppSpec::<Ui> {
        min_size: Some((1080.0, 760.0)),
        setup: Some(setup),
        poll: Some(poll),
        scenes: &[
            ("compare-dark", scene_dark),
            ("compare-light", scene_light),
            ("compare-camera", scene_camera),
            ("compare-desktop", scene_desktop),
            ("compare-bloodmoon", scene_bloodmoon),
            ("compare-emerald", scene_emerald),
            ("compare-astro", scene_astro),
            ("compare-carbon", scene_carbon),
            ("compare-en", scene_en),
        ],
        ..AppSpec::new("ScrcpyGUI", (1200.0, 890.0), Ui::default, ui::view)
    };

    let mut app =
        rune::App::new(spec).global_shortcut("ctrl+alt+shift+c", platform::shortcuts::recenter);

    #[cfg(target_os = "windows")]
    {
        app = app.global_shortcut("ctrl+alt+shift+w", platform::shortcuts::toggle_drag);
    }

    app.run();
}
