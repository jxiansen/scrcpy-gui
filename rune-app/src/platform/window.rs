//! The app's main window handle, reachable from tray items, the pin button and
//! the taskbar sync without threading it through the view tree.
//!
//! The handle is refreshed by `on_ready` on every native creation (first launch
//! and every reopen after a close with `keep_running`), so requests made while
//! the window is gone are queued by Rune and applied when it comes back.

use std::cell::RefCell;

use rune::{Taskbar, WindowHandle};

thread_local! {
    static MAIN: RefCell<Option<WindowHandle>> = const { RefCell::new(None) };
}

/// Remembers the current native window handle (`App::on_ready`).
pub fn set(handle: &WindowHandle) {
    MAIN.with(|slot| *slot.borrow_mut() = Some(handle.clone()));
}

/// Runs `f` with the handle, if a window exists or was queued for one.
fn with<R>(f: impl FnOnce(&WindowHandle) -> R) -> Option<R> {
    MAIN.with(|slot| slot.borrow().as_ref().map(f))
}

pub fn set_always_on_top(on: bool) {
    let _ = with(|handle| handle.set_always_on_top(on));
}

pub fn set_taskbar(state: Taskbar) {
    let _ = with(|handle| handle.set_taskbar(state));
}

/// The window's current place on the desktop, for saving across launches.
pub fn placement() -> Option<rune::WindowPlacement> {
    with(|handle| handle.placement()).flatten()
}

pub fn set_position(position: (i32, i32)) {
    let _ = with(|handle| handle.set_position(position));
}

/// `size` is logical points; `resize_instant` avoids an animated jump after a reopen.
pub fn resize_instant(size: (f32, f32)) {
    let _ = with(|handle| handle.resize_instant(size));
}
