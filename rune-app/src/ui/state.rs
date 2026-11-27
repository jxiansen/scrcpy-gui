//! Application state and the bridge between Rune's UI thread and the async backend.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rune::prefs::Prefs;
use rune::{Taskbar, TaskbarProgress, TextEdit, Waker};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::backend::{AppEvent, EventSink, ScrcpyConfig, SharedState, commands};
use crate::i18n::{t, tv};

const PREFS_ID: &str = "com.kb.scrcpy-gui";

/// Locks shared with backend tasks must survive a poisoned holder: a panicking
/// task should degrade, not take the UI thread (or the poll loops) with it.
trait LockAlive<T> {
    fn lock_alive(&self) -> MutexGuard<'_, T>;
}

impl<T> LockAlive<T> for Mutex<T> {
    fn lock_alive(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub model: String,
    #[serde(default)]
    pub friendly: Option<String>,
}

impl Device {
    pub fn label(&self) -> &str {
        if let Some(name) = &self.friendly {
            return name;
        }
        // Sidebar.formatDeviceLabel: strip the adb mDNS wrapper for display only.
        // Keep the " (2)" conflict suffix: two devices resolving to the same
        // mDNS instance name must stay distinguishable.
        let s = self.serial.as_str();
        if let Some(rest) = s.strip_prefix("adb-") {
            if let Some(pos) = rest.find("._adb-tls-connect._") {
                return &rest[..pos];
            }
        }
        &self.model
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    Mirror,
    Camera,
    Desktop,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mirror => "mirror",
            Self::Camera => "camera",
            Self::Desktop => "desktop",
        }
    }

    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub fn label(self) -> &'static str {
        match self {
            Self::Mirror => "Mirror",
            Self::Camera => "Camera",
            Self::Desktop => "Desktop",
        }
    }

    /// Reads the persisted `config.session_mode`; unknown values fall back to
    /// mirror, the same default the old frontend used.
    pub fn from_mode(mode: &str) -> Self {
        match mode.trim().to_ascii_lowercase().as_str() {
            "camera" => Self::Camera,
            "desktop" => Self::Desktop,
            _ => Self::Mirror,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Usb,
    Wifi,
}

#[derive(Clone, Debug, Default)]
pub struct Download {
    pub active: bool,
    pub percent: u32,
    pub message: String,
    pub success: bool,
}

/// One `rune_kit::Toast`'s copy; `Ui::toasts` holds at most three live notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastNote {
    /// Monotonic id; `toast-expire` names the note it removes.
    pub seq: u64,
    pub title: String,
    pub description: String,
}

/// The GUI window's saved place, in physical desktop pixels (the space
/// `WindowOptions::position` / `physical_size` take). `scale` converts the
/// content size back to logical points for `WindowHandle::resize`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct SavedWindow {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    /// Whether the window was maximized when saved; `x/y/w/h` then hold the
    /// *normal* (restore) frame. Older saves lack the field and read as false.
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MdnsService {
    pub name: String,
    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub service: String,
    pub address: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateInfo {
    pub local: String,
    pub latest: String,
}

#[derive(Deserialize)]
struct ScrcpyCheck {
    found: bool,
    message: String,
    /// The folder this check queried; present on new payloads so a stale
    /// result can be dropped. Ignored here (compared before deserialising).
    #[serde(default)]
    #[allow(dead_code)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct WindowPos {
    /// The session that sampled the position. Several devices can mirror at
    /// once; only the selected device's position may become the remembered
    /// launch position. Optional so an older payload still parses.
    #[serde(default)]
    device: Option<String>,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct IpPayload {
    ip: String,
}

pub struct Ui {
    pub runtime: tokio::runtime::Runtime,
    pub state: SharedState,
    pub sink: Arc<EventSink>,
    rx: UnboundedReceiver<AppEvent>,
    pub prefs: Prefs,
    pub config: ScrcpyConfig,
    pub devices: Vec<Device>,
    pub mdns: Vec<MdnsService>,
    pub logs: Vec<(String, String)>,
    pub running: BTreeMap<String, bool>,
    pub history: Vec<String>,
    pub tab: SessionKind,
    pub transport: Transport,
    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub adb_menu: bool,
    /// The kill-adb confirmation dialog flag; read by the end-adb dialog and
    /// the disconnect flow.
    pub confirm_kill_adb: bool,
    /// The "disconnect this device?" confirmation dialog.
    pub disconnect_confirm: bool,
    /// The Connect / pair dialog.
    pub connect_open: bool,
    /// The About dialog.
    pub about_open: bool,
    /// The right-side settings drawer.
    pub settings_open: bool,
    /// The settings card's advanced section.
    pub advanced_open: bool,
    /// The bottom console, expanded.
    pub console_open: bool,
    pub ip: TextEdit,
    pub pair_ip: TextEdit,
    pub pair_code: TextEdit,
    pub command: TextEdit,
    pub background_color: TextEdit,
    pub open_select: Option<String>,
    pub alert: Option<String>,
    pub update: Option<UpdateInfo>,
    pub onboarding_open: bool,
    pub toasts: Vec<ToastNote>,
    toast_seq: u64,
    /// Bumped by `connect` and `cancel_connect`; a stale ADB task checks it so a
    /// cancelled or superseded attempt cannot report success or reset the flags.
    connect_token: Arc<AtomicU64>,
    pub is_refreshing: bool,
    pub connecting: bool,
    pub pairing: bool,
    /// Whether the file-drop overlay is currently highlighted.
    pub drop_hover: bool,
    pub remember_position: bool,
    busy: Arc<AtomicBool>,
    /// Guards the 5 s single-pass fetch so a slow ADB call cannot stack a
    /// second poll on top of it.
    polling: Arc<AtomicBool>,
    /// The pending refresh is coalesced: requests arriving while a settle loop
    /// runs (connect success, path change, tray) set this instead of being
    /// dropped, and the loop runs one more pass after finishing. The path is
    /// re-read from the shared config snapshot so a path change mid-refresh
    /// is picked up by the extra pass.
    refresh_pending: Arc<AtomicBool>,
    refresh_path: Arc<Mutex<Option<String>>>,
    friendly_names: Arc<Mutex<HashMap<String, String>>>,
    last_device_sig: Arc<Mutex<String>>,
    pub dark: bool,
    /// `light`, `dark` or `system`; `dark` keeps the last resolved value for code
    /// that reads the legacy pref.
    pub color_mode: String,
    /// The GUI window's always-on-top state (not scrcpy's `config.always_on_top`).
    pub panel_on_top: bool,
    /// The saved GUI window place; `None` on first launch.
    window_placement: Option<SavedWindow>,
    last_taskbar: Option<Taskbar>,
    pub locale: String,
    pub scrcpy_ok: Option<bool>,
    pub scrcpy_message: String,
    update_checked: bool,
    update_path: Option<String>,
    pub download: Download,
    /// Whether a prefs write failed at least once this run; failures are
    /// reported once so the alert is not spammed by every save.
    prefs_failed: bool,
    /// The custom `scrcpy_path` a download replaced. If the download fails, the
    /// old install is still the working one and the path is put back.
    download_prev_path: Option<Option<String>>,
    pub waker: Option<Waker>,
    /// Set by `--scene compare-*`; freezes all asynchronous data for deterministic rendering.
    pub fixture: bool,
}

fn default_config() -> ScrcpyConfig {
    ScrcpyConfig {
        device: String::new(),
        session_mode: "mirror".into(),
        audio_enabled: Some(true),
        vsync: Some(true),
        ..ScrcpyConfig::default()
    }
}

impl Default for Ui {
    fn default() -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = Arc::new(EventSink::new(tx));
        let state: SharedState = Arc::new(crate::backend::ScrcpyState::default());
        let prefs = Prefs::open(PREFS_ID);
        let config: ScrcpyConfig = prefs.get("config").unwrap_or_else(default_config);
        let tab = SessionKind::from_mode(&config.session_mode);
        let bg_color = config.background_color.clone().unwrap_or_default();
        let dark = prefs.get("dark").unwrap_or(true);
        let color_mode: String = prefs
            .get("color_mode")
            .unwrap_or_else(|| if dark { "dark".into() } else { "light".into() });
        let panel_on_top: bool = prefs.get("panel_on_top").unwrap_or(false);
        let window_placement: Option<SavedWindow> = prefs.get("window");
        let locale: String = prefs.get("locale").unwrap_or_else(|| "zh-CN".into());
        let history: Vec<String> = prefs.get("history").unwrap_or_default();
        let remember_position: bool = prefs.get("remember_position").unwrap_or(true);
        Self {
            runtime,
            state,
            sink,
            rx,
            prefs,
            config,
            devices: Vec::new(),
            mdns: Vec::new(),
            logs: Vec::new(),
            running: BTreeMap::new(),
            history,
            tab,
            transport: Transport::Usb,
            adb_menu: false,
            confirm_kill_adb: false,
            disconnect_confirm: false,
            connect_open: false,
            about_open: false,
            settings_open: false,
            advanced_open: false,
            console_open: false,
            ip: TextEdit::new(""),
            pair_ip: TextEdit::new(""),
            pair_code: TextEdit::new(""),
            command: TextEdit::new(""),
            background_color: TextEdit::new(bg_color),
            open_select: None,
            alert: None,
            update: None,
            onboarding_open: true,
            toasts: Vec::new(),
            toast_seq: 0,
            connect_token: Arc::new(AtomicU64::new(0)),
            is_refreshing: false,
            connecting: false,
            pairing: false,
            drop_hover: false,
            remember_position,
            busy: Arc::new(AtomicBool::new(false)),
            polling: Arc::new(AtomicBool::new(false)),
            refresh_pending: Arc::new(AtomicBool::new(false)),
            refresh_path: Arc::new(Mutex::new(None)),
            friendly_names: Arc::new(Mutex::new(HashMap::new())),
            last_device_sig: Arc::new(Mutex::new(String::new())),
            dark,
            color_mode,
            panel_on_top,
            window_placement,
            last_taskbar: None,
            locale,
            scrcpy_ok: None,
            scrcpy_message: String::new(),
            update_checked: false,
            update_path: None,
            download: Download::default(),
            download_prev_path: None,
            waker: None,
            fixture: false,
            prefs_failed: false,
        }
    }
}

/// Resets a busy flag even when the task panics or is cancelled: without this
/// one wedged fetch would disable refreshing for the rest of the run.
struct FlagGuard(Arc<AtomicBool>);

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Ui {
    /// Fixed data for `cargo run -- --scene compare-*`; no ADB, no clock, no prefs.
    pub fn apply_fixture(&mut self, light: bool, tab: SessionKind) {
        self.fixture = true;
        self.dark = !light;
        self.color_mode = if light { "light" } else { "dark" }.into();
        self.locale = "zh-CN".into();
        self.tab = tab;
        self.transport = Transport::Usb;
        self.config = default_config();
        self.panel_on_top = false;
        self.remember_position = true;
        self.settings_open = false;
        self.advanced_open = false;
        self.console_open = false;
        self.connect_open = false;
        self.about_open = false;
        self.confirm_kill_adb = false;
        self.disconnect_confirm = false;
        self.open_select = None;
        self.alert = None;
        self.update = None;
        self.toasts.clear();
        self.toast_seq = 0;
        self.download = Download::default();
        self.download_prev_path = None;
        self.connect_token.store(0, Ordering::SeqCst);
        self.busy.store(false, Ordering::SeqCst);
        self.polling.store(false, Ordering::SeqCst);
        self.connecting = false;
        self.is_refreshing = false;
        self.config.device = "706fc5a1".into();
        self.config.session_mode = tab.as_str().into();
        self.config.bitrate = Some(8);
        self.scrcpy_ok = Some(true);
        self.scrcpy_message = "SCRCPY READY".into();
        self.onboarding_open = false;
        self.devices = vec![Device {
            serial: "706fc5a1".into(),
            model: "PJA110".into(),
            friendly: Some("PJA110".into()),
        }];
        self.mdns = vec![MdnsService {
            name: "adb-616f16f3c-YnOIRI".into(),
            service: "_adb-tls-connect._tcp".into(),
            address: "172.16.1.145:38517".into(),
        }];
        self.logs = vec![
            ("12:34:56".into(), "[SYSTEM] ScrcpyGUI ready".into()),
            ("12:34:56".into(), "[SYSTEM] Device: PJA110".into()),
        ];
        self.history = vec!["192.168.1.42:5555".into(), "10.0.0.8:5555".into()];
        self.ip.replace_text("192.168.x.x:5555");
        self.pair_ip.replace_text("192.168.x.x:55");
        self.background_color.replace_text("");
    }

    pub fn setup(&mut self, waker: Waker) {
        self.sink.set_waker(waker.clone());
        self.waker = Some(waker);
        if self.fixture {
            return;
        }
        self.log("[SYSTEM] ScrcpyGUI ready".into());
        #[cfg(target_os = "windows")]
        crate::platform::grab::register(self.state.clone());
        self.check_scrcpy();
        self.refresh_devices();
        self.reconnect_history();
        self.start_history_reconnect();
        self.start_device_poll();
    }

    pub fn poll(&mut self) -> bool {
        let mut events = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        let changed = !events.is_empty();
        // Download progress is high-frequency. The channel is unbounded, so a
        // slow UI frame can leave a backlog; only the newest reading matters.
        let last_progress = events.iter().rposition(is_download_progress);
        for (index, ev) in events.into_iter().enumerate() {
            if is_download_progress(&ev) && Some(index) != last_progress {
                continue;
            }
            self.handle_event(ev);
        }
        self.sync_taskbar();
        changed
    }

    fn handle_event(&mut self, ev: AppEvent) {
        match ev.name.as_str() {
            "scrcpy-log" => {
                if let Ok(line) = serde_json::from_value::<String>(ev.payload.clone()) {
                    self.log(line);
                }
            }
            "scrcpy-status" => self.handle_status(&ev.payload),
            "download-progress" => self.handle_download_progress(&ev.payload),
            "scrcpy-check" => {
                // Drop a stale check: it names the folder it queried, and the
                // user may have switched folders while it ran.
                let queried = ev.payload.get("path").and_then(Value::as_str);
                let stale = ev.payload.get("path").is_some()
                    && queried != self.config.scrcpy_path.as_deref();
                if !stale && let Ok(v) = serde_json::from_value::<ScrcpyCheck>(ev.payload) {
                    self.scrcpy_ok = Some(v.found);
                    self.scrcpy_message = v.message;
                    if v.found {
                        self.onboarding_open = false;
                        let path = self.config.scrcpy_path.clone();
                        if (!self.update_checked || self.update_path != path)
                            && !self.download.active
                        {
                            self.update_checked = true;
                            self.update_path = path;
                            self.check_update();
                        }
                    } else {
                        self.update_checked = false;
                    }
                }
            }
            "scrcpy-window-pos" => {
                if self.remember_position
                    && let Ok(pos) = serde_json::from_value::<WindowPos>(ev.payload)
                    && pos
                        .device
                        .as_deref()
                        .is_none_or(|device| device == self.config.device)
                {
                    self.config.window_x = Some(pos.x);
                    self.config.window_y = Some(pos.y);
                    self.save_config();
                }
            }
            "device-list" => {
                if let Ok(mut devices) = serde_json::from_value::<Vec<Device>>(ev.payload) {
                    // Keep names learned by earlier passes even if this payload
                    // only carries adb's model fields.
                    {
                        let mut names = self.friendly_names.lock_alive();
                        for device in &mut devices {
                            match device.friendly.clone() {
                                Some(name) => {
                                    names.insert(device.serial.clone(), name);
                                }
                                None => {
                                    let name = names.get(&device.serial).cloned();
                                    device.friendly = name;
                                }
                            }
                        }
                    }
                    self.devices = devices;
                }
            }
            "adb-killed" => self.refresh_devices(),
            "refresh-request" => self.refresh_devices(),
            "device-poll" => {
                // The 5 s background poll is a single silent pass; the settle
                // loop and connect already own the busy flags.
                if !self.is_refreshing && !self.busy.load(Ordering::SeqCst) {
                    self.refresh_mdns();
                }
            }
            "refresh-done" => self.is_refreshing = false,
            "connect-done" => self.connecting = false,
            "pair-done" => self.pairing = false,
            "history-add" => {
                if let Ok(v) = serde_json::from_value::<IpPayload>(ev.payload) {
                    let ip = v.ip.clone();
                    let title = t(&self.locale, "header.scrcpyReady").to_string();
                    let detail = tv(&self.locale, "logs.connectedSuccessfully", &[("ip", &ip)]);
                    self.show_toast(title, detail);
                    self.add_history(v.ip);
                    // A wireless connect succeeded: refresh both lists now
                    // instead of waiting up to 5 s for the background poll.
                    self.refresh_devices();
                }
            }
            "app-toast" => {
                let title = ev
                    .payload
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !title.is_empty() {
                    let description = ev
                        .payload
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    self.show_toast(title.to_string(), description);
                }
            }
            "app-alert" => {
                if let Some(message) = ev.payload.get("message").and_then(Value::as_str) {
                    self.alert = Some(message.to_string());
                }
                // A failed launch left no live session for this device.
                if let Some(device) = ev.payload.get("device").and_then(Value::as_str) {
                    if self.config.device == device {
                        self.set_active_device(None);
                    }
                }
            }
            "toast-expire" => {
                if let Some(seq) = ev.payload.get("seq").and_then(Value::as_u64) {
                    self.toasts.retain(|note| note.seq != seq);
                }
            }
            "update-available" => {
                if let Ok(v) = serde_json::from_value::<UpdateInfo>(ev.payload) {
                    self.update = Some(v);
                }
            }
            "mdns-list" => {
                if let Ok(list) = serde_json::from_value::<Vec<MdnsService>>(ev.payload) {
                    self.mdns = list;
                }
            }
            "reconnect-history" => self.reconnect_history(),
            _ => {}
        }
    }

    fn handle_status(&mut self, p: &Value) {
        if let (Some(device), Some(running)) = (
            p.get("device").and_then(Value::as_str),
            p.get("running").and_then(Value::as_bool),
        ) {
            // Start and stop run on different worker threads; a stale stop's
            // `false` can reach the channel after a newer start's `true`. The
            // backend stamps each status with its session generation and the
            // registry knows the current one: drop anything older.
            let generation = p.get("generation").and_then(Value::as_u64);
            if !commands::status_is_current(device, generation) {
                return;
            }
            if running {
                self.running.insert(device.to_string(), true);
            } else {
                self.running.remove(device);
            }
            // The OS recenter shortcut targets the selected device only while
            // its session is live.
            if self.config.device == device {
                self.set_active_device(if running {
                    Some(device.to_string())
                } else {
                    None
                });
            }
            return;
        }
        match p.get("type").and_then(Value::as_str).unwrap_or("") {
            "downloading" => {
                self.download.active = true;
                self.download.message = p
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
            }
            "download-progress" => self.handle_download_progress(p),
            "download-complete" => {
                self.download.active = false;
                self.download.success = p.get("success").and_then(Value::as_bool).unwrap_or(false);
                self.download.message = p
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
                if self.download.success {
                    self.download_prev_path = None;
                    let title = t(&self.locale, "onboarding.successBinariesActive").to_string();
                    let detail = if self.download.message.is_empty() {
                        t(&self.locale, "header.scrcpyReady").to_string()
                    } else {
                        self.download.message.clone()
                    };
                    self.show_toast(title, detail);
                    self.check_scrcpy();
                    self.refresh_devices();
                } else {
                    self.restore_download_path();
                    self.log(format!("[SYSTEM] {}", self.download.message));
                }
            }
            _ => {}
        }
    }

    fn handle_download_progress(&mut self, p: &Value) {
        self.download.active = true;
        if let Some(percent) = p.get("percent").and_then(Value::as_u64) {
            self.download.percent = percent.min(100) as u32;
        }
    }

    /// Puts the custom install path back after a failed download: the new
    /// binaries never landed, so the old folder is still the working one.
    fn restore_download_path(&mut self) {
        let Some(previous) = self.download_prev_path.take() else {
            return;
        };
        self.config.scrcpy_path = previous;
        self.save_config();
        self.check_scrcpy();
        self.refresh_devices();
    }

    /// Shows one kit toast for six seconds; at most three stay visible, the
    /// oldest dropping off the back when a fourth arrives.
    pub fn show_toast(&mut self, title: impl Into<String>, description: impl Into<String>) {
        self.toast_seq += 1;
        let seq = self.toast_seq;
        self.toasts.push(ToastNote {
            seq,
            title: title.into(),
            description: description.into(),
        });
        if self.toasts.len() > 3 {
            let extra = self.toasts.len() - 3;
            self.toasts.drain(..extra);
        }
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            sink.emit("toast-expire", serde_json::json!({ "seq": seq }));
        });
    }

    /// Drops the toast with `seq`; the close button and `toast-expire` both use it.
    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub fn dismiss_toast(&mut self, seq: u64) {
        self.toasts.retain(|note| note.seq != seq);
    }

    pub fn log(&mut self, text: String) {
        if self.fixture {
            for line in text.split('\n') {
                if !line.trim().is_empty() {
                    self.logs.push(("12:34:56".into(), line.to_string()));
                }
            }
        } else {
            let stamp = chrono::Local::now().format("%H:%M:%S").to_string();
            for line in text.split('\n') {
                if !line.trim().is_empty() {
                    self.logs.push((stamp.clone(), line.to_string()));
                }
            }
        }
        if self.logs.len() > 100 {
            self.logs.drain(..self.logs.len() - 100);
        }
    }

    pub fn save_config(&mut self) {
        if self.fixture {
            return;
        }
        if let Err(e) = self.prefs.set("config", &self.config) {
            self.note_prefs_error(e);
        }
    }

    /// Persist one key, reporting a failure once instead of silently
    /// discarding the write (the value still lives in memory this run).
    /// Takes the value by value so callers can pass `self.field.clone()`
    /// without the immutable field borrow colliding with the `&mut self`.
    fn persist<T: Serialize>(&mut self, key: &str, value: T) {
        if let Err(e) = self.prefs.set(key, &value) {
            self.note_prefs_error(e);
        }
    }

    /// A prefs write that failed (read-only disk, quota, serialization): the
    /// setting is kept in memory for this run but the user must be told,
    /// otherwise the app silently loses every change at the next restart.
    fn note_prefs_error(&mut self, error: rune::prefs::PrefsError) {
        if self.prefs_failed {
            return;
        }
        self.prefs_failed = true;
        self.log(format!("[SYSTEM] Settings could not be saved: {error}"));
        let title = t(&self.locale, "alerts.settingsSaveFailedTitle");
        let detail = t(&self.locale, "alerts.settingsSaveFailedDetail");
        self.alert = Some(format!("{title}\n\n{detail}"));
    }

    pub fn set_color_mode(&mut self, mode: &str, system_dark: bool) {
        self.color_mode = mode.to_string();
        self.dark = match mode {
            "light" => false,
            "dark" => true,
            _ => system_dark,
        };
        if self.fixture {
            return;
        }
        self.persist("color_mode", self.color_mode.clone());
        self.persist("dark", self.dark);
    }

    /// Whether the UI paints dark for the current mode; `system` asks the window.
    pub fn resolved_dark(&self, system_dark: bool) -> bool {
        match self.color_mode.as_str() {
            "light" => false,
            "dark" => true,
            _ => system_dark,
        }
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.into();
        if self.fixture {
            return;
        }
        self.persist("locale", self.locale.clone());
    }

    /// The GUI window's pin; independent of scrcpy's `config.always_on_top`.
    pub fn toggle_panel_on_top(&mut self) {
        self.panel_on_top = !self.panel_on_top;
        self.persist("panel_on_top", self.panel_on_top);
        crate::platform::window::set_always_on_top(self.panel_on_top);
    }

    /// What the taskbar button should show: busy while downloading, and the
    /// number of devices with a live scrcpy session.
    pub fn taskbar_state(&self) -> Taskbar {
        let running = self.running.values().filter(|live| **live).count() as u32;
        Taskbar::new()
            .progress(if self.download.active {
                TaskbarProgress::Busy
            } else {
                TaskbarProgress::None
            })
            .badge(running, "Scrcpy sessions")
    }

    /// Applies the taskbar state only when it changed, from `poll`.
    pub fn sync_taskbar(&mut self) {
        let state = self.taskbar_state();
        if self.last_taskbar.as_ref() != Some(&state) {
            crate::platform::window::set_taskbar(state.clone());
            self.last_taskbar = Some(state);
        }
    }

    /// Pushes the persisted window flags after the native window (re)appears.
    pub fn apply_window_state(&mut self) {
        self.apply_window_placement();
        crate::platform::window::set_always_on_top(self.panel_on_top);
        let taskbar = self.taskbar_state();
        crate::platform::window::set_taskbar(taskbar.clone());
        self.last_taskbar = Some(taskbar);
    }

    /// Puts the window back where it was last saved, in logical points.
    pub fn apply_window_placement(&self) {
        let Some(saved) = self.window_placement else {
            return;
        };
        crate::platform::window::set_position((saved.x, saved.y));
        let scale = if saved.scale > 0.0 { saved.scale } else { 1.0 };
        crate::platform::window::resize_instant((saved.w as f32 / scale, saved.h as f32 / scale));
    }

    /// Saves the window's place before it closes; `on_close` and `on_quit` both
    /// call this, so a tray Quit keeps the last on-screen position too. While
    /// minimized/maximized the live frame is not the normal frame (Windows
    /// reports -32000 for a minimized window), so the system's restore record
    /// is saved instead. The in-memory copy is updated too: a tray reopen
    /// re-applies `window_placement`, which must be this save, not the value
    /// loaded at startup.
    pub fn save_window_placement(&mut self) {
        let Some(place) = crate::platform::window::placement() else {
            return;
        };
        let ((x, y), (w, h)) = if place.minimized || place.maximized {
            place
                .restore
                .unwrap_or((place.position, place.content_size))
        } else {
            (place.position, place.content_size)
        };
        let saved = SavedWindow {
            x,
            y,
            w,
            h,
            scale: place.scale as f32,
            maximized: place.maximized,
        };
        self.window_placement = Some(saved);
        if let Err(e) = self.prefs.set("window", &saved) {
            self.note_prefs_error(e);
        }
    }

    /// Stops every device's scrcpy process, as the tray menu's "Stop all" does.
    /// The selected device is included even when its `running:true` has not
    /// arrived yet, so a start in flight is cancelled too.
    pub fn stop_all(&mut self) {
        let mut devices: Vec<String> = self
            .running
            .iter()
            .filter(|(_, live)| **live)
            .map(|(device, _)| device.clone())
            .collect();
        if !self.config.device.is_empty() && !devices.contains(&self.config.device) {
            devices.push(self.config.device.clone());
        }
        for device in devices {
            self.stop_session(&device);
        }
    }

    pub fn add_history(&mut self, ip: String) {
        if !ip.contains(':') {
            return;
        }
        self.history = [ip.clone()]
            .into_iter()
            .chain(self.history.iter().filter(|v| **v != ip).cloned())
            .take(10)
            .collect();
        self.persist("history", self.history.clone());
    }

    pub fn toggle_remember_position(&mut self) {
        self.remember_position = !self.remember_position;
        if self.fixture {
            return;
        }
        self.persist("remember_position", self.remember_position);
        if !self.remember_position {
            self.config.window_x = None;
            self.config.window_y = None;
            self.save_config();
        }
    }

    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.persist("history", self.history.clone());
    }

    pub fn check_scrcpy(&self) {
        if self.fixture {
            return;
        }
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::check_scrcpy(path.clone()).await;
            let found = v.get("found").and_then(Value::as_bool).unwrap_or(false);
            let message = v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // Carry the queried path: a slow check of an old folder must not
            // overwrite the readiness state of the folder now configured.
            sink.emit(
                "scrcpy-check",
                serde_json::json!({ "found": found, "message": message, "path": path }),
            );
        });
    }

    /// One ADB + mDNS fetch. A resolves the friendly names in the same call;
    /// they are cached here so a later payload without them keeps its labels.
    /// This is the single-pass path used by the 5 s poll and `refresh_mdns`;
    /// the settle loop drives the same `fetch_and_emit`.
    fn fetch_devices_once(&self, silent: bool) {
        if self.fixture
            || self.busy.load(Ordering::SeqCst)
            || self.polling.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        let names = self.friendly_names.clone();
        let last_sig = self.last_device_sig.clone();
        let polling = self.polling.clone();
        self.runtime.spawn(async move {
            let _guard = FlagGuard(polling);
            fetch_and_emit(&sink, path, &last_sig, &names, silent).await;
        });
    }

    /// Manual refresh: up to 8 silent passes, 2 s apart, until the device set
    /// stops changing (mirrors `refreshDevicesUntilSettled` in the old hook).
    /// The whole loop is bounded so a wedged ADB cannot disable the refresh
    /// button for minutes on end.
    pub fn refresh_devices(&mut self) {
        if self.fixture {
            return;
        }
        *self.refresh_path.lock_alive() = self.config.scrcpy_path.clone();
        if self.busy.swap(true, Ordering::SeqCst) {
            // A settle loop is already running; ask it to run one more pass
            // with the current config instead of dropping this request.
            self.refresh_pending.store(true, Ordering::SeqCst);
            return;
        }
        self.is_refreshing = true;
        let sink = self.sink.clone();
        let busy = self.busy.clone();
        let pending = self.refresh_pending.clone();
        let refresh_path = self.refresh_path.clone();
        let sig = self.last_device_sig.clone();
        let names = self.friendly_names.clone();
        let mut path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let _guard = FlagGuard(busy);
            loop {
                let settle = async {
                    let mut previous = String::new();
                    for i in 0..=8 {
                        let ok = fetch_and_emit(&sink, path.clone(), &sig, &names, true).await;
                        let current = sig.lock_alive().clone();
                        if !ok || current == previous {
                            break;
                        }
                        previous = current;
                        if i < 8 {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                    }
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(45), settle).await;
                if !pending.swap(false, Ordering::SeqCst) {
                    break;
                }
                // A queued request may carry a newer custom path.
                path = refresh_path.lock_alive().clone();
            }
            sink.emit("refresh-done", serde_json::json!({}));
        });
    }

    /// A single silent device + mDNS pass. Also the body of the 5 s poll, so
    /// both lists stay current without touching `is_refreshing` (no button
    /// flicker, no overlap with a manual settle loop).
    pub fn refresh_mdns(&self) {
        if self.fixture {
            return;
        }
        self.fetch_devices_once(true);
    }

    /// Emits a `device-poll` every 5 s; the UI thread runs a single silent
    /// fetch unless a manual refresh or connect owns the busy state.
    fn start_device_poll(&self) {
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // The first `interval` tick resolves immediately; setup already
            // refreshed once, so wait for the first real period.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                sink.emit("device-poll", serde_json::json!({}));
            }
        });
    }

    fn start_history_reconnect(&self) {
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(25)).await;
                sink.emit("reconnect-history", serde_json::json!({}));
            }
        });
    }

    fn reconnect_history(&mut self) {
        if self.fixture {
            return;
        }
        // `adb devices` reports an mDNS transport as
        // `adb-<serial>-<random>._adb-tls-connect._tcp`, never as the history's
        // `ip:port`, so exact serial equality kept re-connecting live devices
        // every 25 s. Match the address's host through the discovered mDNS
        // service instead.
        let devices = self.devices.clone();
        let mdns = self.mdns.clone();
        let pending: Vec<String> = self
            .history
            .iter()
            .filter(|ip| !history_entry_connected(ip, &devices, &mdns))
            .cloned()
            .collect();
        for ip in pending {
            let sink = self.sink.clone();
            let path = self.config.scrcpy_path.clone();
            self.runtime.spawn(async move {
                let result = commands::adb_connect(sink.clone(), ip, path, Some(true)).await;
                let ok = result
                    .as_ref()
                    .ok()
                    .and_then(|v| v.get("success").and_then(Value::as_bool))
                    .unwrap_or(false);
                if ok {
                    // A silent reconnect still has to surface the device: run
                    // the settle refresh instead of waiting for the 5 s poll.
                    sink.emit("refresh-request", serde_json::json!({}));
                }
            });
        }
    }

    pub fn connect(&mut self, ip: String) {
        self.open_select = None;
        // Enter submits even when the button is disabled; an empty address
        // would spawn a pointless `adb connect ""`.
        let ip = ip.trim().to_string();
        if ip.is_empty() {
            return;
        }
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        // A cancel or a newer connect bumps the token: a stale ADB attempt can
        // then neither report success nor clear the flags of its replacement.
        let token = self.connect_token.fetch_add(1, Ordering::SeqCst) + 1;
        let connect_token = self.connect_token.clone();
        let timeout_title = t(&self.locale, "toast.connectTimeout").to_string();
        let ip_label = ip.clone();
        // `connecting` alone drives the connect spinner/cancel; a concurrent
        // device refresh must neither show nor clear it (and vice versa).
        self.connecting = true;
        self.runtime.spawn(async move {
            let current = || connect_token.load(Ordering::SeqCst) == token;
            let attempt = async {
                let result =
                    commands::adb_connect(sink.clone(), ip.clone(), path.clone(), Some(false))
                        .await;
                if !current() {
                    return;
                }
                match result {
                    Ok(v) => {
                        let message = v.get("message").and_then(Value::as_str).unwrap_or("");
                        let ok = v.get("success").and_then(Value::as_bool).unwrap_or(false);
                        let lower = message.to_lowercase();
                        let failed =
                            lower.contains("failed to connect") || lower.contains("cannot connect");
                        if !ok && failed {
                            sink.emit(
                                "scrcpy-log",
                                "[SYSTEM] Retrying connection once...".to_string(),
                            );
                            let _ = commands::run_terminal_command(
                                None,
                                format!("adb disconnect {ip}"),
                                path.clone(),
                            )
                            .await;
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            if !current() {
                                return;
                            }
                            if let Ok(retry) = commands::adb_connect(
                                sink.clone(),
                                ip.clone(),
                                path.clone(),
                                Some(false),
                            )
                            .await
                            {
                                if !current() {
                                    return;
                                }
                                let retry_ok = retry
                                    .get("success")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                if retry_ok {
                                    sink.emit("history-add", serde_json::json!({ "ip": ip }));
                                }
                            }
                        } else if ok {
                            sink.emit("history-add", serde_json::json!({ "ip": ip }));
                        }
                        sink.emit("scrcpy-log", format!("[SYSTEM] {message}"));
                    }
                    Err(e) => sink.emit("scrcpy-log", format!("[ADB ERROR] {e}")),
                }
            };
            let limit = std::time::Duration::from_secs(15);
            let timed_out = tokio::time::timeout(limit, attempt).await.is_err();
            if timed_out {
                if current() {
                    sink.emit(
                        "app-toast",
                        serde_json::json!({ "title": timeout_title, "description": ip_label }),
                    );
                    sink.emit("connect-done", serde_json::json!({}));
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if current() {
                    sink.emit("connect-done", serde_json::json!({}));
                }
            }
        });
    }

    /// Cancels an in-flight `connect`: clears the connect flag now, shows the
    /// cancelled toast, and invalidates the ADB task so it stays silent. A
    /// concurrent device refresh keeps its own spinner.
    pub fn cancel_connect(&mut self) {
        self.connect_token.fetch_add(1, Ordering::SeqCst);
        let was_connecting = self.connecting;
        self.connecting = false;
        if was_connecting {
            let title = t(&self.locale, "toast.connectCancelled").to_string();
            let detail = self.ip.text.trim().to_string();
            self.show_toast(title, detail);
        }
    }

    pub fn pair(&mut self, ip: String, code: String) {
        self.open_select = None;
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.pairing = true;
        self.runtime.spawn(async move {
            match commands::adb_pair(sink.clone(), ip, code, path).await {
                Ok(v) => {
                    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
                    sink.emit("scrcpy-log", format!("[SYSTEM] {message}"));
                }
                Err(e) => sink.emit("scrcpy-log", format!("[ADB ERROR] {e}")),
            }
            sink.emit("refresh-request", serde_json::json!({}));
            sink.emit("pair-done", serde_json::json!({}));
        });
    }

    /// Clears the selected device, stopping its session first. The stop is
    /// unconditional (when a device is selected): a start still in flight is
    /// invisible to `session_live`, and only `stop_scrcpy`'s generation bump
    /// can cancel it before it spawns an untracked mirror.
    pub fn disconnect_device(&mut self) {
        let serial = self.config.device.clone();
        if serial.is_empty() {
            return;
        }
        self.stop_session(&serial);
        self.config.device.clear();
        self.open_select = None;
        self.save_config();
        self.set_active_device(None);
    }

    /// Asks before clearing the device while its session is live; otherwise
    /// disconnects straight away.
    pub fn request_disconnect(&mut self) {
        let serial = self.config.device.as_str();
        let live = !serial.is_empty() && self.session_live(serial);
        if live {
            self.disconnect_confirm = true;
        } else {
            self.disconnect_device();
        }
    }

    pub fn confirm_disconnect(&mut self) {
        self.disconnect_confirm = false;
        self.disconnect_device();
    }

    pub fn cancel_disconnect(&mut self) {
        self.disconnect_confirm = false;
    }

    /// Closes the topmost layer, matching the reverse of `view()`'s `layer.push`
    /// order: the open dropdown first, then toasts, modal dialogs, and finally
    /// the settings / console / advanced drawers. `false` means the key was not
    /// handled and may bubble.
    pub fn close_top_layer(&mut self) -> bool {
        if self.open_select.take().is_some() {
            return true;
        }
        if self.toasts.pop().is_some() {
            return true;
        }
        if self.alert.is_some() {
            self.alert = None;
            return true;
        }
        if self.onboarding_open {
            self.onboarding_open = false;
            return true;
        }
        if self.update.is_some() {
            self.update = None;
            return true;
        }
        if self.disconnect_confirm {
            self.disconnect_confirm = false;
            return true;
        }
        if self.confirm_kill_adb {
            self.confirm_kill_adb = false;
            return true;
        }
        if self.about_open {
            self.about_open = false;
            return true;
        }
        if self.connect_open {
            self.connect_open = false;
            return true;
        }
        if self.settings_open {
            self.settings_open = false;
            return true;
        }
        if self.console_open {
            self.console_open = false;
            return true;
        }
        if self.advanced_open {
            self.advanced_open = false;
            return true;
        }
        false
    }

    pub fn select_device(&mut self, serial: String) {
        self.config.device = serial.clone();
        self.open_select = None;
        self.save_config();
        // Selecting an idle device must not clear the active device of another
        // device that is still running (Ctrl+Alt+Shift+C would stop recentring
        // it). Only a running selection becomes authoritative here.
        if self.session_live(&serial) {
            self.set_active_device(Some(serial));
        }
    }

    /// Live sessions come from the event-driven map; the backend's generation
    /// table is the tie-breaker when an event has not reached the UI yet.
    fn session_live(&self, serial: &str) -> bool {
        self.running.get(serial).copied().unwrap_or(false) || commands::is_running(serial)
    }

    pub fn start_session(&mut self) {
        if self.config.device.is_empty() {
            self.alert = Some(t(&self.locale, "alerts.noDeviceSelectedMessage").to_string());
            return;
        }
        // A second click before `running:true` arrives must not spawn a second
        // scrcpy (the backend would supersede the first, but the churn races
        // the status events). The backend registry is authoritative here.
        if self.session_live(&self.config.device) {
            return;
        }
        if self.scrcpy_ok == Some(false) {
            let message = if self.scrcpy_message.is_empty() {
                t(&self.locale, "alerts.noDeviceSelectedMessage").to_string()
            } else {
                self.scrcpy_message.clone()
            };
            self.alert = Some(message);
            return;
        }
        self.open_select = None;
        self.config.session_mode = self.tab.as_str().into();
        self.save_config();
        let device = self.config.device.clone();
        // Ctrl+Alt+Shift+C recentres the active device, so mark it before the
        // child spawns; a failed launch clears it again through `app-alert`.
        self.set_active_device(Some(device.clone()));
        self.log(tv(
            &self.locale,
            "logs.initializingScrcpy",
            &[("device", &device)],
        ));
        let sink = self.sink.clone();
        let event_sink = (*sink).clone();
        let config = self.config.clone();
        let locale = self.locale.clone();
        let device_label = device.clone();
        let generation = commands::next_generation(&device);
        self.runtime.spawn(async move {
            if let Err(e) = commands::start_scrcpy(device, generation, config, event_sink).await {
                let message = tv(&locale, "logs.failedToStartScrcpy", &[("error", &e)]);
                sink.emit("scrcpy-log", message.clone());
                sink.emit(
                    "app-alert",
                    serde_json::json!({ "message": message, "device": device_label }),
                );
            }
        });
    }

    pub fn stop_session(&mut self, device: &str) {
        let sink = self.sink.clone();
        // Optimistic: the Stop button flips back immediately and cannot get
        // stuck if the backend's stopped event is lost. A live session's
        // authoritative `false` follows from `stop_scrcpy`.
        self.running.remove(device);
        if self.config.device == device {
            self.set_active_device(None);
        }
        let device = device.to_string();
        self.runtime.spawn(async move {
            if let Err(e) = commands::stop_scrcpy(&device).await {
                sink.emit("scrcpy-log", format!("[ERROR] {e}"));
            }
        });
    }

    pub fn download_scrcpy(&mut self) {
        if self.download.active || self.fixture {
            return;
        }
        self.download.active = true;
        self.download.percent = 0;
        self.download.success = false;
        self.open_select = None;
        // Download always extracts to the default exe-adjacent folder; a stale
        // custom path would make the freshly installed binary invisible. Keep
        // it so a failed download can put the working install back.
        self.download_prev_path = Some(self.config.scrcpy_path.clone());
        self.config.scrcpy_path = None;
        self.save_config();
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            // `commands::download_scrcpy` emits the single `download-complete`
            // failure event itself; emitting another here ran the failure
            // handling twice and could clobber a retry started in between.
            let _ = commands::download_scrcpy(sink).await;
        });
    }

    pub fn run_command(&self, cmd: String) {
        let sink = self.sink.clone();
        let device = self.config.device.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::run_terminal_command(Some(device), cmd, path).await;
            for key in ["stdout", "stderr", "message"] {
                if let Some(text) = v.get(key).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        sink.emit("scrcpy-log", format!("[ADB] {text}"));
                    }
                }
            }
        });
    }

    /// Switches the capture mode and persists it. Closing any open dropdown is
    /// part of the switch: its options belong to the previous tab.
    pub fn set_tab(&mut self, tab: SessionKind) {
        self.open_select = None;
        let changed = self.tab != tab || self.config.session_mode != tab.as_str();
        self.tab = tab;
        self.config.session_mode = tab.as_str().into();
        if changed {
            self.save_config();
        }
    }

    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
        self.open_select = None;
    }

    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub fn toggle_adb_menu(&mut self) {
        self.open_select = None;
        self.adb_menu = !self.adb_menu;
        if !self.adb_menu {
            self.confirm_kill_adb = false;
        }
    }

    #[allow(dead_code)] // legacy/unwired API, kept for follow-up
    pub fn confirm_kill_adb(&mut self) {
        self.open_select = None;
        if !self.confirm_kill_adb {
            self.confirm_kill_adb = true;
            return;
        }
        self.confirm_kill_adb = false;
        self.adb_menu = false;
        self.kill_adb();
    }

    pub fn kill_adb(&self) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let _ = commands::kill_adb(sink.clone(), path).await;
            sink.emit("adb-killed", serde_json::json!({}));
        });
    }

    pub fn handle_file(&mut self, path: std::path::PathBuf) {
        let device = self.config.device.clone();
        if device.trim().is_empty() {
            self.log(t(&self.locale, "logs.noDeviceForDragDrop").to_string());
            return;
        }
        let sink = self.sink.clone();
        let custom = self.config.scrcpy_path.clone();
        let locale = self.locale.clone();
        let text = path.to_string_lossy().to_string();
        self.runtime.spawn(async move {
            let apk = text.to_lowercase().ends_with(".apk");
            let key = if apk {
                "logs.installingApk"
            } else {
                "logs.pushingFile"
            };
            sink.emit(
                "scrcpy-log",
                tv(&locale, key, &[("device", &device), ("filePath", &text)]),
            );
            let v = if apk {
                commands::install_apk(device, text, custom).await
            } else {
                commands::push_file(device, text, custom).await
            };
            if let Some(message) = v.get("message").and_then(Value::as_str) {
                sink.emit("scrcpy-log", format!("[ADB] {message}"));
            }
        });
    }

    pub fn check_update(&self) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::check_scrcpy_update(path).await;
            if v.get("update_available")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let local = v
                    .get("local_version")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let latest = v
                    .get("latest_version")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                sink.emit(
                    "update-available",
                    serde_json::json!({ "local": local, "latest": latest }),
                );
            }
        });
    }

    /// Closes the update prompt and installs into the default folder.
    /// `download_scrcpy` records the current custom path and clears it; a
    /// failed download restores it in `handle_status`.
    pub fn update_now(&mut self) {
        self.update = None;
        self.download_scrcpy();
    }

    pub fn export_report(&self) {
        let payload = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "localStorage": {
                "scrcpy_config": serde_json::to_string(&self.config).unwrap_or_default(),
                "scrcpy_color_mode": self.color_mode,
                "scrcpy_locale": self.locale,
                "scrcpy_history": serde_json::to_string(&self.history).unwrap_or_default(),
            },
            "logs": self
                .logs
                .iter()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>(),
        });
        let name = format!(
            "scrcpy-gui-logs-{}.json",
            chrono::Utc::now().timestamp_millis()
        );
        let sink = self.sink.clone();
        let locale = self.locale.clone();
        let content = serde_json::to_string_pretty(&payload).unwrap_or_default();
        let toast_title = t(&self.locale, "logPanel.report").to_string();
        let toast_detail = tv(
            &self.locale,
            "logPanel.reportSavedAlert",
            &[("fileName", &name)],
        );
        self.runtime.spawn(async move {
            let message = match commands::save_report(content, name.clone()).await {
                Ok(path) => {
                    sink.emit(
                        "app-toast",
                        serde_json::json!({
                            "title": toast_title,
                            "description": toast_detail,
                        }),
                    );
                    tv(
                        &locale,
                        "logPanel.diagnosticReportSaved",
                        &[("fileName", &name), ("path", &path)],
                    )
                }
                Err(e) => format!("[ERROR] {e}"),
            };
            sink.emit("scrcpy-log", message);
        });
    }

    pub fn set_active_device(&self, device: Option<String>) {
        let _ = commands::set_active_device(self.state.clone(), device);
    }

    pub fn set_scrcpy_path(&mut self, path: Option<String>) {
        self.config.scrcpy_path = path.filter(|p| !p.is_empty());
        self.open_select = None;
        // A manual folder choice is authoritative; a pending rollback would
        // otherwise overwrite it on the next failed download.
        self.download_prev_path = None;
        self.save_config();
        self.check_scrcpy();
        self.refresh_devices();
    }

    pub fn set_record_path(&mut self, path: String) {
        self.config.record_path = Some(path);
        self.save_config();
    }
}

/// True for both download-progress shapes: the typed `scrcpy-status` event and
/// the legacy top-level name. `poll` uses it to drop superseded readings.
fn is_download_progress(ev: &AppEvent) -> bool {
    ev.name == "download-progress"
        || (ev.name == "scrcpy-status"
            && ev.payload.get("type").and_then(Value::as_str) == Some("download-progress"))
}

/// Copies the backend-resolved friendly names into the shared cache. A falls
/// back to this map on every successful `get_devices`; older payloads may also
/// expose it as `friendlyNames`.
fn merge_backend_friendly(v: &Value, names: &Arc<Mutex<HashMap<String, String>>>) {
    for key in ["friendly", "friendlyNames"] {
        let Some(map) = v.get(key).and_then(Value::as_object) else {
            continue;
        };
        let mut cache = names.lock_alive();
        for (serial, name) in map {
            if let Some(name) = name.as_str().filter(|name| !name.trim().is_empty()) {
                cache.insert(serial.clone(), name.trim().to_string());
            }
        }
    }
}

/// Builds the hub rows from either `["serial"]` or `{serial, model, friendly}`
/// entries, preferring an explicit name, then the cache, then the adb model.
fn devices_from_payload(v: &Value, names: &HashMap<String, String>) -> Vec<Device> {
    let models = v
        .get("deviceModels")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let Some(entries) = v.get("devices").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for entry in entries {
        let (serial, model, friendly) = match entry {
            Value::String(serial) => (serial.clone(), None, None),
            Value::Object(object) => {
                let Some(serial) = object.get("serial").and_then(Value::as_str) else {
                    continue;
                };
                (
                    serial.to_string(),
                    object
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    object
                        .get("friendly")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                )
            }
            _ => continue,
        };
        let model = model
            .or_else(|| {
                models
                    .get(&serial)
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| serial.clone());
        let friendly = friendly.or_else(|| names.get(&serial).cloned());
        devices.push(Device {
            serial,
            model,
            friendly,
        });
    }
    devices
}

fn host_part(value: &str) -> &str {
    value
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(value)
}

fn same_host(a: &str, b: &str) -> bool {
    !a.is_empty() && host_part(a).eq_ignore_ascii_case(host_part(b))
}

/// Whether a recent `ip:port` is already in the hub. Exact serial equality
/// covers plain `adb connect` sessions; a phone reconnecting over mDNS appears
/// as `adb-<instance>._adb-tls-connect._tcp`, so compare the mDNS service's
/// host (its advertised port is random) and instance name instead.
fn history_entry_connected(entry: &str, devices: &[Device], mdns: &[MdnsService]) -> bool {
    if devices.iter().any(|device| device.serial == entry) {
        return true;
    }
    devices.iter().any(|device| {
        mdns.iter().any(|service| {
            device.serial.contains(service.name.as_str())
                && (service.address == entry || same_host(&service.address, entry))
        })
    })
}

/// One ADB + mDNS pass shared by the settle loop, `refresh_mdns` and the 5 s
/// poll. Publishes both lists and returns whether the device read succeeded; a
/// failed read leaves the last good lists untouched.
async fn fetch_and_emit(
    sink: &Arc<EventSink>,
    path: Option<String>,
    last_sig: &Arc<Mutex<String>>,
    names: &Arc<Mutex<HashMap<String, String>>>,
    silent: bool,
) -> bool {
    let v = commands::get_devices(path.clone()).await;
    if v.get("error").and_then(Value::as_bool).unwrap_or(false) {
        if !silent {
            let message = v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("ADB error");
            sink.emit("scrcpy-log", format!("[ADB ERROR] {message}"));
        }
        return false;
    }

    merge_backend_friendly(&v, names);
    let devices = {
        let cache = names.lock_alive();
        devices_from_payload(&v, &cache)
    };

    let device_sig = devices
        .iter()
        .map(|device| device.serial.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let previous = last_sig.lock_alive().clone();
    let previous_mdns_sig = previous
        .split_once('#')
        .map(|(_, mdns)| mdns.to_string())
        .unwrap_or_default();

    let mdns_v = commands::get_mdns_devices(path).await;
    let mdns_ok = !mdns_v
        .get("error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let services: Vec<Value> = mdns_v
        .get("services")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|service| {
            let kind = service.get("service").and_then(Value::as_str).unwrap_or("");
            kind.contains("_adb-tls-connect") || kind.contains("_adb-tls-pairing")
        })
        .collect();
    let mdns_sig = if mdns_ok {
        services
            .iter()
            .map(|service| {
                format!(
                    "{}|{}",
                    service.get("address").and_then(Value::as_str).unwrap_or(""),
                    service.get("name").and_then(Value::as_str).unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    } else {
        previous_mdns_sig
    };

    *last_sig.lock_alive() = format!("{device_sig}#{mdns_sig}");
    sink.emit("device-list", devices);
    if mdns_ok {
        sink.emit("mdns-list", services);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `Ui` without touching prefs: `fixture` makes every `save_*`
    /// call a no-op, and the default config replaces whatever is on disk.
    #[allow(clippy::field_reassign_with_default)]
    fn test_ui() -> Ui {
        let mut ui = Ui::default();
        ui.fixture = true;
        ui.onboarding_open = false;
        ui.config = default_config();
        ui
    }

    #[test]
    fn session_kind_reads_persisted_mode() {
        assert_eq!(SessionKind::from_mode("mirror"), SessionKind::Mirror);
        assert_eq!(SessionKind::from_mode("Camera"), SessionKind::Camera);
        assert_eq!(SessionKind::from_mode(" DESKTOP "), SessionKind::Desktop);
        assert_eq!(SessionKind::from_mode(""), SessionKind::Mirror);
        assert_eq!(SessionKind::from_mode("unknown"), SessionKind::Mirror);
    }

    #[test]
    fn set_tab_persists_session_mode_and_closes_dropdown() {
        let mut ui = test_ui();
        ui.open_select = Some("codec".into());
        ui.set_tab(SessionKind::Camera);
        assert_eq!(ui.tab, SessionKind::Camera);
        assert_eq!(ui.config.session_mode, "camera");
        assert_eq!(ui.open_select, None);
    }

    #[test]
    fn running_false_removes_the_device() {
        let mut ui = test_ui();
        ui.handle_status(&serde_json::json!({ "device": "abc", "running": true }));
        assert_eq!(ui.running.get("abc"), Some(&true));
        ui.handle_status(&serde_json::json!({ "device": "abc", "running": false }));
        assert!(!ui.running.contains_key("abc"));
    }

    #[test]
    fn top_level_download_progress_is_still_accepted() {
        let mut ui = test_ui();
        ui.handle_event(AppEvent::new(
            "download-progress",
            serde_json::json!({ "percent": 42 }),
        ));
        assert_eq!(ui.download.percent, 42);
        assert!(ui.download.active);
    }

    #[test]
    fn poll_keeps_only_the_newest_progress_reading() {
        let mut ui = test_ui();
        ui.sink.emit(
            "scrcpy-status",
            serde_json::json!({ "type": "download-progress", "percent": 5 }),
        );
        ui.sink
            .emit("download-progress", serde_json::json!({ "percent": 61 }));
        ui.sink
            .emit("download-progress", serde_json::json!({ "percent": 77 }));
        assert!(ui.poll());
        assert_eq!(ui.download.percent, 77);
    }

    #[test]
    fn failed_download_restores_the_custom_path() {
        let mut ui = test_ui();
        ui.config.scrcpy_path = None;
        ui.download_prev_path = Some(Some(r"C:\scrcpy".into()));
        ui.handle_status(&serde_json::json!({
            "type": "download-complete",
            "success": false,
            "message": "boom",
        }));
        assert_eq!(ui.config.scrcpy_path.as_deref(), Some(r"C:\scrcpy"));
    }

    #[test]
    fn close_top_layer_closes_the_dropdown_before_toasts() {
        let mut ui = test_ui();
        ui.open_select = Some("codec".into());
        ui.show_toast("t", "d");
        assert!(ui.close_top_layer());
        assert_eq!(ui.open_select, None);
        assert_eq!(ui.toasts.len(), 1);
        assert!(ui.close_top_layer());
        assert!(ui.toasts.is_empty());
    }

    #[test]
    fn close_top_layer_follows_the_view_push_order() {
        let mut ui = test_ui();
        ui.connect_open = true;
        ui.alert = Some("alert".into());
        ui.settings_open = true;
        assert!(ui.close_top_layer());
        assert_eq!(ui.alert, None);
        assert!(ui.connect_open);
        assert!(ui.close_top_layer());
        assert!(!ui.connect_open);
        assert!(ui.settings_open);
        assert!(ui.close_top_layer());
        assert!(!ui.settings_open);
    }

    #[test]
    fn mdns_serial_label_strips_the_wrapper_and_keeps_conflicts() {
        let device = Device {
            serial: "adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp".into(),
            model: "PJA110".into(),
            friendly: None,
        };
        assert_eq!(device.label(), "ZY22MGW35T-Z3uXXq (2)");
        let named = Device {
            friendly: Some("OnePlus 12".into()),
            ..device
        };
        assert_eq!(named.label(), "OnePlus 12");
    }

    #[test]
    fn history_matches_a_live_mdns_device_by_address() {
        let devices = vec![Device {
            serial: "adb-ABC123-random._adb-tls-connect._tcp".into(),
            model: "Phone".into(),
            friendly: None,
        }];
        let mdns = vec![MdnsService {
            name: "adb-ABC123-random".into(),
            service: "_adb-tls-connect._tcp".into(),
            address: "192.168.1.5:38517".into(),
        }];
        assert!(history_entry_connected("192.168.1.5:5555", &devices, &mdns));
        assert!(!history_entry_connected(
            "192.168.1.6:5555",
            &devices,
            &mdns
        ));
        assert!(!history_entry_connected("192.168.1.5:5555", &devices, &[]));
    }

    #[test]
    fn device_payload_merges_backend_friendly_names() {
        let names = Arc::new(Mutex::new(HashMap::new()));
        let payload = serde_json::json!({
            "devices": ["abc", { "serial": "def", "model": "Pixel" }],
            "deviceModels": { "abc": "adb model" },
            "friendly": { "abc": "Galaxy S23" },
        });
        merge_backend_friendly(&payload, &names);
        let devices = {
            let cache = names.lock_alive();
            devices_from_payload(&payload, &cache)
        };
        assert_eq!(devices[0].friendly.as_deref(), Some("Galaxy S23"));
        assert_eq!(devices[0].model, "adb model");
        assert_eq!(devices[1].serial, "def");
        assert_eq!(devices[1].model, "Pixel");
    }
}
