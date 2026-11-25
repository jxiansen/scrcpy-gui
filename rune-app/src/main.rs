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

use rune::prefs::Prefs;
use rune::{AppSpec, Tx, Waker};

use crate::ui::Ui;
use crate::ui::state::{SavedWindow, SessionKind};

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

/// Kill every scrcpy child synchronously so closing the GUI leaves no orphans.
/// `start_kill` only signals; it never waits (unlike `stop_scrcpy`'s 500 ms per
/// device), so a Windows logoff or tray Quit stays fast.
fn on_quit(ui: &mut Ui, _tx: &mut Tx) {
    ui.save_window_placement();
    let mut processes = ui.state.processes.lock().unwrap();
    for (_, mut child) in processes.drain() {
        let _ = child.start_kill();
    }
}

/// Save where the window is before it closes (also the tray's close-to-tray path).
fn on_close(ui: &mut Ui, _tx: &mut Tx) {
    ui.save_window_placement();
}

fn main() {
    // Headless tooling (`--scene`, `--snap`, `--tree`, `--script`, ...) must be
    // able to run while the GUI is open. Only a real launch claims the single
    // instance and creates a tray icon.
    let headless = rune::is_headless();
    if !headless && !rune::single_instance("com.kb.scrcpy-gui") {
        return;
    }

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

    // Hidden until `on_ready`, after the first frame is on the GPU, so the window
    // never flashes an empty surface. A saved physical place is applied by the
    // window host itself; `apply_window_placement` repeats it after a tray reopen.
    let prefs = Prefs::open("com.kb.scrcpy-gui");
    let saved_window: Option<SavedWindow> = prefs.get("window");
    let mut window_options = rune::WindowOptions {
        visible: false,
        ..Default::default()
    };
    // Headless snapshots always use AppSpec's 1200x890 viewport; a resized GUI
    // window must not shrink the deterministic scenes. A saved size below the
    // app's minimum is stale (e.g. from a previous test) and is ignored.
    if !headless {
        if let Some(saved) = saved_window {
            let scale = if saved.scale > 0.0 { saved.scale } else { 1.0 };
            window_options.position = Some((saved.x, saved.y));
            if saved.w as f32 / scale >= 1080.0 && saved.h as f32 / scale >= 760.0 {
                window_options.physical_size = Some((saved.w, saved.h));
            }
        }
    }

    let mut app = rune::App::new(spec)
        .window_options(window_options)
        .global_shortcut("ctrl+alt+shift+c", platform::shortcuts::recenter);

    #[cfg(target_os = "windows")]
    {
        app = app.global_shortcut("ctrl+alt+shift+w", platform::shortcuts::toggle_drag);
    }

    let handle = app.handle();
    // Windows stays in the tray when the window closes; other platforms quit as
    // before. Headless runs must not create a real tray icon.
    let tray = if headless {
        None
    } else {
        platform::tray::start(handle.clone())
    };
    if cfg!(target_os = "windows") && tray.is_some() {
        app = app.keep_running();
    }

    app = app
        .on_ready({
            let handle = handle.clone();
            move |window: &rune::WindowHandle| {
                platform::window::set(window);
                window.show();
                handle.update(|s: &mut Ui, _| s.apply_window_state());
            }
        })
        .on_quit(on_quit)
        .on_close(on_close);

    app.run();
    drop(tray);
    if !headless {
        rune::release_instance();
    }
}
