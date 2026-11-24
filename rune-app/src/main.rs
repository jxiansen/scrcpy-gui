//! ScrcpyGUI, rebuilt on Rune's native UI framework.
//!
//! The React/Tauri frontend and the tauri command layer are gone. Rune owns the
//! window and event loop; `backend::commands` keeps the scrcpy/adb logic and
//! emits events into a channel that `Ui::poll` drains on the UI thread.

mod backend;
mod i18n;
mod platform;
mod ui;

use rune::{AppSpec, Tx, Waker};

use crate::ui::Ui;

fn setup(ui: &mut Ui, waker: Waker) {
    ui.setup(waker);
}

fn poll(ui: &mut Ui, _dt: f64, _tx: &mut Tx) -> bool {
    ui.poll()
}

fn main() {
    let spec = AppSpec::<Ui> {
        setup: Some(setup),
        poll: Some(poll),
        ..AppSpec::new("ScrcpyGUI", (1200.0, 890.0), Ui::default, ui::view)
    };

    let mut app = rune::App::new(spec)
        .global_shortcut("ctrl+alt+shift+c", platform::shortcuts::recenter);

    #[cfg(target_os = "windows")]
    {
        app = app.global_shortcut("ctrl+alt+shift+w", platform::shortcuts::toggle_drag);
    }

    app.run();
}
