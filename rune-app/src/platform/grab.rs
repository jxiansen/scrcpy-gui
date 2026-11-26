//! Windows-only: drag the borderless scrcpy mirror window with the mouse.
//!
//! The Tauri plugin/app-handle plumbing has been removed; the Win32 hook only
//! needs the shared process registry to know which PIDs are ours. The shortcut
//! itself is registered in `main.rs` with Rune's `App::global_shortcut`.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    HC_ACTION, HHOOK, IsWindow, IsZoomed, MB_ICONWARNING, MB_OK, MSG, MSLLHOOKSTRUCT, MessageBoxW,
    PM_REMOVE, PeekMessageW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetCursorPos, SetWindowPos,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN,
};
use windows::core::PCWSTR;

use crate::backend::ScrcpyState;

static GRAB_ACTIVE: AtomicBool = AtomicBool::new(false);
/// The window armed for the current drag, kept as a raw address so the
/// low-level hook (a plain `extern "system"` function) can read it. 0 = none.
static GRAB_HWND: AtomicIsize = AtomicIsize::new(0);

/// Flip the grab on/off. Called from the Ctrl+Alt+Shift+W Rune global shortcut.
pub fn toggle() {
    GRAB_ACTIVE.fetch_xor(true, Ordering::SeqCst);
}

/// Start the follow thread. Call once from the app's setup.
pub fn register(state: Arc<ScrcpyState>) {
    std::thread::spawn(move || follow_loop(state));
}

fn follow_loop(state: Arc<ScrcpyState>) {
    let mut grabbed: Option<GrabbedWindow> = None;
    let mut hook: Option<HHOOK> = None;

    loop {
        if !GRAB_ACTIVE.load(Ordering::SeqCst) {
            release_hook(&mut hook);
            grabbed = None;
            GRAB_HWND.store(0, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            continue;
        }

        if grabbed.is_none() {
            match GrabbedWindow::arm(&state) {
                Some(g) => {
                    // Publish the window before the hook can fire: a click is
                    // only swallowed when it lands on this exact window.
                    GRAB_HWND.store(g.hwnd.0 as isize, Ordering::SeqCst);
                    match install_hook() {
                        Ok(h) => {
                            hook = Some(h);
                            grabbed = Some(g);
                        }
                        Err(err) => {
                            // Keep the shortcut from getting stuck on and tell
                            // the user instead of failing silently.
                            GRAB_ACTIVE.store(false, Ordering::SeqCst);
                            GRAB_HWND.store(0, Ordering::SeqCst);
                            notify_hook_failure(&err);
                            continue;
                        }
                    }
                }
                None => {
                    GRAB_ACTIVE.store(false, Ordering::SeqCst);
                    continue;
                }
            }
        }

        pump_messages();
        if let Some(g) = &grabbed {
            if !g.follow_cursor(&state) {
                GRAB_ACTIVE.store(false, Ordering::SeqCst);
                release_hook(&mut hook);
                GRAB_HWND.store(0, Ordering::SeqCst);
                grabbed = None;
                continue;
            }
        }
        std::thread::sleep(Duration::from_millis(6));
    }
}

struct GrabbedWindow {
    hwnd: HWND,
    /// The process the window belonged to when the drag started; re-checked
    /// every tick so a recycled handle or a stopped scrcpy never gets moved.
    pid: u32,
    offset: (i32, i32),
}

impl GrabbedWindow {
    fn arm(_state: &Arc<ScrcpyState>) -> Option<GrabbedWindow> {
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
        if pid == 0 || !pid_is_ours(pid) {
            return None;
        }
        if unsafe { IsZoomed(fg) }.as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(fg, &mut rect) }.is_err() {
            return None;
        }
        let centre = ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
        let _ = unsafe { SetCursorPos(centre.0, centre.1) };
        Some(GrabbedWindow {
            hwnd: fg,
            pid,
            offset: (centre.0 - rect.left, centre.1 - rect.top),
        })
    }

    /// The handle must still be a live window of the same, still-running scrcpy
    /// child. A destroyed window or a recycled HWND ends the drag.
    fn is_still_valid(&self, _state: &Arc<ScrcpyState>) -> bool {
        if !unsafe { IsWindow(Some(self.hwnd)) }.as_bool() {
            return false;
        }
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(self.hwnd, Some(&mut pid)) };
        pid == self.pid && pid_is_ours(pid)
    }

    fn follow_cursor(&self, state: &Arc<ScrcpyState>) -> bool {
        if !self.is_still_valid(state) {
            return false;
        }
        let Some((cx, cy)) = cursor_pos() else {
            return true;
        };
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                cx - self.offset.0,
                cy - self.offset.1,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .is_ok()
    }
}

/// Whether the terminating click belongs to the window being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClickAction {
    /// The click is somewhere else: let it reach that window.
    Pass,
    /// The click lands on the dragged window: use it to end the drag.
    Consume,
}

fn click_action(inside_grabbed_window: bool) -> ClickAction {
    if inside_grabbed_window {
        ClickAction::Consume
    } else {
        ClickAction::Pass
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let raw = GRAB_HWND.load(Ordering::SeqCst);
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_LBUTTONDOWN && raw != 0 {
        // Any left click ends the drag, but only a click on the dragged window
        // is consumed. Swallowing every window's next click stole a click from
        // whatever the user actually aimed at.
        let hwnd = HWND(raw as *mut c_void);
        GRAB_ACTIVE.store(false, Ordering::SeqCst);
        let inside = point_in_window(hwnd, lparam);
        // One terminating click only: a second click before the follow thread
        // releases the hook must not be swallowed too.
        GRAB_HWND.store(0, Ordering::SeqCst);
        if click_action(inside) == ClickAction::Consume {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Screen point of a low-level mouse event against the window's current rect.
fn point_in_window(hwnd: HWND, lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return false;
    }
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    point_in_rect(&rect, info.pt.x, info.pt.y)
}

fn point_in_rect(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn install_hook() -> Result<HHOOK, String> {
    let hmod = unsafe { GetModuleHandleW(None) }
        .map_err(|err| format!("GetModuleHandleW failed: {err}"))?;
    unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(HINSTANCE(hmod.0)), 0) }
        .map_err(|err| format!("SetWindowsHookExW(WH_MOUSE_LL) failed: {err}"))
}

/// Surface a hook install failure where the user can see it. The grab has
/// already been switched off, so this runs at most once per attempt.
fn notify_hook_failure(error: &str) {
    eprintln!("[GRAB] Window drag unavailable: {error}");
    let message = format!(
        "Window drag is unavailable, so the mirror window cannot be moved with the mouse.\n\n{error}"
    );
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "ScrcpyGUI"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
}

fn release_hook(hook: &mut Option<HHOOK>) {
    if let Some(h) = hook.take() {
        let _ = unsafe { UnhookWindowsHookEx(h) };
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {}
}

fn pid_is_ours(pid: u32) -> bool {
    crate::backend::commands::is_our_pid(pid)
}

fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_ok() {
        Some((p.x, p.y))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clicks_inside_the_dragged_window_are_consumed() {
        assert_eq!(click_action(true), ClickAction::Consume);
        assert_eq!(click_action(false), ClickAction::Pass);
    }

    #[test]
    fn window_rect_uses_half_open_bounds() {
        let rect = RECT {
            left: 10,
            top: 20,
            right: 110,
            bottom: 220,
        };
        assert!(point_in_rect(&rect, 10, 20));
        assert!(point_in_rect(&rect, 109, 219));
        assert!(!point_in_rect(&rect, 110, 100));
        assert!(!point_in_rect(&rect, 100, 220));
        assert!(!point_in_rect(&rect, 9, 100));
    }
}
