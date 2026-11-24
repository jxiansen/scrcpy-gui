//! Application state and the bridge between Rune's UI thread and the async backend.

use std::collections::BTreeMap;
use std::sync::Arc;

use rune::prefs::Prefs;
use rune::{TextEdit, Waker};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::backend::{AppEvent, EventSink, ScrcpyConfig, SharedState, commands};

const PREFS_ID: &str = "com.kb.scrcpy-gui";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub model: String,
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

/// Where a download stands. Mirrors the `scrcpy-status {type: ...}` events the
/// old frontend listened for.
#[derive(Clone, Debug, Default)]
pub struct Download {
    pub active: bool,
    pub percent: u32,
    pub message: String,
    pub success: bool,
}

#[derive(Deserialize)]
struct UpdateInfo {
    local: String,
    latest: String,
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

pub struct Ui {
    pub runtime: tokio::runtime::Runtime,
    pub state: SharedState,
    pub sink: Arc<EventSink>,
    rx: UnboundedReceiver<AppEvent>,
    pub prefs: Prefs,
    pub config: ScrcpyConfig,
    pub devices: Vec<Device>,
    pub mdns: Vec<MdnsService>,
    pub logs: Vec<String>,
    pub running: BTreeMap<String, bool>,
    pub tab: SessionKind,
    pub ip: TextEdit,
    pub pair_ip: TextEdit,
    pub pair_code: TextEdit,
    pub command: TextEdit,
    pub background_color: TextEdit,
    pub open_select: Option<String>,
    pub alert: Option<String>,
    pub onboarding_open: bool,
    pub dark: bool,
    pub accent: usize,
    pub locale: String,
    pub scrcpy_ok: Option<bool>,
    pub scrcpy_message: String,
    pub download: Download,
    pub waker: Option<Waker>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MdnsService {
    pub name: String,
    pub service: String,
    pub address: String,
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
        let dark = prefs.get("dark").unwrap_or(true);
        let locale: String = prefs.get("locale").unwrap_or_else(|| "zh-CN".into());
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
            tab: SessionKind::Mirror,
            ip: TextEdit::new(""),
            pair_ip: TextEdit::new(""),
            pair_code: TextEdit::new(""),
            command: TextEdit::new(""),
            background_color: TextEdit::new(""),
            open_select: None,
            alert: None,
            onboarding_open: true,
            dark,
            accent: 0,
            locale,
            scrcpy_ok: None,
            scrcpy_message: String::new(),
            download: Download::default(),
            waker: None,
        }
    }
}

impl Ui {
    pub fn setup(&mut self, waker: Waker) {
        self.sink.set_waker(waker.clone());
        self.waker = Some(waker);
        self.log("[SYSTEM] ScrcpyGUI ready".into());
        #[cfg(target_os = "windows")]
        crate::platform::grab::register(self.state.clone());
        self.check_scrcpy();
        self.refresh_devices();
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            self.handle_event(ev);
            changed = true;
        }
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
                        self.check_update();
                    }
                }
            }
            "scrcpy-window-pos" => {
                if let Ok(pos) = serde_json::from_value::<WindowPos>(ev.payload) {
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
            "update-available" => {
                if let Ok(v) = serde_json::from_value::<UpdateInfo>(ev.payload) {
                    self.alert = Some(format!(
                        "Scrcpy update available: {} → {}",
                        v.local, v.latest
                    ));
                }
            }
            "mdns-list" => {
                if let Ok(list) = serde_json::from_value::<Vec<MdnsService>>(ev.payload) {
                    self.mdns = list;
                }
            }
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
                self.check_scrcpy();
            }
            _ => {}
        }
    }

    fn log(&mut self, text: String) {
        for line in text.split('\n') {
            if !line.trim().is_empty() {
                self.logs.push(line.to_string());
            }
        }
        if self.logs.len() > 500 {
            self.logs.drain(..self.logs.len() - 500);
        }
    }

    pub fn save_config(&mut self) {
        let _ = self.prefs.set("config", &self.config);
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        let _ = self.prefs.set("dark", &dark);
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.into();
        let _ = self.prefs.set("locale", &self.locale);
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

    pub fn refresh_devices(&self) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::get_devices(path).await;
            if v.get("error").and_then(Value::as_bool).unwrap_or(false) {
                let message = v
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("ADB error");
                sink.emit("scrcpy-log", format!("[ADB ERROR] {message}"));
                sink.emit("device-list", Vec::<Device>::new());
                return;
            }
            let models = v
                .get("deviceModels")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
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
                        })
                        .collect()
                })
                .unwrap_or_default();
            sink.emit("device-list", devices);
        });
    }

    pub fn refresh_mdns(&self) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::get_mdns_devices(path).await;
            let services = v
                .get("services")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            sink.emit("mdns-list", services);
        });
    }

    pub fn connect(&self, ip: String) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            match commands::adb_connect(sink.clone(), ip.clone(), path, Some(false)).await {
                Ok(v) => {
                    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
                    sink.emit("scrcpy-log", format!("[SYSTEM] {message}"));
                }
                Err(e) => sink.emit("scrcpy-log", format!("[ADB ERROR] {e}")),
            }
            let v = commands::get_devices(None).await;
            if let Some(devices) = v.get("devices").and_then(Value::as_array) {
                let serials: Vec<String> = devices
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                let models = v
                    .get("deviceModels")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let list: Vec<Device> = serials
                    .into_iter()
                    .map(|s| Device {
                        model: models
                            .get(&s)
                            .and_then(Value::as_str)
                            .unwrap_or(&s)
                            .to_string(),
                        serial: s,
                    })
                    .collect();
                sink.emit("device-list", list);
            }
        });
    }

    pub fn pair(&self, ip: String, code: String) {
        let sink = self.sink.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            match commands::adb_pair(sink.clone(), ip, code, path).await {
                Ok(v) => {
                    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
                    sink.emit("scrcpy-log", format!("[SYSTEM] {message}"));
                }
                Err(e) => sink.emit("scrcpy-log", format!("[ADB ERROR] {e}")),
            }
        });
    }

    pub fn start_session(&mut self) {
        if self.config.device.is_empty() {
            if let Some(d) = self.devices.first() {
                self.config.device = d.serial.clone();
            }
        }
        if self.config.device.is_empty() {
            self.log("[SYSTEM] No device selected".into());
            return;
        }
        self.config.session_mode = self.tab.as_str().into();
        self.save_config();
        let sink = self.sink.clone();
        let state = self.state.clone();
        let config = self.config.clone();
        self.runtime.spawn(async move {
            if let Err(e) = commands::run_scrcpy(sink, state, config).await {
                // Errors are already logged by the command itself.
                let _ = e;
            }
        });
    }

    pub fn stop_session(&self, device: &str) {
        let state = self.state.clone();
        let device = device.to_string();
        self.runtime.spawn(async move {
            let _ = commands::stop_scrcpy(state, device).await;
        });
    }

    pub fn download_scrcpy(&self) {
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
            if let Err(e) = commands::download_scrcpy(sink).await {
                // download_scrcpy emits its own errors.
                let _ = e;
            }
        });
    }

    pub fn run_command(&self, cmd: String) {
        let sink = self.sink.clone();
        let device = self.config.device.clone();
        let path = self.config.scrcpy_path.clone();
        self.runtime.spawn(async move {
            let v = commands::run_terminal_command(Some(device), cmd.clone(), path).await;
            for key in ["stdout", "stderr", "message"] {
                if let Some(text) = v.get(key).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        sink.emit("scrcpy-log", format!("[ADB] {text}"));
                    }
                }
            }
        });
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
            sink.emit(
                "scrcpy-log",
                format!(
                    "[SYSTEM] {} {}",
                    if apk { "Installing" } else { "Pushing" },
                    text
                ),
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

    pub fn set_active_device(&self, device: Option<String>) {
        let state = self.state.clone();
        let _ = commands::set_active_device(state, device);
    }

    pub fn select_device(&mut self, serial: String) {
        self.config.device = serial.clone();
        self.save_config();
        let running = self.running.get(&serial).copied().unwrap_or(false);
        self.set_active_device(if running { Some(serial) } else { None });
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
