//! Application state and the bridge between Rune's UI thread and the async backend.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rune::prefs::Prefs;
use rune::{TextEdit, Waker};
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

#[derive(Clone, Debug, Default)]
pub struct Download {
    pub active: bool,
    pub percent: u32,
    pub message: String,
    pub success: bool,
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
    pub logs: Vec<String>,
    pub running: BTreeMap<String, bool>,
    pub history: Vec<String>,
    pub tab: SessionKind,
    pub ip: TextEdit,
    pub pair_ip: TextEdit,
    pub pair_code: TextEdit,
    pub command: TextEdit,
    pub background_color: TextEdit,
    pub open_select: Option<String>,
    pub alert: Option<String>,
    pub update: Option<UpdateInfo>,
    pub onboarding_open: bool,
    pub is_refreshing: bool,
    busy: Arc<AtomicBool>,
    friendly_names: Arc<Mutex<HashMap<String, String>>>,
    last_device_sig: Arc<Mutex<String>>,
    pub dark: bool,
    pub accent: usize,
    pub theme: String,
    pub locale: String,
    pub scrcpy_ok: Option<bool>,
    pub scrcpy_message: String,
    update_checked: bool,
    update_path: Option<String>,
    pub download: Download,
    pub waker: Option<Waker>,
    pub noise: rune::Pixels,
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

fn noise_texture() -> rune::Pixels {
    let pixels = rune::Pixels::new();
    let (w, h) = (1200u32, 890u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let mut seed = 0x1357_9bdfu32;
    for _ in 0..w * h {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let a = ((seed >> 24) as f32 / 255.0 * 102.0) as u8;
        rgba.extend_from_slice(&[255, 255, 255, a]);
    }
    pixels.set(w, h, rgba);
    pixels
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
        let theme: String = prefs.get("theme").unwrap_or_else(|| "ultraviolet".into());
        let history: Vec<String> = prefs.get("history").unwrap_or_default();
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
            ip: TextEdit::new(""),
            pair_ip: TextEdit::new(""),
            pair_code: TextEdit::new(""),
            command: TextEdit::new(""),
            background_color: TextEdit::new(""),
            open_select: None,
            alert: None,
            update: None,
            onboarding_open: true,
            is_refreshing: false,
            busy: Arc::new(AtomicBool::new(false)),
            friendly_names: Arc::new(Mutex::new(HashMap::new())),
            last_device_sig: Arc::new(Mutex::new(String::new())),
            dark,
            accent: 0,
            theme,
            locale,
            scrcpy_ok: None,
            scrcpy_message: String::new(),
            update_checked: false,
            update_path: None,
            download: Download::default(),
            waker: None,
            noise: noise_texture(),
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
        self.reconnect_history();
        self.start_history_reconnect();
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
            "refresh-request" => self.refresh_devices(),
            "refresh-done" => self.is_refreshing = false,
            "history-add" => {
                if let Ok(v) = serde_json::from_value::<IpPayload>(ev.payload) {
                    self.add_history(v.ip);
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
                self.check_scrcpy();
                self.refresh_devices();
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
        if self.logs.len() > 100 {
            self.logs.drain(..self.logs.len() - 100);
        }
    }

    pub fn save_config(&mut self) {
        let _ = self.prefs.set("config", &self.config);
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        let _ = self.prefs.set("dark", &dark);
    }

    pub fn set_theme(&mut self, theme: &str) {
        self.theme = theme.to_string();
        let _ = self.prefs.set("theme", &self.theme);
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.into();
        let _ = self.prefs.set("locale", &self.locale);
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
        self.is_refreshing = true;
        self.runtime.spawn(async move {
            let result =
                commands::adb_connect(sink.clone(), ip.clone(), path.clone(), Some(false)).await;
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
                        let _ = commands::adb_connect(
                            sink.clone(),
                            ip.clone(),
                            path.clone(),
                            Some(false),
                        )
                        .await;
                    }
                    sink.emit("scrcpy-log", format!("[SYSTEM] {message}"));
                    sink.emit("history-add", serde_json::json!({ "ip": ip }));
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(e) => sink.emit("scrcpy-log", format!("[ADB ERROR] {e}")),
            }
            sink.emit("refresh-done", serde_json::json!({}));
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
            sink.emit("refresh-request", serde_json::json!({}));
        });
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

    pub fn download_scrcpy(&self) {
        let sink = self.sink.clone();
        self.runtime.spawn(async move {
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
                "scrcpy_theme": self.theme,
                "scrcpy_color_mode": if self.dark { "dark" } else { "light" },
                "scrcpy_locale": self.locale,
                "scrcpy_history": serde_json::to_string(&self.history).unwrap_or_default(),
            },
            "logs": self.logs,
        });
        let name = format!(
            "scrcpy-gui-logs-{}.json",
            chrono::Utc::now().timestamp_millis()
        );
        let sink = self.sink.clone();
        let content = serde_json::to_string_pretty(&payload).unwrap_or_default();
        self.runtime.spawn(async move {
            let message = match commands::save_report(content, name.clone()).await {
                Ok(path) => tv(
                    "zh-CN",
                    "logPanel.diagnosticReportSaved",
                    &[("fileName", &name), ("path", &path)],
                ),
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
