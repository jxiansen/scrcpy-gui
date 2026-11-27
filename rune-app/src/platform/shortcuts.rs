//! OS-level shortcuts. Rune owns the hotkey manager, so the old Tauri plugin
//! and its `Shortcut` types are gone.

use rune::Tx;

use crate::backend::commands;
use crate::ui::Ui;

/// Ctrl+Alt+Shift+C: re-centre the active mirror window.
pub fn recenter(s: &mut Ui, _tx: &mut Tx) {
    let device = s
        .state
        .active_device
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(device) = device {
        let _ = commands::recenter_scrcpy_window(s.state.clone(), device);
    }
}

/// Ctrl+Alt+Shift+W: grab the focused borderless scrcpy window (Windows only).
#[cfg(target_os = "windows")]
pub fn toggle_drag(_s: &mut Ui, _tx: &mut Tx) {
    crate::platform::grab::toggle();
}
