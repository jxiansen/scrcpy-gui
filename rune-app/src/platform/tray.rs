//! System tray: reopen the window, refresh devices, stop every session or quit.
//!
//! Windows and Linux (with a tray host) get an icon; macOS returns `None`
//! (`TrayError::Unsupported`). Windows keeps the process alive with
//! `App::keep_running` only while a tray exists, so closing the window leaves the
//! app in the notification area and `Show` rebuilds the window.

use rune::{AppHandle, Tray, TrayError, TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::ui::Ui;

const SHOW: &str = "show";
const REFRESH: &str = "refresh";
const STOP_ALL: &str = "stop-all";
const QUIT: &str = "quit";

/// Starts the tray; keep the returned `Tray` alive for as long as the app runs.
pub fn start(handle: AppHandle<Ui>) -> Option<Tray> {
    let spec = TraySpec::new("com.kb.scrcpy-gui", "ScrcpyGUI", app_tray_icon())
        .tooltip("ScrcpyGUI")
        .menu(vec![
            TrayItem::action(SHOW, "Show ScrcpyGUI"),
            TrayItem::action(REFRESH, "Refresh devices"),
            TrayItem::action(STOP_ALL, "Stop all sessions"),
            TrayItem::Separator,
            TrayItem::action(QUIT, "Quit"),
        ]);

    let on_event = handle.clone();
    match Tray::new(spec, move |event| match event {
        TrayEvent::Menu(id) => match id.as_str() {
            SHOW => on_event.open(),
            REFRESH => on_event.update(|s: &mut Ui, _| s.refresh_devices()),
            STOP_ALL => on_event.update(|s: &mut Ui, _| s.stop_all()),
            QUIT => on_event.quit(),
            _ => {}
        },
        TrayEvent::Activate { .. } => on_event.open(),
        _ => {}
    }) {
        Ok(tray) => Some(tray),
        Err(TrayError::Unsupported) => None,
        Err(err) => {
            eprintln!("[SYSTEM] System tray unavailable: {err}");
            None
        }
    }
}

/// The same OpenCode-style mirroring glyph rendered into the window/taskbar
/// icon, so the tray matches the dock and app icons exactly.
fn app_tray_icon() -> TrayIcon {
    TrayIcon::rgba(32, 32, include_bytes!("tray-icon.rgba").to_vec())
}
