//! Framework-free replacement for the Tauri command layer.
//!
//! The old backend lived in `src-tauri/src/commands.rs` and talked to the UI
//! through `tauri::Window::emit`. Here the same code emits into an unbounded
//! channel; `main.rs` drains that channel on Rune's UI thread.

pub mod commands;

pub use commands::ScrcpyConfig;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rune::Waker;
use serde::Serialize;
use tokio::process::Child;
use tokio::sync::mpsc::UnboundedSender;

/// One event from the backend to the Rune UI. Strings are the old Tauri event
/// names (`scrcpy-log`, `scrcpy-status`, `scrcpy-window-pos`, ...).
#[derive(Debug)]
pub struct AppEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

impl AppEvent {
    pub fn new(name: impl Into<String>, payload: impl Serialize) -> Self {
        Self {
            name: name.into(),
            payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// Cloneable, infallible event sink handed to every spawned backend task.
#[derive(Clone)]
pub struct EventSink {
    tx: UnboundedSender<AppEvent>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl EventSink {
    pub fn new(tx: UnboundedSender<AppEvent>) -> Self {
        Self {
            tx,
            waker: Arc::new(Mutex::new(None)),
        }
    }

    /// Rune hands the app a waker in `AppSpec::setup`; call it once here so
    /// backend events are seen even when Rune has no frames scheduled.
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(|p| p.into_inner()) = Some(waker);
    }

    /// Clone the waker out of the lock before calling it: the callback is
    /// framework code that may re-enter `emit` (a nested lock would deadlock),
    /// and a panicking holder must not poison every later `emit`.
    pub fn emit(&self, name: impl Into<String>, payload: impl Serialize) {
        // The UI is gone on shutdown; dropping events is preferable to panicking
        // inside a monitor task (the old code did `.unwrap()` here).
        if self.tx.send(AppEvent::new(name, payload)).is_ok() {
            let waker = self
                .waker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            if let Some(waker) = waker {
                waker();
            }
        }
    }
}

/// Replaces Tauri's `app.manage(ScrcpyState)` with a plain shared value.
#[derive(Default)]
pub struct ScrcpyState {
    pub processes: Mutex<HashMap<String, Child>>,
    pub active_device: Mutex<Option<String>>,
    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub final_capture_hint: Mutex<HashMap<String, (i32, i32)>>,
}

pub type SharedState = Arc<ScrcpyState>;

/// `tauri::path::video_dir()` equivalent, without Tauri.
pub fn video_dir() -> Option<PathBuf> {
    let home = rune::dirs::home()?;
    #[cfg(target_os = "macos")]
    let dir = home.join("Movies");
    #[cfg(not(target_os = "macos"))]
    let dir = home.join("Videos");
    Some(dir)
}

/// `tauri::path::download_dir()` equivalent, without Tauri.
pub fn downloads_dir() -> Option<PathBuf> {
    rune::dirs::home().map(|h| h.join("Downloads"))
}
