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
    let spec = TraySpec::new("com.kb.scrcpy-gui", "ScrcpyGUI", accent_dot(32))
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

/// The OpenCode theme accent as a round, antialiased RGBA dot (straight alpha).
fn accent_dot(size: u32) -> TrayIcon {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 * 0.42;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let alpha = (radius - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            if alpha > 0.0 {
                let at = ((y * size + x) * 4) as usize;
                rgba[at] = 0x3b;
                rgba[at + 1] = 0x5c;
                rgba[at + 2] = 0xf6;
                rgba[at + 3] = (alpha * 255.0).round() as u8;
            }
        }
    }
    TrayIcon::rgba(size, size, rgba)
}
