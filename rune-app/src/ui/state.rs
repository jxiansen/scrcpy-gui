//! Application state and the bridge between Rune's UI thread and the async backend.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rune::prefs::Prefs;
use rune::{Taskbar, TaskbarProgress, TextEdit, Waker};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::backend::{AppEvent, EventSink, ScrcpyConfig, SharedState, commands};
use crate::i18n::{t, tv};

const PREFS_ID: &str = "com.kb.scrcpy-gui";

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
        let s = self.serial.as_str();
        if let Some(rest) = s.strip_prefix("adb-") {
            if let Some(pos) = rest.find("._adb-tls-connect._") {
                let head = &rest[..pos];
                return match head.rfind(" (") {
                    Some(i) if head.ends_with(')') => &head[..i],
                    _ => head,
                };
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

    pub fn label(self) -> &'static str {
        match self {
            Self::Mirror => "Mirror",
            Self::Camera => "Camera",
            Self::Desktop => "Desktop",
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
}

#[derive(Clone, Debug, Deserialize)]
pub struct MdnsService {
    pub name: String,
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
}

#[derive(Deserialize)]
struct WindowPos {
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
    pub adb_menu: bool,
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
    pub drop_hover: bool,
    pub remember_position: bool,
    busy: Arc<AtomicBool>,
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
            tab: SessionKind::Mirror,
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
            waker: None,
            fixture: false,
        }
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
        self.connect_token.store(0, Ordering::SeqCst);
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
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            self.handle_event(ev);
            changed = true;
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
            "scrcpy-check" => {
                if let Ok(v) = serde_json::from_value::<ScrcpyCheck>(ev.payload) {
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
                {
                    self.config.window_x = Some(pos.x);
                    self.config.window_y = Some(pos.y);
                    self.save_config();
                }
            }
            "device-list" => {
                if let Ok(devices) = serde_json::from_value::<Vec<Device>>(ev.payload) {
                    self.devices = devices;
                }
            }
            "adb-killed" => self.refresh_devices(),
            "refresh-request" => self.refresh_devices(),
            "refresh-done" => {
                self.is_refreshing = false;
                self.connecting = false;
            }
            "pair-done" => self.pairing = false,
            "history-add" => {
                if let Ok(v) = serde_json::from_value::<IpPayload>(ev.payload) {
                    let ip = v.ip.clone();
                    let title = t(&self.locale, "header.scrcpyReady").to_string();
                    let detail = tv(&self.locale, "logs.connectedSuccessfully", &[("ip", &ip)]);
                    self.show_toast(title, detail);
                    self.add_history(v.ip);
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
            self.running.insert(device.to_string(), running);
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
            "download-progress" => {
                self.download.percent =
                    p.get("percent").and_then(Value::as_u64).unwrap_or(0) as u32;
            }
            "download-complete" => {
                self.download.active = false;
                self.download.success = p.get("success").and_then(Value::as_bool).unwrap_or(false);
                self.download.message = p
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into();
                if self.download.success {
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
                    self.log(format!("[SYSTEM] {}", self.download.message));
                }
            }
            _ => {}
        }
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
        let _ = self.prefs.set("config", &self.config);
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
        let _ = self.prefs.set("color_mode", &self.color_mode);
        let _ = self.prefs.set("dark", &self.dark);
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
        let _ = self.prefs.set("locale", &self.locale);
    }

    /// The GUI window's pin; independent of scrcpy's `config.always_on_top`.
    pub fn toggle_panel_on_top(&mut self) {
        self.panel_on_top = !self.panel_on_top;
        let _ = self.prefs.set("panel_on_top", &self.panel_on_top);
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
    /// call this, so a tray Quit keeps the last on-screen position too.
    pub fn save_window_placement(&mut self) {
        let Some(place) = crate::platform::window::placement() else {
            return;
        };
        let saved = SavedWindow {
            x: place.position.0,
            y: place.position.1,
            w: place.content_size.0,
            h: place.content_size.1,
            scale: place.scale as f32,
        };
        let _ = self.prefs.set("window", &saved);
    }

    /// Stops every device's scrcpy process, as the tray menu's "Stop all" does.
    pub fn stop_all(&self) {
        let devices: Vec<String> = self
            .state
            .processes
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
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
        let _ = self.prefs.set("history", &self.history);
    }

    pub fn toggle_remember_position(&mut self) {
        self.remember_position = !self.remember_position;
        if self.fixture {
            return;
        }
        let _ = self.prefs.set("remember_position", &self.remember_position);
        if !self.remember_position {
            self.config.window_x = None;
            self.config.window_y = None;
            self.save_config();
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        let _ = self.prefs.set("history", &self.history);
    }

    pub fn check_scrcpy(&self) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::check_scrcpy(path).await;
            let found = v.get("found").and_then(Value::as_bool).unwrap_or(false);
            let message = v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            sink.emit(
                "scrcpy-check",
                serde_json::json!({ "found": found, "message": message }),
            );
        });
    }

    /// One ADB + mDNS fetch, plus one friendly-name lookup per newly seen serial.
    fn fetch_devices_once(&self, silent: bool) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        let names = self.friendly_names.clone();
        let last_sig = self.last_device_sig.clone();
        self.runtime.spawn(async move {
            let v = commands::get_devices(path.clone()).await;
            if v.get("error").and_then(Value::as_bool).unwrap_or(false) {
                let message = v
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("ADB error");
                if !silent {
                    sink.emit("scrcpy-log", format!("[ADB ERROR] {message}"));
                }
                sink.emit("device-list", Vec::<Device>::new());
                return;
            }
            let models = v
                .get("deviceModels")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let mut devices: Vec<Device> = v
                .get("devices")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(|serial| Device {
                            serial: serial.to_string(),
                            model: models
                                .get(serial)
                                .and_then(Value::as_str)
                                .unwrap_or(serial)
                                .to_string(),
                            friendly: names.lock().unwrap().get(serial).cloned(),
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mdns = commands::get_mdns_devices(path.clone()).await;
            let services = mdns
                .get("services")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            sink.emit("mdns-list", services);

            let targets: Vec<String> = devices
                .iter()
                .map(|d| d.serial.clone())
                .filter(|serial| !names.lock().unwrap().contains_key(serial))
                .collect();
            for serial in &targets {
                let emulator = serial
                    .strip_prefix("emulator-")
                    .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
                let command = if emulator {
                    "getprop ro.boot.qemu.avd_name; getprop ro.kernel.qemu.avd_name"
                } else {
                    "getprop ro.product.marketname; getprop ro.product.manufacturer; getprop ro.product.model"
                };
                let res = commands::adb_shell(serial.clone(), command.into(), path.clone()).await;
                if res.get("success").and_then(Value::as_bool).unwrap_or(false) {
                    let output = res.get("output").and_then(Value::as_str).unwrap_or("");
                    let parts: Vec<&str> = output.lines().map(str::trim).collect();
                    let name = if emulator {
                        parts.iter().find(|s| !s.is_empty()).map(|s| {
                            format!("{} ({serial})", s.replace('_', " "))
                        })
                    } else {
                        let market = parts.first().copied().unwrap_or("");
                        let manufacturer = parts.get(1).copied().unwrap_or("");
                        let model = parts.get(2).copied().unwrap_or("");
                        if !market.is_empty() {
                            Some(market.to_string())
                        } else if !manufacturer.is_empty()
                            && !model.is_empty()
                            && !model
                                .to_lowercase()
                                .starts_with(&manufacturer.to_lowercase())
                        {
                            Some(format!("{manufacturer} {model}"))
                        } else if !model.is_empty() {
                            Some(model.to_string())
                        } else {
                            None
                        }
                    };
                    if let Some(name) = name {
                        names.lock().unwrap().insert(serial.clone(), name.clone());
                        if let Some(d) = devices.iter_mut().find(|d| d.serial == *serial) {
                            d.friendly = Some(name);
                        }
                    }
                }
            }
            *last_sig.lock().unwrap() = devices
                .iter()
                .map(|d| d.serial.as_str())
                .collect::<Vec<_>>()
                .join(",");
            sink.emit("device-list", devices);
        });
    }

    /// Manual refresh: up to 8 silent passes, 2 s apart, until the device set
    /// stops changing (mirrors `refreshDevicesUntilSettled` in the old hook).
    pub fn refresh_devices(&mut self) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.is_refreshing = true;
        let sink = self.sink.clone();
        let busy = self.busy.clone();
        let sig = self.last_device_sig.clone();
        let names = self.friendly_names.clone();
        self.runtime.spawn(async move {
            let mut previous = String::new();
            for i in 0..=8 {
                fetch_and_emit(&sink, &sig, &names).await;
                let current = sig.lock().unwrap().clone();
                if current == previous {
                    break;
                }
                previous = current;
                if i < 8 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
            sink.emit("refresh-done", serde_json::json!({}));
            busy.store(false, Ordering::SeqCst);
        });
    }

    pub fn refresh_mdns(&self) {
        self.fetch_devices_once(true);
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
        let connected: HashSet<String> = self.devices.iter().map(|d| d.serial.clone()).collect();
        let pending: Vec<String> = self
            .history
            .iter()
            .filter(|ip| !connected.contains(*ip))
            .cloned()
            .collect();
        for ip in pending {
            let sink = self.sink.clone();
            let path = self.config.scrcpy_path.clone();
            self.runtime.spawn(async move {
                let _ = commands::adb_connect(sink.clone(), ip, path, Some(true)).await;
            });
        }
    }

    pub fn connect(&mut self, ip: String) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        // A cancel or a newer connect bumps the token: a stale ADB attempt can
        // then neither report success nor clear the flags of its replacement.
        let token = self.connect_token.fetch_add(1, Ordering::SeqCst) + 1;
        let connect_token = self.connect_token.clone();
        let timeout_title = t(&self.locale, "toast.connectTimeout").to_string();
        let ip_label = ip.clone();
        self.is_refreshing = true;
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
                    // The future is dropped here; `refresh-done` resets the
                    // spinner through `handle_event`, like a normal completion.
                    sink.emit(
                        "app-toast",
                        serde_json::json!({ "title": timeout_title, "description": ip_label }),
                    );
                    sink.emit("refresh-done", serde_json::json!({}));
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if current() {
                    sink.emit("refresh-done", serde_json::json!({}));
                }
            }
        });
    }

    /// Cancels an in-flight `connect`: clears the busy flags now, shows the
    /// cancelled toast, and invalidates the ADB task so it stays silent.
    pub fn cancel_connect(&mut self) {
        self.connect_token.fetch_add(1, Ordering::SeqCst);
        let was_connecting = self.connecting || self.is_refreshing;
        self.connecting = false;
        self.is_refreshing = false;
        if was_connecting {
            let title = t(&self.locale, "toast.connectCancelled").to_string();
            let detail = self.ip.text.trim().to_string();
            self.show_toast(title, detail);
        }
    }

    pub fn pair(&mut self, ip: String, code: String) {
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

    /// Clears the selected device, stopping its session first if it is running.
    pub fn disconnect_device(&mut self) {
        let serial = self.config.device.clone();
        if serial.is_empty() {
            return;
        }
        if self.running.get(&serial).copied().unwrap_or(false) {
            self.stop_session(&serial);
        }
        self.config.device.clear();
        self.save_config();
        self.set_active_device(None);
    }

    /// Asks before clearing the device while its session is live; otherwise
    /// disconnects straight away.
    pub fn request_disconnect(&mut self) {
        let serial = self.config.device.as_str();
        let live = !serial.is_empty() && self.running.get(serial).copied().unwrap_or(false);
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

    /// Closes the topmost layer, innermost dialog first; `false` means the key
    /// was not handled and may bubble. Order: toasts, the modal dialogs, then
    /// the settings / console / advanced drawers.
    pub fn close_top_layer(&mut self) -> bool {
        if self.toasts.pop().is_some() {
            return true;
        }
        if self.connect_open {
            self.connect_open = false;
            return true;
        }
        if self.about_open {
            self.about_open = false;
            return true;
        }
        if self.confirm_kill_adb {
            self.confirm_kill_adb = false;
            return true;
        }
        if self.update.is_some() {
            self.update = None;
            return true;
        }
        if self.onboarding_open {
            self.onboarding_open = false;
            return true;
        }
        if self.alert.is_some() {
            self.alert = None;
            return true;
        }
        if self.disconnect_confirm {
            self.disconnect_confirm = false;
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
        self.save_config();
        let running = self.running.get(&serial).copied().unwrap_or(false);
        self.set_active_device(if running { Some(serial) } else { None });
    }

    pub fn start_session(&mut self) {
        if self.config.device.is_empty() {
            self.alert = Some(t(&self.locale, "alerts.noDeviceSelectedMessage").to_string());
            return;
        }
        self.config.session_mode = self.tab.as_str().into();
        self.save_config();
        self.log(tv(
            &self.locale,
            "logs.initializingScrcpy",
            &[("device", &self.config.device)],
        ));
        let sink = self.sink.clone();
        let state = self.state.clone();
        let config = self.config.clone();
        self.runtime.spawn(async move {
            let _ = commands::run_scrcpy(sink, state, config).await;
        });
    }

    pub fn stop_session(&self, device: &str) {
        let state = self.state.clone();
        let device = device.to_string();
        self.runtime.spawn(async move {
            let _ = commands::stop_scrcpy(state, device).await;
        });
    }

    pub fn download_scrcpy(&mut self) {
        if self.download.active {
            return;
        }
        self.download.active = true;
        // Download always extracts to the default exe-adjacent folder; a stale
        // custom path would make the freshly installed binary invisible.
        self.config.scrcpy_path = None;
        self.save_config();
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            if let Err(e) = commands::download_scrcpy(sink.clone()).await {
                sink.emit(
                    "scrcpy-status",
                    serde_json::json!({
                        "type": "download-complete",
                        "success": false,
                        "message": e,
                    }),
                );
            }
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

    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
    }

    pub fn toggle_adb_menu(&mut self) {
        self.adb_menu = !self.adb_menu;
        if !self.adb_menu {
            self.confirm_kill_adb = false;
        }
    }

    pub fn confirm_kill_adb(&mut self) {
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

    pub fn handle_file(&self, path: std::path::PathBuf) {
        let sink = self.sink.clone();
        let device = self.config.device.clone();
        let custom = self.config.scrcpy_path.clone();
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
                tv("zh-CN", key, &[("device", &device), ("filePath", &text)]),
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

    pub fn update_now(&mut self) {
        self.update = None;
        self.config.scrcpy_path = None;
        self.save_config();
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
                        "zh-CN",
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
        self.save_config();
        self.check_scrcpy();
        self.refresh_devices();
    }

    pub fn set_record_path(&mut self, path: String) {
        self.config.record_path = Some(path);
        self.save_config();
    }
}

/// Used by the settle loop: fetch devices and publish a compact signature so
/// the loop can tell whether the set changed between passes.
async fn fetch_and_emit(
    sink: &Arc<EventSink>,
    last_sig: &Arc<Mutex<String>>,
    names: &Arc<Mutex<HashMap<String, String>>>,
) {
    let v = commands::get_devices(None).await;
    let models = v
        .get("deviceModels")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let names_snapshot = names.lock().unwrap().clone();
    let devices: Vec<Device> = v
        .get("devices")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|serial| Device {
                    serial: serial.to_string(),
                    model: models
                        .get(serial)
                        .and_then(Value::as_str)
                        .unwrap_or(serial)
                        .to_string(),
                    friendly: names_snapshot.get(serial).cloned(),
                })
                .collect()
        })
        .unwrap_or_default();

    let device_sig = devices
        .iter()
        .map(|d| d.serial.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let mdns_v = commands::get_mdns_devices(None).await;
    let services: Vec<Value> = mdns_v
        .get("services")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| {
            let service = v.get("service").and_then(Value::as_str).unwrap_or("");
            service.contains("_adb-tls-connect") || service.contains("_adb-tls-pairing")
        })
        .collect();
    let mdns_sig = services
        .iter()
        .map(|v| {
            format!(
                "{}|{}",
                v.get("address").and_then(Value::as_str).unwrap_or(""),
                v.get("name").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    *last_sig.lock().unwrap() = format!("{device_sig}#{mdns_sig}");
    sink.emit("device-list", devices);
    sink.emit("mdns-list", services);
}
