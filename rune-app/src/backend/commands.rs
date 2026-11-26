use super::{EventSink, ScrcpyState};
use serde_json::json;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

const AUDIO_FALLBACK_CHAIN: &[&str] = &["aac", "flac", "raw"];

/// ADB device listings/pairing must never hang the async bridge forever.
const ADB_LIST_TIMEOUT: Duration = Duration::from_secs(10);
const ADB_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const ADB_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const ADB_PAIR_TIMEOUT: Duration = Duration::from_secs(20);

/// Console commands are read incrementally; the transcript is capped so a
/// runaway command cannot grow the UI transcript without bound.
const MAX_CONSOLE_OUTPUT: usize = 1024 * 1024;
const CONSOLE_TIMEOUT: Duration = Duration::from_secs(120);
const AUDIO_FALLBACK_DELAY: Duration = Duration::from_millis(250);

fn is_audio_codec_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("could not create default audio encoder")
        || lower.contains("failed to initialize audio")
    {
        return true;
    }
    let mentions_codec = lower.contains("audio encoder") || lower.contains("audio codec");
    let mentions_failure = lower.contains("fail")
        || lower.contains("error")
        || lower.contains("could not")
        || lower.contains("not available")
        || lower.contains("not supported");
    mentions_codec && mentions_failure
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn binary_filename(binary_name: &str) -> String {
    let exe_ext = std::env::consts::EXE_EXTENSION;
    if exe_ext.is_empty() {
        binary_name.to_string()
    } else {
        format!("{}.{}", binary_name, exe_ext)
    }
}

/// Development convenience only: a release build must not execute binaries
/// found next to whatever directory it happened to be started in.
fn include_cwd_scrcpy_bin() -> bool {
    cfg!(debug_assertions)
}

fn get_binary_path(binary_name: &str, custom_folder: Option<String>) -> String {
    let binary_filename = binary_filename(binary_name);

    // 1. Check custom folder if provided
    if let Some(folder) = custom_folder {
        if !folder.trim().is_empty() {
            let full_path = Path::new(&folder).join(&binary_filename);
            if full_path.exists() && full_path.is_file() {
                return full_path.to_string_lossy().to_string();
            }
        }
    }

    // 2. Check local scrcpy-bin folder (relative to executable for portable/production)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let local_bin = exe_dir.join("scrcpy-bin").join(&binary_filename);
            if local_bin.exists() && local_bin.is_file() {
                return local_bin.to_string_lossy().to_string();
            }
        }
    }

    // 3. Fallback to current working directory scrcpy-bin. Only in debug
    // builds: release binaries must not pick up executables from the cwd.
    if include_cwd_scrcpy_bin() {
        if let Ok(current_dir) = std::env::current_dir() {
            let local_bin = current_dir.join("scrcpy-bin").join(&binary_filename);
            if local_bin.exists() && local_bin.is_file() {
                return local_bin.to_string_lossy().to_string();
            }
        }
    }

    // 4. Return simple name to rely on system PATH
    binary_name.to_string()
}

/// Absolute System32 path for OS tools, so a hostile `PATH` entry cannot
/// shadow `taskkill`.
#[cfg(target_os = "windows")]
fn system32_tool(tool: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join(tool)
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn create_command<S: AsRef<std::ffi::OsStr>>(program: S) -> TokioCommand {
    let mut cmd = TokioCommand::new(program);
    // A timed-out ADB/scrcpy helper must not outlive its Future.
    cmd.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub async fn check_scrcpy(custom_path: Option<String>) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);

    // Check version to verify it exists and is runnable
    let output = create_command(&exe_path).arg("--version").output().await;

    match output {
        Ok(o) if o.status.success() => {
            json!({ "found": true, "message": "Scrcpy Ready" })
        }
        Ok(_) => {
            json!({ "found": false, "message": "Failed to start scrcpy (Exit Code != 0)" })
        }
        Err(_) => {
            json!({ "found": false, "message": "Scrcpy not found" })
        }
    }
}

/// ADB device states that can appear in the second column of `adb devices -l`
/// output. Only "device" means the device is connected and authorized; every
/// other state (offline, mid-pairing, unauthorized, ...) is not usable yet.
const KNOWN_ADB_STATES: &[&str] = &[
    "device",
    "offline",
    "unauthorized",
    "authorizing",
    "unknown",
    "bootloader",
    "recovery",
    "sideload",
    "host",
    "connecting",
];

/// Parses one data line of `adb devices -l` output into `(serial, state)`.
///
/// The serial and state are whitespace-separated, but the serial itself can
/// contain a space: an mDNS wireless-debugging serial in "conflict" form
/// (when two devices would otherwise resolve to the same mDNS name) looks
/// like `adb-SERIAL (2)._adb-tls-connect._tcp`. Splitting on the first
/// whitespace run, or assuming a specific serial shape, would misparse that
/// case. Instead this finds the first token that is a *known ADB state* and
/// treats everything before it as the serial, so it corresponds to the state
/// ADB actually reports, regardless of the serial's shape (USB ID, IP:port,
/// or mDNS name). Any further `-l` fields (`product:... model:...
/// transport_id:...`) are simply ignored.
fn parse_adb_device_line(line: &str) -> Option<(String, String)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let state_idx = tokens.iter().position(|t| KNOWN_ADB_STATES.contains(t))?;
    if state_idx == 0 {
        return None; // no serial before the state
    }
    Some((tokens[..state_idx].join(" "), tokens[state_idx].to_string()))
}

/// Extracts the `model:` field from a `adb devices -l` line, if present, for
/// a more user-friendly device label than the raw serial (e.g. "SM S911B"
/// instead of a USB serial or IP:port). Adb replaces spaces in the model name
/// with underscores in this raw form; those are turned back into spaces,
/// which is as far as this goes without an extra `getprop` round-trip per
/// device -- still closer to a real name than a serial.
fn extract_device_model(line: &str) -> Option<String> {
    let model = line
        .split_whitespace()
        .find_map(|t| t.strip_prefix("model:"))?;
    if model.is_empty() {
        return None;
    }
    Some(model.replace('_', " "))
}

fn is_emulator_serial(serial: &str) -> bool {
    serial
        .strip_prefix("emulator-")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Turns `getprop` output (one value per line) into the display name the UI
/// shows: the AVD name for emulators, otherwise the market name, or
/// `manufacturer model` when the market name is empty / model already includes
/// the manufacturer.
fn friendly_name_from_getprop(serial: &str, output: &str) -> Option<String> {
    let parts: Vec<&str> = output.lines().map(str::trim).collect();
    if is_emulator_serial(serial) {
        return parts
            .iter()
            .find(|s| !s.is_empty())
            .map(|s| format!("{} ({serial})", s.replace('_', " ")));
    }

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
}

fn friendly_names_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn query_friendly_name(adb_path: &str, serial: &str) -> Option<String> {
    let command = if is_emulator_serial(serial) {
        "getprop ro.boot.qemu.avd_name; getprop ro.kernel.qemu.avd_name"
    } else {
        "getprop ro.product.marketname; getprop ro.product.manufacturer; getprop ro.product.model"
    };
    let output = timeout(
        ADB_QUERY_TIMEOUT,
        create_command(adb_path)
            .arg("-s")
            .arg(serial)
            .arg("shell")
            .arg(command)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    friendly_name_from_getprop(serial, &String::from_utf8_lossy(&output.stdout))
}

async fn resolve_friendly_names(
    adb_path: &str,
    devices: &[String],
) -> serde_json::Map<String, serde_json::Value> {
    let mut resolved = serde_json::Map::new();
    let mut pending = Vec::new();
    {
        let cache = friendly_names_cache().lock().unwrap();
        for serial in devices {
            if let Some(name) = cache.get(serial) {
                resolved.insert(serial.clone(), json!(name));
            } else {
                pending.push(serial.clone());
            }
        }
    }

    let mut tasks = tokio::task::JoinSet::new();
    for serial in pending {
        let adb_path = adb_path.to_string();
        tasks.spawn(async move {
            let name = query_friendly_name(&adb_path, &serial).await;
            (serial, name)
        });
    }
    while let Some(joined) = tasks.join_next().await {
        if let Ok((serial, Some(name))) = joined {
            friendly_names_cache()
                .lock()
                .unwrap()
                .insert(serial.clone(), name.clone());
            resolved.insert(serial, json!(name));
        }
    }
    resolved
}

pub async fn get_devices(custom_path: Option<String>) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = timeout(
        ADB_LIST_TIMEOUT,
        create_command(&adb_path).arg("devices").arg("-l").output(),
    )
    .await;

    match output {
        Ok(Ok(o)) => {
            if o.status.success() {
                let out_str = String::from_utf8_lossy(&o.stdout);
                let mut devices: Vec<String> = Vec::new();
                let mut device_models = serde_json::Map::new();
                for line in out_str.lines().skip(1) {
                    // Skip "List of devices attached"
                    let Some((serial, state)) = parse_adb_device_line(line) else {
                        continue;
                    };
                    if state != "device" {
                        continue;
                    }
                    if let Some(model) = extract_device_model(line) {
                        device_models.insert(serial.clone(), json!(model));
                    }
                    devices.push(serial);
                }

                let friendly = resolve_friendly_names(&adb_path, &devices).await;

                json!({
                    "error": false,
                    "devices": devices,
                    "deviceModels": device_models,
                    "friendly": friendly
                })
            } else {
                json!({ "error": true, "message": "ADB returned error" })
            }
        }
        Ok(Err(e)) => {
            json!({ "error": true, "message": e.to_string() })
        }
        Err(_) => {
            json!({ "error": true, "message": "ADB devices timed out after 10s" })
        }
    }
}

pub async fn get_mdns_devices(custom_path: Option<String>) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = timeout(
        ADB_LIST_TIMEOUT,
        create_command(&adb_path)
            .arg("mdns")
            .arg("services")
            .output(),
    )
    .await;

    match output {
        Ok(Ok(o)) => {
            if o.status.success() {
                let out_str = String::from_utf8_lossy(&o.stdout);
                let mut services = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for line in out_str.lines().skip(1) {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 3 {
                        let name = parts[0].trim();
                        let service = parts[1].trim();
                        let address = parts[2].trim();
                        let key = format!("{}|{}|{}", name, service, address);
                        if !seen.contains(&key) {
                            services.push(json!({
                                "name": name,
                                "service": service,
                                "address": address
                            }));
                            seen.insert(key);
                        }
                    }
                }
                json!({ "error": false, "services": services })
            } else {
                json!({ "error": true, "message": "ADB mdns returned error" })
            }
        }
        Ok(Err(e)) => {
            json!({ "error": true, "message": e.to_string() })
        }
        Err(_) => {
            json!({ "error": true, "message": "ADB mdns timed out after 10s" })
        }
    }
}

pub async fn adb_connect(
    sink: Arc<EventSink>,
    ip: String,
    custom_path: Option<String>,
    silent: Option<bool>,
) -> Result<serde_json::Value, String> {
    let silent = silent.unwrap_or(false);
    let adb_path = get_binary_path("adb", custom_path);
    if !silent {
        sink.emit(
            "scrcpy-log",
            format!("[SYSTEM] Attempting wireless connection to {}...", ip),
        );
    }

    let child = create_command(&adb_path)
        .arg("connect")
        .arg(&ip)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start adb connect: {}", e))?;

    // Implement 5s timeout
    let output_res = timeout(ADB_CONNECT_TIMEOUT, child.wait_with_output()).await;

    match output_res {
        Ok(Ok(output)) => {
            let out_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let err_text = String::from_utf8_lossy(&output.stderr).trim().to_string();

            if !silent {
                // Log everything to terminal for visibility
                if !out_text.is_empty() {
                    sink.emit("scrcpy-log", format!("[ADB] {}", out_text));
                }
                if !err_text.is_empty() {
                    sink.emit("scrcpy-log", format!("[ADB ERROR] {}", err_text));
                }
            }

            let success = output.status.success()
                && !out_text.contains("cannot connect")
                && !out_text.contains("failed");
            Ok(
                json!({ "success": success, "message": if out_text.is_empty() { err_text } else { out_text } }),
            )
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            if !silent {
                sink.emit(
                    "scrcpy-log",
                    format!("[SYSTEM] Connection to {} timed out after 5s.", ip),
                );
            }
            Ok(json!({ "success": false, "message": "connection timed out" }))
        }
    }
}

pub async fn adb_pair(
    sink: Arc<EventSink>,
    ip: String,
    code: String,
    custom_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let adb_path = get_binary_path("adb", custom_path);
    sink.emit("scrcpy-log", format!("[SYSTEM] Pairing with {}...", ip));

    let output = timeout(
        ADB_PAIR_TIMEOUT,
        create_command(&adb_path)
            .arg("pair")
            .arg(&ip)
            .arg(&code)
            .output(),
    )
    .await;

    let output = match output {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => {
            sink.emit(
                "scrcpy-log",
                format!("[SYSTEM] Pairing with {} timed out after 20s.", ip),
            );
            return Ok(json!({ "success": false, "message": "pairing timed out" }));
        }
    };

    let out_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err_text = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !out_text.is_empty() {
        sink.emit("scrcpy-log", format!("[ADB] {}", out_text));
    }
    if !err_text.is_empty() {
        sink.emit("scrcpy-log", format!("[ADB ERROR] {}", err_text));
    }

    let success = output.status.success()
        && (out_text.contains("Successfully paired") || err_text.contains("Successfully paired"));

    Ok(
        json!({ "success": success, "message": if out_text.is_empty() { err_text } else { out_text } }),
    )
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub async fn adb_shell(
    device: String,
    command: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("shell")
        .arg(&command)
        .output()
        .await;

    match output {
        Ok(o) => {
            json!({ "success": o.status.success(), "output": String::from_utf8_lossy(&o.stdout).to_string() })
        }
        Err(e) => json!({ "success": false, "message": e.to_string() }),
    }
}

/// Set by [`cancel_terminal_command`] so a long console command can be stopped
/// from the UI without waiting for the 120 s timeout.
static CONSOLE_CANCEL: AtomicBool = AtomicBool::new(false);

/// Cancels the in-flight console command. The UI has no cancel button yet, so
/// the function is kept for that wiring; the timeout and `kill_on_drop` still
/// ensure a command cannot run forever.
#[allow(dead_code)]
pub fn cancel_terminal_command() {
    CONSOLE_CANCEL.store(true, Ordering::SeqCst);
}

/// Reads a child pipe incrementally, keeping at most `cap` bytes but always
/// draining the pipe so the child never blocks on a full buffer.
async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: usize) -> String {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                if room > 0 {
                    let take = room.min(n);
                    kept.extend_from_slice(&buf[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    let mut text = String::from_utf8_lossy(&kept).to_string();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    text
}

enum ConsoleOutcome {
    Exited(std::process::ExitStatus),
    Cancelled,
    Failed(String),
}

pub async fn run_terminal_command(
    device: Option<String>,
    cmd: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    let mut parts = match split_args(&cmd) {
        Ok(parts) => parts,
        Err(e) => {
            return json!({ "success": false, "message": format!("Cannot parse command: {}", e) });
        }
    };
    if parts.is_empty() {
        return json!({ "success": false, "message": "No command provided" });
    }

    let first_part = parts[0].to_lowercase();
    let is_scrcpy = first_part == "scrcpy";
    let is_adb = first_part == "adb";

    let binary_name = if is_scrcpy { "scrcpy" } else { "adb" };
    let exe_path = get_binary_path(binary_name, custom_path);

    // If they explicitly typed "adb" or "scrcpy", remove it from arguments
    if is_adb || is_scrcpy {
        parts.remove(0);
    }

    let mut args = Vec::new();

    // Auto-inject device ID for ADB/Scrcpy commands if a device is active and not already specified
    let has_serial_flag =
        parts.contains(&"-s".to_string()) || parts.contains(&"--serial".to_string());

    if !has_serial_flag {
        if let Some(ref d) = device {
            if !d.is_empty() {
                // For ADB, don't inject for certain global commands
                let is_global_adb = binary_name == "adb"
                    && !parts.is_empty()
                    && (parts[0] == "devices" || parts[0] == "connect" || parts[0] == "pair");

                if !is_global_adb {
                    args.push("-s".to_string());
                    args.push(d.clone());
                }
            }
        }
    }

    for part in parts {
        args.push(part);
    }

    CONSOLE_CANCEL.store(false, Ordering::SeqCst);

    let mut child = match create_command(&exe_path)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return json!({ "success": false, "message": e.to_string() }),
    };

    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.start_kill();
        return json!({ "success": false, "message": "Failed to capture command output" });
    };

    let stdout_task = tokio::spawn(read_capped(stdout, MAX_CONSOLE_OUTPUT));
    let stderr_task = tokio::spawn(read_capped(stderr, MAX_CONSOLE_OUTPUT));

    let outcome = {
        let wait = async {
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => return ConsoleOutcome::Exited(status),
                    Ok(None) => {}
                    Err(e) => return ConsoleOutcome::Failed(e.to_string()),
                }
                if CONSOLE_CANCEL.load(Ordering::SeqCst) {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return ConsoleOutcome::Cancelled;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        match timeout(CONSOLE_TIMEOUT, wait).await {
            Ok(outcome) => outcome,
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                ConsoleOutcome::Cancelled
            }
        }
    };

    let stdout = timeout(Duration::from_secs(2), stdout_task)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let stderr = timeout(Duration::from_secs(2), stderr_task)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();

    match outcome {
        ConsoleOutcome::Exited(status) => json!({
            "success": status.success(),
            "binary": binary_name,
            "stdout": stdout,
            "stderr": stderr
        }),
        ConsoleOutcome::Cancelled => json!({
            "success": false,
            "binary": binary_name,
            "stdout": stdout,
            "stderr": stderr,
            "message": "Command cancelled or timed out"
        }),
        ConsoleOutcome::Failed(e) => json!({ "success": false, "message": e }),
    }
}

fn split_args(s: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars = s.chars();

    for c in chars {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    if in_quotes {
        return Err("Unclosed quotes".to_string());
    }
    Ok(args)
}

pub async fn push_file(
    device: String,
    file_path: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("push")
        .arg(&file_path)
        .arg("/sdcard/Download/")
        .output()
        .await;

    match output {
        Ok(o) => {
            if o.status.success() {
                json!({ "success": true, "message": "File pushed to Downloads" })
            } else {
                json!({ "success": false, "message": "Transfer failed" })
            }
        }
        Err(e) => json!({ "success": false, "message": e.to_string() }),
    }
}

pub async fn install_apk(
    device: String,
    file_path: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    let adb_path = get_binary_path("adb", custom_path);

    let output = create_command(&adb_path)
        .arg("-s")
        .arg(&device)
        .arg("install")
        .arg(&file_path)
        .output()
        .await;

    match output {
        Ok(o) => {
            let out_text = String::from_utf8_lossy(&o.stdout);
            let err_text = String::from_utf8_lossy(&o.stderr);

            if o.status.success() {
                json!({ "success": true, "message": out_text.trim() })
            } else {
                json!({ "success": false, "message": err_text.trim() })
            }
        }
        Err(e) => json!({ "success": false, "message": e.to_string() }),
    }
}

pub async fn kill_adb(
    sink: Arc<EventSink>,
    custom_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let adb_path = get_binary_path("adb", custom_path);
    sink.emit(
        "scrcpy-log",
        "[SYSTEM] Terminating ADB stack...".to_string(),
    );

    if let Ok(mut child) = create_command(&adb_path).arg("kill-server").spawn() {
        if timeout(Duration::from_secs(5), child.wait()).await.is_err() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }

    // Force kill adb process via OS specifics. Taskkill is resolved from
    // System32 so an attacker-controlled PATH cannot shadow it.
    let force = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            StdCommand::new(system32_tool("taskkill.exe"))
                .args(["/F", "/IM", "adb.exe", "/T"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        }
        #[cfg(not(target_os = "windows"))]
        {
            StdCommand::new("pkill")
                .arg("-f")
                .arg("adb")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        }
    });
    let _ = timeout(Duration::from_secs(5), force).await;

    sink.emit("scrcpy-log", "[SYSTEM] ADB Stack Terminated.".to_string());
    Ok(json!({ "success": true, "message": "ADB Stack Terminated" }))
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub async fn list_scrcpy_options(
    device: String,
    arg: String,
    custom_path: Option<String>,
) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);

    let mut command = create_command(&exe_path);
    command.arg("-s").arg(&device).arg(&arg);

    let output = command.output().await;

    match output {
        Ok(o) => {
            let out_text = String::from_utf8_lossy(&o.stdout);
            let err_text = String::from_utf8_lossy(&o.stderr); // scrcpy often prints lists to stderr
            let combined = format!("{}{}", out_text, err_text);
            json!({ "success": o.status.success(), "output": combined })
        }
        Err(e) => json!({ "success": false, "message": e.to_string() }),
    }
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
fn detect_host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
fn render_driver_label(driver: &str) -> &'static str {
    match driver {
        "direct3d" => "D3D11 (Direct3D)",
        "opengl" => "OpenGL",
        "opengles2" => "OpenGL ES 2",
        "opengles" => "OpenGL ES",
        "metal" => "Metal",
        "software" => "Software",
        "vulkan" => "Vulkan",
        _ => "Custom",
    }
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
fn is_driver_allowed_on_os(driver: &str, host_os: &str) -> bool {
    match host_os {
        "windows" => driver != "metal",
        "macos" => driver != "direct3d",
        "linux" => driver != "direct3d" && driver != "metal",
        _ => true,
    }
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
fn detect_render_drivers_from_help(help_output: &str) -> Vec<String> {
    let known = [
        "direct3d",
        "opengl",
        "opengles2",
        "opengles",
        "metal",
        "software",
        "vulkan",
    ];

    let lower = help_output.to_lowercase();
    let render_context = if let Some(start) = lower.find("--render-driver") {
        let end = (start + 1600).min(lower.len());
        lower[start..end].to_string()
    } else {
        String::new()
    };

    known
        .iter()
        .filter(|driver| {
            render_context.contains(&format!("\"{}\"", driver))
                || render_context.contains(&format!("'{}'", driver))
                || render_context.contains(*driver)
        })
        .map(|driver| (*driver).to_string())
        .collect()
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub async fn get_render_drivers(custom_path: Option<String>) -> serde_json::Value {
    let exe_path = get_binary_path("scrcpy", custom_path);
    let host_os = detect_host_os();

    let output = create_command(&exe_path).arg("--help").output().await;

    match output {
        Ok(o) => {
            let out_text = String::from_utf8_lossy(&o.stdout);
            let err_text = String::from_utf8_lossy(&o.stderr);
            let combined = format!("{}{}", out_text, err_text);
            let lower = combined.to_lowercase();

            let supports_render_driver = lower.contains("--render-driver");
            let supports_hwdec = lower.contains("--hwdec");
            if !supports_render_driver {
                return json!({
                    "success": true,
                    "hostOs": host_os,
                    "supportsRenderDriver": false,
                    "supportedDrivers": [],
                    "detectedDrivers": [],
                    "supportsHwdec": supports_hwdec,
                    "diagnostics": "scrcpy does not advertise --render-driver in --help output"
                });
            }

            let detected_drivers = detect_render_drivers_from_help(&combined);
            let supported_drivers: Vec<serde_json::Value> = detected_drivers
                .iter()
                .filter(|driver| is_driver_allowed_on_os(driver, host_os))
                .map(|driver| {
                    json!({
                        "id": driver,
                        "label": render_driver_label(driver)
                    })
                })
                .collect();

            json!({
                "success": o.status.success(),
                "hostOs": host_os,
                "supportsRenderDriver": true,
                "detectedDrivers": detected_drivers,
                "supportedDrivers": supported_drivers,
                "supportsHwdec": supports_hwdec,
                "diagnostics": "render drivers parsed from scrcpy --help"
            })
        }
        Err(e) => json!({
            "success": false,
            "hostOs": host_os,
            "supportsRenderDriver": false,
            "detectedDrivers": [],
            "supportedDrivers": [],
            "supportsHwdec": false,
            "message": e.to_string()
        }),
    }
}

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScrcpyConfig {
    pub device: String,
    pub session_mode: String,
    // ... other fields matching JS ...
    pub bitrate: Option<u32>,
    pub fps: Option<u32>,
    pub stay_awake: Option<bool>,
    pub turn_off: Option<bool>,
    pub audio_enabled: Option<bool>,
    pub audio_codec: Option<String>,
    pub always_on_top: Option<bool>,
    pub fullscreen: Option<bool>,
    pub borderless: Option<bool>,
    pub record: Option<bool>,
    pub record_path: Option<String>,
    pub scrcpy_path: Option<String>,
    pub otg_pure: Option<bool>,
    pub camera_facing: Option<String>,
    pub camera_id: Option<String>,
    pub codec: Option<String>,
    pub camera_ar: Option<String>,
    pub camera_high_speed: Option<bool>,
    pub vd_width: Option<u32>,
    pub vd_height: Option<u32>,
    pub vd_dpi: Option<u32>,
    pub rotation: Option<String>,
    pub res: Option<String>,
    pub hid_keyboard: Option<bool>,
    pub hid_mouse: Option<bool>,
    pub render_driver: Option<String>,
    /// Host-side video decoder (scrcpy v5+). "auto" is the default and sends
    /// no flag; the other values must be one of HWDEC_VALUES.
    pub hwdec: Option<String>,
    // v4 features
    pub flex_display: Option<bool>,
    pub camera_torch: Option<bool>,
    pub camera_zoom: Option<f32>,
    pub background_color: Option<String>,
    pub keep_active: Option<bool>,
    /// Force SDL renderer VSync on/off (anti-tearing). scrcpy 4.0 (SDL3)
    /// disables renderer VSync by default; this re-asserts it via the SDL hint.
    pub vsync: Option<bool>,
    /// Last known screen position of the scrcpy mirror window. Captured (client
    /// area top-left) while a windowed session runs and restored via
    /// --window-x / --window-y on the next launch, so the window reopens where
    /// the user left it, including when relaunching borderless.
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    // v4.1 features
    /// Ignore video encoder size constraints entirely (scrcpy v4.1+).
    /// Useful when the device reports incorrect encoder limits that prevent
    /// scrcpy from starting.
    pub ignore_video_encoder_constraints: Option<bool>,
}

const HWDEC_VALUES: &[&str] = &["disabled", "vaapi", "d3d11va", "videotoolbox"];

/// `--camera-id` takes a numeric id only (0, 1, ...). Anything else is a stale
/// or hand-edited label and must not become an arbitrary flag value.
fn parse_camera_id(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    trimmed.parse().ok()
}

/// The three values scrcpy accepts for `--camera-facing`, compared
/// case-insensitively.
fn resolve_camera_facing(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    ["back", "front", "external"]
        .into_iter()
        .find(|facing| trimmed.eq_ignore_ascii_case(facing))
}

/// Device names can contain `:`, spaces, path separators or arbitrary
/// punctuation (mDNS serials). The recording filename is restricted to a
/// conservative allowlist so it can never escape the chosen directory.
fn sanitize_record_component(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "device".to_string()
    } else {
        cleaned
    }
}

fn is_hwdec_allowed_on_os(value: &str) -> bool {
    match value {
        "disabled" => true,
        "vaapi" => cfg!(target_os = "linux"),
        "d3d11va" => cfg!(target_os = "windows"),
        "videotoolbox" => cfg!(target_os = "macos"),
        _ => false,
    }
}

/// Return the explicit `--hwdec` value to pass, or None for scrcpy's default
/// ("auto") / unknown values. Keeping an allowlist here means a corrupted or
/// hand-edited config can't inject an arbitrary flag value.
fn resolve_hwdec_flag(config: &ScrcpyConfig) -> Option<&str> {
    let requested = config.hwdec.as_deref()?.trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case("auto") {
        return None;
    }
    HWDEC_VALUES
        .iter()
        .find(|value| value.eq_ignore_ascii_case(requested) && is_hwdec_allowed_on_os(value))
        .copied()
}

fn resolve_audio_codec_flag<'a>(
    config: &'a ScrcpyConfig,
    audio_codec_override: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(c) = audio_codec_override {
        let trimmed = c.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    config.audio_codec.as_deref().and_then(|c| {
        let trimmed = c.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Parses `KEY=value` shell output (as printed by `xdotool ... --shell`) and
/// returns the integer value for `key`. Only compiled where it is used (the
/// Linux capture path, and the unit tests).
#[cfg(any(test, target_os = "linux"))]
fn parse_shell_var_int(text: &str, key: &str) -> Option<i32> {
    let prefix = format!("{}=", key);
    text.lines()
        .find(|l| l.starts_with(&prefix))
        .and_then(|l| l[prefix.len()..].trim().parse().ok())
}

/// Windows: finds the first visible, unowned top-level window belonging to
/// process `pid` (matches .NET's `MainWindowHandle` heuristic), i.e. scrcpy's
/// SDL mirror window rather than some owned tool/popup window. Shared by the
/// position-capture and recenter paths so both agree on which window they
/// mean.
#[cfg(target_os = "windows")]
fn find_window_by_pid(pid: u32) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows::core::BOOL;

    // State threaded through EnumWindows via LPARAM.
    struct Search {
        target_pid: u32,
        found: HWND,
    }

    // WNDENUMPROC: BOOL(1) keeps enumerating, BOOL(0) stops.
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        if GetWindow(hwnd, GW_OWNER)
            .map(|o| !o.0.is_null())
            .unwrap_or(false)
        {
            return BOOL(1);
        }
        let mut wnd_pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut wnd_pid as *mut u32));
        if wnd_pid == search.target_pid {
            search.found = hwnd;
            return BOOL(0); // found it -> stop enumeration
        }
        BOOL(1)
    }

    let mut search = Search {
        target_pid: pid,
        found: HWND::default(),
    };
    // EnumWindows returns Err both on real failure and when the callback
    // returns BOOL(0) (our intentional early stop), so ignore the Result and
    // read the captured handle instead.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut search as *mut Search as isize));
    }
    (!search.found.0.is_null()).then_some(search.found)
}

/// Queries the on-screen position of the scrcpy SDL window owned by process
/// `pid`, returning the client-area top-left in screen pixels. Returns `None`
/// silently whenever the window can't be read (not created yet, minimized,
/// tooling missing, permission denied); every failure mode is non-fatal.
///
/// The reference frame differs per platform and is reconciled afterwards by
/// [`resolve_window_pos_to_persist`], rather than by hardcoding decoration
/// sizes:
/// - Windows: Win32 `ClientToScreen` -> client origin (matches scrcpy/SDL).
/// - Linux/X11 (+XWayland): `xdotool` -> window origin (usually the client area).
/// - macOS: `osascript`/System Events -> frame origin (title-bar top).
fn try_capture_window_pos(pid: u32) -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        // Native Win32: this replaces spawning PowerShell + `Add-Type` (a C#
        // JIT) on every poll, it is a couple of direct syscalls with no child
        // process.
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};

        if let Some(hwnd) = find_window_by_pid(pid) {
            // A minimized window reports an off-screen sentinel origin
            // (~ -32000,-32000) and a maximized one the monitor edge; neither is
            // the windowed position we want, so skip them and keep the last good
            // sample.
            let special = unsafe { IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() };
            if !special {
                // ClientToScreen maps the client-area origin (0,0) into screen
                // pixels; GetWindowRect would return the outer frame and creep
                // the window up by the title-bar height on every relaunch.
                let mut pt = POINT { x: 0, y: 0 };
                if unsafe { ClientToScreen(hwnd, &mut pt) }.as_bool() {
                    return Some((pt.x, pt.y));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let pid_str = pid.to_string();
        let output = StdCommand::new("xdotool")
            .args(["search", "--pid", &pid_str, "getwindowgeometry", "--shell"])
            .output()
            .ok()?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let x = parse_shell_var_int(&text, "X")?;
            let y = parse_shell_var_int(&text, "Y")?;
            return Some((x, y));
        }
    }
    #[cfg(target_os = "macos")]
    {
        // System Events needs Accessibility permission for the app (one-time
        // grant in System Settings). The try block swallows every error (no
        // window yet, permission denied, ...) so this stays a silent no-op
        // until the permission is granted.
        let script = format!(
            concat!(
                "tell application \"System Events\"\n",
                "try\n",
                "set proc to first process whose unix id is {}\n",
                "set pos to position of first window of proc\n",
                "((item 1 of pos) as string) & \" \" & ((item 2 of pos) as string)\n",
                "end try\n",
                "end tell"
            ),
            pid
        );
        let output = StdCommand::new("osascript")
            .args(["-e", &script])
            .output()
            .ok()?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut parts = text.split_whitespace();
            let x: i32 = parts.next()?.parse().ok()?;
            let y: i32 = parts.next()?.parse().ok()?;
            return Some((x, y));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let _ = pid;
    None
}

/// Moves the scrcpy mirror window owned by `pid` to the centre of the primary
/// screen and brings it to the front. A safety net for a persisted position
/// that no longer lands on any connected monitor (e.g. it was saved while a
/// second monitor was attached, which has since been unplugged) -- the window
/// can reopen fully off-screen, with no title bar to drag back if borderless.
/// Best-effort and silent: does nothing if the window can't be found, is
/// already maximized (it fills its monitor; there is nothing to recover), or
/// the move fails.
fn recenter_window(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, GetWindowRect, IsIconic, IsZoomed, SM_CXSCREEN, SM_CYSCREEN,
            SW_RESTORE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow,
        };

        let Some(hwnd) = find_window_by_pid(pid) else {
            return;
        };
        if unsafe { IsZoomed(hwnd) }.as_bool() {
            return;
        }
        if unsafe { IsIconic(hwnd) }.as_bool() {
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let (screen_w, screen_h) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                (screen_w - w) / 2,
                (screen_h - h) / 2,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let _ = unsafe { SetForegroundWindow(hwnd) };
    }
    #[cfg(target_os = "linux")]
    {
        let pid_str = pid.to_string();
        let Ok(search_out) = StdCommand::new("xdotool")
            .args(["search", "--pid", &pid_str])
            .output()
        else {
            return;
        };
        let Some(window_id) = String::from_utf8_lossy(&search_out.stdout)
            .lines()
            .next()
            .map(str::to_string)
        else {
            return;
        };

        let Ok(geom_out) = StdCommand::new("xdotool")
            .args(["getwindowgeometry", "--shell", &window_id])
            .output()
        else {
            return;
        };
        let geom_text = String::from_utf8_lossy(&geom_out.stdout);
        let (Some(w), Some(h)) = (
            parse_shell_var_int(&geom_text, "WIDTH"),
            parse_shell_var_int(&geom_text, "HEIGHT"),
        ) else {
            return;
        };

        let Ok(display_out) = StdCommand::new("xdotool")
            .arg("getdisplaygeometry")
            .output()
        else {
            return;
        };
        let display_text = String::from_utf8_lossy(&display_out.stdout);
        let mut parts = display_text.split_whitespace();
        let (Some(Ok(screen_w)), Some(Ok(screen_h))) = (
            parts.next().map(str::parse::<i32>),
            parts.next().map(str::parse::<i32>),
        ) else {
            return;
        };

        let x = ((screen_w - w).max(0) / 2).to_string();
        let y = ((screen_h - h).max(0) / 2).to_string();
        let _ = StdCommand::new("xdotool")
            .args(["windowmove", &window_id, &x, &y])
            .output();
        let _ = StdCommand::new("xdotool")
            .args(["windowactivate", &window_id])
            .output();
    }
    #[cfg(target_os = "macos")]
    {
        // Same Accessibility-permission requirement as the position capture
        // above; a silent no-op until it is granted.
        let script = format!(
            concat!(
                "tell application \"System Events\"\n",
                "try\n",
                "set proc to first process whose unix id is {}\n",
                "set win to first window of proc\n",
                "set {{winW, winH}} to size of win\n",
                "tell application \"Finder\" to set screenBounds to bounds of window of desktop\n",
                "set screenW to (item 3 of screenBounds)\n",
                "set screenH to (item 4 of screenBounds)\n",
                "set position of win to {{((screenW - winW) / 2), ((screenH - winH) / 2)}}\n",
                "set frontmost of proc to true\n",
                "end try\n",
                "end tell"
            ),
            pid
        );
        let _ = StdCommand::new("osascript").args(["-e", &script]).output();
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let _ = pid;
}

/// Re-centres the running mirror window for `device` on the primary screen.
/// Bound to a global OS shortcut (Ctrl+Alt+Shift+C, see `shortcuts.rs`) so a
/// window whose saved position ends up off-screen (see [`recenter_window`])
/// can be recovered without being able to see or focus it directly. A no-op
/// if `device` has no tracked process (e.g. nothing is running).
pub fn recenter_device(state: &ScrcpyState, device: &str) {
    // Live children are owned by the generation registry now; keep the
    // `ScrcpyState` map as a fallback for any child inserted by older wiring.
    let pid = registry()
        .sessions
        .lock()
        .unwrap()
        .get(device)
        .and_then(|session| session.pid)
        .or_else(|| {
            state
                .processes
                .lock()
                .unwrap()
                .get(device)
                .and_then(|c| c.id())
        });
    if let Some(pid) = pid {
        recenter_window(pid);
    }
}

pub fn recenter_scrcpy_window(state: Arc<ScrcpyState>, device: String) -> Result<(), String> {
    recenter_device(&state, &device);
    Ok(())
}

/// Records which device the GUI currently considers selected, so the global
/// recentre shortcut (which fires with no window necessarily focused) knows
/// which mirror window to act on.
pub fn set_active_device(state: Arc<ScrcpyState>, device: Option<String>) -> Result<(), String> {
    *state.active_device.lock().unwrap() = device;
    Ok(())
}

/// Converts a raw captured position into the value to persist as
/// --window-x/--window-y so the window round-trips exactly on the next launch.
///
/// [`try_capture_window_pos`] reports coordinates in each OS tool's native
/// frame, which may differ from scrcpy's own (SDL) reference by the window
/// decorations, a difference that varies by platform, window manager, theme
/// and SDL version. Instead of hardcoding decoration sizes we learn the offset
/// live: when we launched at an explicit position (`requested`), the delta
/// between it and the *first* sample (`first`, taken before the user can move
/// the window) is the decoration offset; subtracting it from the last sample
/// yields a value that round-trips. Offsets larger than any plausible window
/// decoration are ignored (the window was moved before the first sample, or was
/// placed by the WM), falling back to the raw `last` position.
fn resolve_window_pos_to_persist(
    requested: Option<(i32, i32)>,
    first: Option<(i32, i32)>,
    last: (i32, i32),
) -> (i32, i32) {
    if let (Some((rx, ry)), Some((fx, fy))) = (requested, first) {
        let (dx, dy) = (fx - rx, fy - ry);
        if dx.abs() <= 80 && dy.abs() <= 120 {
            return (last.0 - dx, last.1 - dy);
        }
    }
    last
}

/// Emits the resolved window position (when one was captured) followed by the
/// session-stopped status. Shared by every monitor-loop exit path.
fn emit_window_pos_and_stop(
    sink: &EventSink,
    device: &str,
    requested: Option<(i32, i32)>,
    first: Option<(i32, i32)>,
    last: Option<(i32, i32)>,
) {
    if let Some(last) = last {
        let (x, y) = resolve_window_pos_to_persist(requested, first, last);
        sink.emit(
            "scrcpy-window-pos",
            json!({ "device": device, "x": x, "y": y }),
        );
    }
    sink.emit(
        "scrcpy-status",
        json!({ "device": device, "running": false }),
    );
}

fn build_scrcpy_args(
    config: &ScrcpyConfig,
    video_dir_fallback: Option<String>,
    audio_codec_override: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();

    // Construct arguments based on config
    if !config.device.is_empty() {
        args.push("-s".to_string());
        args.push(config.device.clone());
    }

    let codec = config.codec.as_deref().unwrap_or("h264");
    args.push(format!("--video-codec={}", codec));

    let otg_pure = config.otg_pure.unwrap_or(false);
    let hid_keyboard = config.hid_keyboard.unwrap_or(false);
    let hid_mouse = config.hid_mouse.unwrap_or(false);

    if config.session_mode == "mirror" && (hid_keyboard || hid_mouse) && otg_pure {
        if config.device.contains('.') || config.device.contains(':') {
            args.push("--no-video".to_string());
            args.push("--no-audio".to_string());
            args.push("--keyboard=uhid".to_string());
            args.push("--mouse=uhid".to_string());
        } else {
            args.push("--otg".to_string());
        }
    } else {
        if hid_keyboard {
            args.push("--keyboard=uhid".to_string());
        }
        if hid_mouse {
            args.push("--mouse=uhid".to_string());
        }

        if let Some(render_driver) = &config.render_driver {
            let selected_driver = render_driver.trim();
            if !selected_driver.is_empty() && selected_driver != "auto" {
                args.push("--render-driver".to_string());
                args.push(selected_driver.to_string());
            }
        }

        if let Some(hwdec) = resolve_hwdec_flag(config) {
            args.push(format!("--hwdec={}", hwdec));
        }

        if let Some(bitrate) = config.bitrate {
            args.push("--video-bit-rate".to_string());
            args.push(format!("{}M", bitrate));
        }

        let audio_enabled = config.audio_enabled.unwrap_or(true);
        if !audio_enabled {
            args.push("--no-audio".to_string());
        }
        if audio_enabled {
            if let Some(codec) = resolve_audio_codec_flag(config, audio_codec_override) {
                args.push(format!("--audio-codec={}", codec));
            }
        }
        if let Some(aot) = config.always_on_top {
            if aot {
                args.push("--always-on-top".to_string());
            }
        }
        if let Some(fs) = config.fullscreen {
            if fs {
                args.push("--fullscreen".to_string());
            }
        }
        if let Some(bl) = config.borderless {
            if bl {
                args.push("--window-borderless".to_string());
            }
        }

        // Restore the last saved window position (client-area origin), for
        // mirror sessions only. Skipped in fullscreen (scrcpy ignores it) and
        // for camera/desktop, whose windows must not share, or overwrite, the
        // mirror window's remembered position.
        if config.session_mode == "mirror" && !config.fullscreen.unwrap_or(false) {
            if let Some(wx) = config.window_x {
                args.push(format!("--window-x={}", wx));
            }
            if let Some(wy) = config.window_y {
                args.push(format!("--window-y={}", wy));
            }
        }

        if let Some(rot) = &config.rotation {
            if rot != "0" {
                args.push("--orientation".to_string());
                args.push(rot.clone());
            }
        }

        let can_control = config.session_mode != "camera";
        if can_control {
            if let Some(sa) = config.stay_awake {
                if sa {
                    args.push("--stay-awake".to_string());
                }
            }
            if let Some(ka) = config.keep_active {
                if ka {
                    args.push("--keep-active".to_string());
                }
            }
            if let Some(to) = config.turn_off {
                if to {
                    args.push("--turn-screen-off".to_string());
                    args.push("--no-power-on".to_string());
                }
            }
        }

        if config.session_mode == "camera" {
            args.push("--video-source=camera".to_string());
            // Only a numeric camera id is a real id. Older configs stored the
            // facing label ("back"/"front"/"external") in camera_id; those are
            // ignored in favour of camera_facing instead of being pasted into
            // the flag.
            if let Some(id) = config.camera_id.as_deref().and_then(parse_camera_id) {
                args.push(format!("--camera-id={}", id));
            } else if let Some(facing) = config
                .camera_facing
                .as_deref()
                .and_then(resolve_camera_facing)
            {
                args.push(format!("--camera-facing={}", facing));
            }

            // Resolution logic: Map res (e.g., "1920") to standard camera sizes (e.g., "1920x1080")
            if let Some(res) = &config.res {
                if res != "0" {
                    let camera_size = match res.as_str() {
                        "3840" => "3840x2160",
                        "2560" => "2560x1440",
                        "1920" => "1920x1080",
                        "1600" => "1600x900",
                        "1280" => "1280x720",
                        "1024" => "1024x576",
                        "800" => "800x480",
                        _ => "1920x1080",
                    };
                    args.push(format!("--camera-size={}", camera_size));
                } else {
                    // Safe fallback default to avoid camera configuration error on non-standard sensor photo sizes (e.g. 4080x3060)
                    args.push("--camera-size=1920x1080".to_string());
                }
            } else {
                args.push("--camera-size=1920x1080".to_string());
            }

            if let Some(ar) = &config.camera_ar {
                if ar != "0" {
                    args.push(format!("--camera-ar={}", ar));
                }
            }
            if let Some(chs) = config.camera_high_speed {
                if chs {
                    args.push("--camera-high-speed".to_string());
                }
            }
            // v4: camera torch and zoom
            if let Some(true) = config.camera_torch {
                args.push("--camera-torch".to_string());
            }
            if let Some(zoom) = config.camera_zoom {
                if zoom > 1.005 {
                    // avoid floating point noise at 1.0
                    args.push(format!("--camera-zoom={:.1}", zoom));
                }
            }
            // fps handled generically below
        } else if config.session_mode == "desktop" {
            let w = config.vd_width.unwrap_or(1920);
            let h = config.vd_height.unwrap_or(1080);
            let dpi = config.vd_dpi.unwrap_or(420);
            args.push(format!("--new-display={}x{}/{}", w, h, dpi));
            args.push("--video-buffer=100".to_string());
            // v4: flex display (resize virtual display with window)
            if let Some(true) = config.flex_display {
                args.push("--flex-display".to_string());
            }
        }

        if let Some(fps) = config.fps {
            if fps > 0 {
                if config.session_mode == "camera" {
                    args.push("--camera-fps".to_string());
                } else {
                    args.push("--max-fps".to_string());
                }
                args.push(fps.to_string());
            }
        } else if config.session_mode == "camera" && config.camera_high_speed.unwrap_or(false) {
            args.push("--camera-fps".to_string());
            args.push("60".to_string());
        }

        // Shared resolution logic (applies to mirror and desktop)
        if config.session_mode != "camera" {
            if let Some(res) = &config.res {
                if res != "0" {
                    args.push("--max-size".to_string());
                    args.push(res.clone());
                }
            }
        }

        if let Some(rec) = config.record {
            if rec {
                let mut path = config.record_path.clone().unwrap_or_default();

                // If path is empty, try to get Video dir fallback
                if path.trim().is_empty() {
                    path = video_dir_fallback.unwrap_or_else(|| ".".to_string());
                }

                let filename = format!(
                    "scrcpy_{}_{}.mkv",
                    sanitize_record_component(&config.device),
                    chrono::Local::now().format("%Y%m%d_%H%M%S")
                );
                let full_path = std::path::Path::new(&path).join(filename);
                args.push(format!("--record={}", full_path.to_string_lossy()));
            }
        }

        // v4: background color (all modes)
        if let Some(ref color) = config.background_color {
            let trimmed = color.trim();
            if !trimmed.is_empty() {
                args.push(format!("--background-color={}", trimmed));
            }
        }

        // v4.1: ignore video encoder size constraints
        if let Some(true) = config.ignore_video_encoder_constraints {
            args.push("--ignore-video-encoder-constraints".to_string());
        }
    }

    args
}

/// Incrementally reads one scrcpy pipe. Lines are decoded lossily: a single
/// non-UTF-8 byte (device output, locale, emoji) must not stop the reader.
async fn read_scrcpy_logs<R>(reader: R, sink: EventSink, audio_flag: Option<Arc<AtomicBool>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            read = reader.read_until(b'\n', &mut buf) => {
                match read {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf)
                            .trim_end_matches(['\r', '\n'])
                            .to_string();
                        if let Some(flag) = &audio_flag {
                            if is_audio_codec_error(&line) {
                                flag.store(true, Ordering::SeqCst);
                            }
                        }
                        if !line.is_empty() {
                            batch.push(line);
                        }
                        buf.clear();
                        if batch.len() >= 50 {
                            sink.emit("scrcpy-log", batch.join("\n"));
                            batch.clear();
                        }
                    }
                    Err(_) => break,
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    sink.emit("scrcpy-log", batch.join("\n"));
                    batch.clear();
                }
            }
        }
    }
    if !batch.is_empty() {
        sink.emit("scrcpy-log", batch.join("\n"));
    }
}

async fn spawn_scrcpy_streams(
    sink: &EventSink,
    exe_path: &str,
    adb_exe_path: &str,
    server_path: Option<&str>,
    args: &[String],
    vsync: bool,
) -> Result<(tokio::process::Child, Arc<AtomicBool>, Vec<JoinHandle<()>>), String> {
    let command_str = format!("> scrcpy {}", args.join(" "));
    sink.emit("scrcpy-log", command_str);

    let mut command = create_command(exe_path);
    command.args(args);
    command.env("ADB", adb_exe_path);
    if let Some(sp) = server_path {
        if Path::new(sp).exists() {
            command.env("SCRCPY_SERVER_PATH", sp);
        }
    }
    // scrcpy 4.0 migrated from SDL2 to SDL3, which leaves the renderer's VSync
    // disabled by default and reintroduced screen tearing for users who had
    // none on scrcpy 3.x (SDL2). Re-assert VSync through SDL's hint env var so
    // the renderer syncs to the display refresh; "0" lets the user opt out
    // (slightly lower input latency) via the in-app toggle.
    command.env("SDL_RENDER_VSYNC", if vsync { "1" } else { "0" });
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| e.to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture scrcpy stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture scrcpy stderr".to_string())?;

    let audio_error_flag = Arc::new(AtomicBool::new(false));
    let stdout_reader = tokio::spawn(read_scrcpy_logs(stdout, sink.clone(), None));
    let stderr_reader = tokio::spawn(read_scrcpy_logs(
        stderr,
        sink.clone(),
        Some(audio_error_flag.clone()),
    ));

    Ok((child, audio_error_flag, vec![stdout_reader, stderr_reader]))
}

/// Bounded wait for the log readers to reach EOF after the child exited.
async fn join_readers(readers: Vec<JoinHandle<()>>) {
    for handle in readers {
        let _ = timeout(Duration::from_secs(2), handle).await;
    }
}

/// A running (or respawning) scrcpy session. `child` is `None` only while the
/// audio-codec fallback sleeps; keeping the entry in the map during that gap
/// means a stop can still cancel the retry instead of racing it.
struct Session {
    generation: u64,
    pid: Option<u32>,
    mirror_window: bool,
    child: Option<tokio::process::Child>,
    readers: Vec<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    sink: EventSink,
    requested_pos: Option<(i32, i32)>,
    pos: Arc<Mutex<WindowPosTrack>>,
}

#[derive(Clone, Copy, Default)]
struct WindowPosTrack {
    first: Option<(i32, i32)>,
    last: Option<(i32, i32)>,
}

/// Generation-scoped process registry. `commands.rs` is the only owner of live
/// scrcpy children; `ScrcpyState::processes` is kept for the ported helper
/// signatures but is no longer authoritative.
#[derive(Default)]
struct SessionRegistry {
    counter: AtomicU64,
    generations: Mutex<HashMap<String, u64>>,
    sessions: Mutex<HashMap<String, Session>>,
    shutdown: AtomicBool,
}

fn registry() -> &'static SessionRegistry {
    static REGISTRY: OnceLock<SessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(SessionRegistry::default)
}

impl SessionRegistry {
    fn bump_generation(&self, device: &str) -> u64 {
        // Serialize with session insertion: a start must not be able to pass
        // its generation check and then insert after a concurrent stop.
        let _sessions = self.sessions.lock().unwrap();
        let generation = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.generations
            .lock()
            .unwrap()
            .insert(device.to_string(), generation);
        generation
    }

    fn current_generation(&self, device: &str) -> Option<u64> {
        self.generations.lock().unwrap().get(device).copied()
    }

    fn is_current(&self, device: &str, generation: u64) -> bool {
        self.current_generation(device) == Some(generation)
    }

    fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

/// Increments and returns the current session generation for `device`.
pub fn next_generation(device: &str) -> u64 {
    registry().bump_generation(device)
}

/// Whether a scrcpy session currently owns `device` (including a fallback
/// respawn gap). Replaces the UI's separate `running` table.
pub fn is_running(device: &str) -> bool {
    let registry = registry();
    let sessions = registry.sessions.lock().unwrap();
    match sessions.get(device) {
        Some(session) => registry.current_generation(device) == Some(session.generation),
        None => false,
    }
}

/// True when `pid` belongs to a live scrcpy session managed here. The borderless
/// drag hook uses this instead of the old (now empty) `ScrcpyState::processes`.
pub fn is_our_pid(pid: u32) -> bool {
    let sessions = registry().sessions.lock().unwrap();
    sessions.values().any(|session| session.pid == Some(pid))
}

/// True when automatic audio fallback should retry. A normal exit (status 0)
/// must never be retried just because stderr mentioned audio.
fn should_audio_fallback(
    auto_fallback: bool,
    success: bool,
    audio_failed: bool,
    chain_index: usize,
) -> bool {
    auto_fallback && !success && audio_failed && chain_index < AUDIO_FALLBACK_CHAIN.len()
}

/// Windows: force-kill a process tree through the System32 `taskkill`.
/// Returns `Err` when taskkill itself failed (the process may still be gone;
/// callers confirm with `Child::wait`).
async fn terminate_process_tree(pid: u32) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            let output = StdCommand::new(system32_tool("taskkill.exe"))
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("taskkill failed to start: {}", e))?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "taskkill exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let output = StdCommand::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .output()
                .map_err(|e| format!("kill failed to start: {}", e))?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "kill exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        }
    })
    .await
    .map_err(|e| format!("terminate task failed: {}", e))?
}

/// Synchronous kill used from `shutdown_all` (no runtime required).
fn kill_process_tree_sync(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = StdCommand::new(system32_tool("taskkill.exe"))
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = StdCommand::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

async fn capture_window_pos(pid: u32) -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        try_capture_window_pos(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        tokio::task::spawn_blocking(move || try_capture_window_pos(pid))
            .await
            .ok()
            .flatten()
    }
}

fn emit_session_stopped(session: &Session, device: &str, fresh: Option<(i32, i32)>) {
    let track = *session.pos.lock().unwrap();
    let last = fresh.or(track.last);
    emit_window_pos_and_stop(
        &session.sink,
        device,
        session.requested_pos,
        track.first,
        last,
    );
}

/// A monitor/stop may only publish the final `running:false` status while no
/// newer session owns the device; otherwise a stop/start that happened while
/// the old session was being reaped would overwrite the newer `running:true`.
fn emit_session_stopped_if_owned(session: &Session, device: &str, fresh: Option<(i32, i32)>) {
    let registry = registry();
    {
        let sessions = registry.sessions.lock().unwrap();
        if registry.shutdown_requested() || sessions.contains_key(device) {
            return;
        }
    }
    emit_session_stopped(session, device, fresh);
}

/// Terminates a session that was replaced by a newer start. The old monitor
/// sees its entry disappear and exits without emitting a second status.
async fn terminate_previous_session(mut session: Session) {
    session.cancel.store(true, Ordering::SeqCst);
    if let Some(pid) = session.pid {
        let _ = terminate_process_tree(pid).await;
    }
    if let Some(mut child) = session.child.take() {
        if timeout(Duration::from_secs(3), child.wait()).await.is_err() {
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
    join_readers(std::mem::take(&mut session.readers)).await;
}

/// Global shutdown for `on_quit`: drains and kills every child and makes any
/// in-flight start refuse to insert a new process afterwards.
pub fn shutdown_all() {
    let registry = registry();
    registry.shutdown.store(true, Ordering::SeqCst);
    let sessions: Vec<(String, Session)> = {
        let mut sessions = registry.sessions.lock().unwrap();
        sessions.drain().collect()
    };
    for (device, mut session) in sessions {
        registry.bump_generation(&device);
        session.cancel.store(true, Ordering::SeqCst);
        session.sink.emit(
            "scrcpy-status",
            json!({ "device": device, "running": false }),
        );
        if let Some(pid) = session.pid {
            kill_process_tree_sync(pid);
        }
        if let Some(mut child) = session.child.take() {
            let _ = child.start_kill();
        }
    }
}

pub async fn start_scrcpy(
    device: String,
    generation: u64,
    config: ScrcpyConfig,
    sink: EventSink,
) -> Result<(), String> {
    let registry = registry();
    // A stale generation (stopped, superseded, or quitting) must never spawn.
    if registry.shutdown_requested() || !registry.is_current(&device, generation) {
        return Ok(());
    }

    let video_dir = super::video_dir().map(|p| p.to_string_lossy().to_string());
    let exe_path = get_binary_path("scrcpy", config.scrcpy_path.clone());

    // Log the session details for the user
    let mode_label = match config.session_mode.as_str() {
        "camera" => "Camera Mode",
        "desktop" => "Desktop Mode",
        _ => "Screen Mirroring",
    };

    let res_label = config
        .res
        .as_ref()
        .map(|r| if r == "0" { "Original" } else { r })
        .unwrap_or("Original");
    let bitrate_label = format!("{}Mbps", config.bitrate.unwrap_or(8));
    let fps_label = format!("{}fps", config.fps.unwrap_or(60));

    sink.emit(
        "scrcpy-log",
        format!("[SYSTEM] Starting {} session...", mode_label),
    );
    sink.emit(
        "scrcpy-log",
        format!(
            "[SYSTEM] Target: {} | Config: {} @ {}, {}",
            config.device, res_label, bitrate_label, fps_label
        ),
    );

    if config.record.unwrap_or(false) {
        let path = config
            .record_path
            .as_ref()
            .map(|p| if p.is_empty() { "Videos" } else { p })
            .unwrap_or("Videos");
        sink.emit(
            "scrcpy-log",
            format!("[SYSTEM] Recording enabled -> output to {}", path),
        );
    }

    let adb_exe_path = get_binary_path("adb", config.scrcpy_path.clone());
    let server_path = if !exe_path.is_empty() && exe_path != "scrcpy" {
        Path::new(&exe_path)
            .parent()
            .map(|p| p.join("scrcpy-server").to_string_lossy().to_string())
    } else {
        None
    };

    sink.emit("scrcpy-log", format!("[SYSTEM] Using scrcpy: {}", exe_path));
    sink.emit(
        "scrcpy-log",
        format!("[SYSTEM] Using adb: {}", adb_exe_path),
    );

    // Decide whether automatic audio codec fallback should kick in on errors.
    let audio_enabled = config.audio_enabled.unwrap_or(true);
    let audio_codec_mode = config.audio_codec.as_deref().unwrap_or("auto").trim();
    let should_auto_fallback = audio_enabled
        && (audio_codec_mode.is_empty() || audio_codec_mode.eq_ignore_ascii_case("auto"));

    // First attempt: honour the user's audio codec choice. In "auto" mode this
    // launches without --audio-codec, matching the previous default behaviour.
    let initial_args = build_scrcpy_args(&config, video_dir.clone(), None);

    // Any earlier session for this device is superseded (or a duplicate start
    // raced us): terminate it before spawning. The registry insert below also
    // refuses to overwrite an existing entry, so a live child can never be
    // orphaned by the map.
    let previous = registry.sessions.lock().unwrap().remove(&device);
    if let Some(previous) = previous {
        terminate_previous_session(previous).await;
    }

    if registry.shutdown_requested() || !registry.is_current(&device, generation) {
        return Ok(());
    }

    let (child, audio_error_flag, readers) = match spawn_scrcpy_streams(
        &sink,
        &exe_path,
        &adb_exe_path,
        server_path.as_deref(),
        &initial_args,
        config.vsync.unwrap_or(true),
    )
    .await
    {
        Ok(spawned) => spawned,
        Err(e) => {
            sink.emit(
                "scrcpy-status",
                json!({ "device": device, "running": false }),
            );
            return Err(e);
        }
    };

    // Check the generation again after the spawn: a stop that arrived while
    // the process was starting must win over this session.
    let pid = child.id();
    let requested_pos = match (config.window_x, config.window_y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    // Only track and persist the position for windowed mirror sessions. A
    // fullscreen session has no meaningful position, and camera/desktop
    // sessions must not overwrite the mirror window's remembered position.
    let mirror_window = config.session_mode == "mirror" && !config.fullscreen.unwrap_or(false);
    let pos = Arc::new(Mutex::new(WindowPosTrack::default()));

    let mut child_opt = Some(child);
    let mut readers_opt = Some(readers);
    let inserted = {
        let mut sessions = registry.sessions.lock().unwrap();
        if registry.shutdown_requested()
            || !registry.is_current(&device, generation)
            || sessions.contains_key(&device)
        {
            false
        } else {
            sessions.insert(
                device.clone(),
                Session {
                    generation,
                    pid,
                    mirror_window,
                    child: child_opt.take(),
                    readers: readers_opt.take().unwrap_or_default(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    sink: sink.clone(),
                    requested_pos,
                    pos: pos.clone(),
                },
            );
            true
        }
    };

    if !inserted {
        // Quit/stop/supersede won the race; leave no freshly spawned orphan.
        if let Some(mut child) = child_opt.take() {
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(2), child.wait()).await;
        }
        join_readers(readers_opt.take().unwrap_or_default()).await;
        return Ok(());
    }

    sink.emit(
        "scrcpy-status",
        json!({ "device": device, "running": true }),
    );

    tokio::spawn(monitor_session(
        device,
        generation,
        sink,
        config,
        exe_path,
        adb_exe_path,
        server_path,
        video_dir,
        should_auto_fallback,
        audio_error_flag,
        0,
    ));

    Ok(())
}

/// Removes a fallback placeholder only if it is still ours. Returns the
/// removed session so the caller can emit the final stopped status.
fn take_placeholder(device: &str, generation: u64) -> Option<Session> {
    let registry = registry();
    let mut sessions = registry.sessions.lock().unwrap();
    let placeholder = sessions
        .get(device)
        .is_some_and(|slot| slot.generation == generation && slot.child.is_none());
    if placeholder {
        sessions.remove(device)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
async fn monitor_session(
    device: String,
    generation: u64,
    sink: EventSink,
    config: ScrcpyConfig,
    exe_path: String,
    adb_exe_path: String,
    server_path: Option<String>,
    video_dir: Option<String>,
    should_auto_fallback: bool,
    mut audio_flag: Arc<AtomicBool>,
    mut chain_index: usize,
) {
    let registry = registry();

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Sample the live window position on every tick: SDL destroys the
        // window before the process exits, so a periodic sample is the only
        // way to catch a move made just before the user closes the window.
        let sample = {
            let sessions = registry.sessions.lock().unwrap();
            match sessions.get(&device) {
                Some(session) if session.generation == generation => {
                    if session.mirror_window {
                        session.pid.map(|pid| (pid, session.pos.clone()))
                    } else {
                        None
                    }
                }
                _ => return,
            }
        };
        if let Some((pid, pos)) = sample {
            if let Some(found) = capture_window_pos(pid).await {
                let mut track = pos.lock().unwrap();
                if track.first.is_none() {
                    track.first = Some(found);
                }
                track.last = Some(found);
            }
        }

        let exit = {
            let mut sessions = registry.sessions.lock().unwrap();
            let Some(session) = sessions.get_mut(&device) else {
                return;
            };
            if session.generation != generation {
                return;
            }
            match session.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => Some(Ok(status)),
                    Ok(None) => None,
                    Err(e) => Some(Err(e.to_string())),
                },
                // stop_scrcpy detached the child and owns the final status.
                None => return,
            }
        };
        let Some(exit) = exit else { continue };

        // Claim the entry before touching anything else: only one monitor can
        // finalise a generation, and a stop that already removed it wins.
        let Some(mut session) = ({
            let mut sessions = registry.sessions.lock().unwrap();
            let ours = sessions
                .get(&device)
                .is_some_and(|session| session.generation == generation);
            if ours { sessions.remove(&device) } else { None }
        }) else {
            return;
        };

        join_readers(std::mem::take(&mut session.readers)).await;

        let status = match exit {
            Ok(status) => status,
            Err(e) => {
                sink.emit(
                    "scrcpy-log",
                    format!("[SYSTEM] Error waiting for scrcpy: {}", e),
                );
                emit_session_stopped_if_owned(&session, &device, None);
                return;
            }
        };

        let success = status.success();
        sink.emit(
            "scrcpy-log",
            format!("[SYSTEM] Scrcpy process exited with status: {}", status),
        );

        let audio_failed = audio_flag.load(Ordering::SeqCst);

        if should_audio_fallback(should_auto_fallback, success, audio_failed, chain_index) {
            let next_codec = AUDIO_FALLBACK_CHAIN[chain_index];
            sink.emit(
                "scrcpy-log",
                format!(
                    "[SYSTEM] Default audio codec failed, retrying with {}...",
                    next_codec.to_uppercase()
                ),
            );

            // Keep a placeholder in the registry during the 250 ms flush window:
            // a stop arriving now can still invalidate the retry.
            let placed = {
                let mut sessions = registry.sessions.lock().unwrap();
                if !registry.shutdown_requested()
                    && registry.is_current(&device, generation)
                    && !sessions.contains_key(&device)
                {
                    sessions.insert(
                        device.clone(),
                        Session {
                            generation,
                            pid: None,
                            mirror_window: session.mirror_window,
                            child: None,
                            readers: Vec::new(),
                            cancel: session.cancel.clone(),
                            sink: sink.clone(),
                            requested_pos: session.requested_pos,
                            pos: Arc::new(Mutex::new(WindowPosTrack::default())),
                        },
                    );
                    true
                } else {
                    false
                }
            };
            if !placed {
                // A stop or a newer start claimed the slot; it owns the status.
                emit_session_stopped_if_owned(&session, &device, None);
                return;
            }

            tokio::time::sleep(AUDIO_FALLBACK_DELAY).await;

            let allowed = {
                let sessions = registry.sessions.lock().unwrap();
                !registry.shutdown_requested()
                    && registry.is_current(&device, generation)
                    && sessions.get(&device).is_some_and(|slot| {
                        slot.generation == generation
                            && slot.child.is_none()
                            && !slot.cancel.load(Ordering::SeqCst)
                    })
            };
            if !allowed {
                if let Some(removed) = take_placeholder(&device, generation) {
                    emit_session_stopped(&removed, &device, None);
                }
                return;
            }

            let new_args = build_scrcpy_args(&config, video_dir.clone(), Some(next_codec));
            match spawn_scrcpy_streams(
                &sink,
                &exe_path,
                &adb_exe_path,
                server_path.as_deref(),
                &new_args,
                config.vsync.unwrap_or(true),
            )
            .await
            {
                Ok((new_child, new_flag, new_readers)) => {
                    let new_pid = new_child.id();
                    let mut child_opt = Some(new_child);
                    let mut readers_opt = Some(new_readers);
                    let installed = {
                        let mut sessions = registry.sessions.lock().unwrap();
                        if registry.shutdown_requested()
                            || !registry.is_current(&device, generation)
                        {
                            false
                        } else if let Some(slot) = sessions.get_mut(&device) {
                            if slot.generation == generation && slot.child.is_none() {
                                slot.child = child_opt.take();
                                slot.pid = new_pid;
                                slot.readers = readers_opt.take().unwrap_or_default();
                                slot.pos = Arc::new(Mutex::new(WindowPosTrack::default()));
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };
                    if !installed {
                        if let Some(removed) = take_placeholder(&device, generation) {
                            emit_session_stopped_if_owned(&removed, &device, None);
                        }
                        if let Some(mut child) = child_opt.take() {
                            let _ = child.start_kill();
                            let _ = timeout(Duration::from_secs(2), child.wait()).await;
                        }
                        join_readers(readers_opt.take().unwrap_or_default()).await;
                        return;
                    }
                    audio_flag = new_flag;
                    chain_index += 1;
                    continue;
                }
                Err(e) => {
                    sink.emit(
                        "scrcpy-log",
                        format!("[SYSTEM] Failed to spawn retry: {}", e),
                    );
                    if let Some(removed) = take_placeholder(&device, generation) {
                        emit_session_stopped_if_owned(&removed, &device, None);
                    }
                    return;
                }
            }
        } else {
            if should_auto_fallback && audio_failed {
                sink.emit(
                    "scrcpy-log",
                    "[SYSTEM] No compatible audio codec found. Consider disabling audio forwarding."
                        .to_string(),
                );
            }
            emit_session_stopped_if_owned(&session, &device, None);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(session_mode: &str) -> ScrcpyConfig {
        ScrcpyConfig {
            device: "device1".to_string(),
            session_mode: session_mode.to_string(),
            bitrate: None,
            fps: None,
            stay_awake: None,
            turn_off: None,
            audio_enabled: None,
            audio_codec: None,
            always_on_top: None,
            fullscreen: None,
            borderless: None,
            record: None,
            record_path: None,
            scrcpy_path: None,
            otg_pure: None,
            camera_facing: None,
            camera_id: None,
            codec: None,
            camera_ar: None,
            camera_high_speed: None,
            vd_width: None,
            vd_height: None,
            vd_dpi: None,
            rotation: None,
            res: None,
            hid_keyboard: None,
            hid_mouse: None,
            render_driver: None,
            hwdec: None,
            flex_display: None,
            camera_torch: None,
            camera_zoom: None,
            background_color: None,
            keep_active: None,
            vsync: None,
            window_x: None,
            window_y: None,
            ignore_video_encoder_constraints: None,
        }
    }

    #[test]
    fn test_build_scrcpy_args_mirror_defaults() {
        let config = base_config("mirror");
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"-s".to_string()));
        assert!(args.contains(&"device1".to_string()));
        assert!(args.contains(&"--video-codec=h264".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_camera_mode() {
        let mut config = base_config("camera");
        config.fps = Some(30);
        config.camera_facing = Some("front".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--video-source=camera".to_string()));
        assert!(args.contains(&"--camera-facing=front".to_string()));
        assert!(args.contains(&"--camera-fps".to_string()));
        assert!(args.contains(&"30".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_bitrate_and_fps() {
        let mut config = base_config("mirror");
        config.bitrate = Some(8);
        config.fps = Some(60);

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--video-bit-rate".to_string()));
        assert!(args.contains(&"8M".to_string()));
        assert!(args.contains(&"--max-fps".to_string()));
        assert!(args.contains(&"60".to_string()));
    }

    #[test]
    fn test_hwdec_default_omits_flag() {
        let mut config = base_config("mirror");
        config.hwdec = Some("auto".to_string());
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--hwdec")));
    }

    #[test]
    fn test_hwdec_disabled_adds_flag() {
        let mut config = base_config("mirror");
        config.hwdec = Some("disabled".to_string());
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--hwdec=disabled".to_string()));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_hwdec_d3d11va_allowed_on_windows() {
        let mut config = base_config("mirror");
        config.hwdec = Some("d3d11va".to_string());
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--hwdec=d3d11va".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_hwdec_vaapi_allowed_on_linux() {
        let mut config = base_config("mirror");
        config.hwdec = Some("vaapi".to_string());
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--hwdec=vaapi".to_string()));
    }

    #[test]
    fn test_hwdec_invalid_value_is_ignored() {
        let mut config = base_config("mirror");
        config.hwdec = Some("--no-audio".to_string());
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--hwdec")));
        assert!(!args.contains(&"--no-audio".to_string()));
    }

    #[test]
    fn test_windows_arm64_download_uses_native_asset() {
        assert_eq!(windows_arch_tag("x86_64"), "win64");
        assert_eq!(windows_arch_tag("aarch64"), "winarm64");
        assert_eq!(windows_arch_tag("x86"), "win32");
    }

    #[test]
    fn test_parse_sha256_manifest_matches_filename() {
        let manifest = "\
deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  scrcpy-win64-v5.0.1.zip\r\n\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef *scrcpy-winarm64-v5.0.1.zip\n";
        assert_eq!(
            parse_sha256_manifest(manifest, "scrcpy-win64-v5.0.1.zip"),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string())
        );
        assert_eq!(
            parse_sha256_manifest(manifest, "scrcpy-winarm64-v5.0.1.zip"),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string())
        );
        assert_eq!(parse_sha256_manifest(manifest, "missing.zip"), None);
    }

    #[test]
    fn test_audio_codec_auto_adds_no_flag() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("auto".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--audio-codec")));
    }

    #[test]
    fn test_audio_codec_manual_adds_flag() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("aac".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--audio-codec=aac".to_string()));
    }

    #[test]
    fn test_audio_codec_override_takes_precedence() {
        let mut config = base_config("mirror");
        config.audio_codec = Some("auto".to_string());

        let args = build_scrcpy_args(&config, None, Some("flac"));
        assert!(args.contains(&"--audio-codec=flac".to_string()));
    }

    #[test]
    fn test_audio_codec_skipped_when_audio_disabled() {
        let mut config = base_config("mirror");
        config.audio_enabled = Some(false);
        config.audio_codec = Some("aac".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--no-audio".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--audio-codec")));
    }

    #[test]
    fn test_is_audio_codec_error_matches_known_patterns() {
        assert!(is_audio_codec_error(
            "ERROR: Could not create default audio encoder for opus"
        ));
        assert!(is_audio_codec_error("Failed to initialize audio/opus"));
        assert!(is_audio_codec_error("audio codec not supported on device"));
        assert!(is_audio_codec_error("audio encoder failed to start"));
    }

    #[test]
    fn test_is_audio_codec_error_ignores_unrelated() {
        assert!(!is_audio_codec_error("ADB disconnected"));
        assert!(!is_audio_codec_error("device offline"));
        assert!(!is_audio_codec_error("video encoder error"));
        assert!(!is_audio_codec_error("permission issue"));
        assert!(!is_audio_codec_error("INFO Audio codec selected: opus"));
    }

    #[test]
    fn test_build_scrcpy_args_window_position() {
        let mut config = base_config("mirror");
        config.window_x = Some(100);
        config.window_y = Some(200);
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--window-x=100".to_string()));
        assert!(args.contains(&"--window-y=200".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_negative_coords() {
        // Multi-monitor: a display left of / above the primary yields negative
        // coordinates, which scrcpy accepts.
        let mut config = base_config("mirror");
        config.window_x = Some(-1920);
        config.window_y = Some(0);
        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--window-x=-1920".to_string()));
        assert!(args.contains(&"--window-y=0".to_string()));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_skipped_in_fullscreen() {
        let mut config = base_config("mirror");
        config.window_x = Some(100);
        config.window_y = Some(200);
        config.fullscreen = Some(true);
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--window-x")));
        assert!(!args.iter().any(|a| a.starts_with("--window-y")));
    }

    #[test]
    fn test_build_scrcpy_args_window_position_only_mirror() {
        // Camera (and desktop) sessions must not receive the mirror window's
        // remembered position, it is scoped to mirror mode.
        let mut config = base_config("camera");
        config.window_x = Some(100);
        config.window_y = Some(200);
        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--window-x")));
        assert!(!args.iter().any(|a| a.starts_with("--window-y")));
    }

    #[test]
    fn test_parse_shell_var_int() {
        let text = "WINDOW=12345\nX=959\nY=24\nWIDTH=428\nHEIGHT=928\n";
        assert_eq!(parse_shell_var_int(text, "X"), Some(959));
        assert_eq!(parse_shell_var_int(text, "Y"), Some(24));
        assert_eq!(parse_shell_var_int(text, "MISSING"), None);
    }

    #[test]
    fn test_resolve_window_pos_first_launch_uses_raw() {
        // First launch ever (nothing requested): persist the raw last sample.
        assert_eq!(
            resolve_window_pos_to_persist(None, Some((100, 72)), (300, 222)),
            (300, 222)
        );
        assert_eq!(
            resolve_window_pos_to_persist(None, None, (300, 222)),
            (300, 222)
        );
    }

    #[test]
    fn test_resolve_window_pos_zero_offset_is_noop() {
        // Windows/Linux: the tool already reports the client origin, so the
        // first sample matches what we requested -> offset 0 -> last unchanged.
        let requested = Some((100, 100));
        let first = Some((100, 100));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (640, 480)),
            (640, 480)
        );
    }

    #[test]
    fn test_resolve_window_pos_cancels_titlebar_offset() {
        // macOS-like: the tool reports the frame origin (title-bar top), 28 px
        // above the client origin scrcpy positions. Requested (100,100) but
        // first observed (100,72); the -28 px offset must be cancelled so the
        // persisted value maps back to the client origin.
        let requested = Some((100, 100));
        let first = Some((100, 72));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (300, 222)),
            (300, 250)
        );
    }

    #[test]
    fn test_resolve_window_pos_cancels_full_frame_offset() {
        // Frame offset on both axes (border + title bar).
        let requested = Some((100, 100));
        let first = Some((92, 69));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (292, 219)),
            (300, 250)
        );
    }

    #[test]
    fn test_resolve_window_pos_ignores_implausible_offset() {
        // The window was moved before the first sample (or placed by the WM):
        // the offset dwarfs any window decoration, so fall back to the raw last.
        let requested = Some((100, 100));
        let first = Some((900, 700));
        assert_eq!(
            resolve_window_pos_to_persist(requested, first, (950, 760)),
            (950, 760)
        );
    }

    #[test]
    fn test_parse_adb_device_line_usb_serial() {
        assert_eq!(
            parse_adb_device_line("ZY22MGW35T device"),
            Some(("ZY22MGW35T".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_tcpip_serial() {
        assert_eq!(
            parse_adb_device_line("192.168.1.50:5555 device"),
            Some(("192.168.1.50:5555".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_mdns_serial() {
        assert_eq!(
            parse_adb_device_line("adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp device"),
            Some((
                "adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp".to_string(),
                "device".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_adb_device_line_mdns_conflict_serial_with_space() {
        // mDNS conflict resolution appends " (2)" inside the serial itself, so
        // the line has a space that is NOT the serial/state separator.
        assert_eq!(
            parse_adb_device_line("adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp device"),
            Some((
                "adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp".to_string(),
                "device".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_adb_device_line_ignores_trailing_l_fields() {
        // `adb devices -l` appends product/model/transport_id fields after the
        // state; they must not affect the parsed serial or state.
        assert_eq!(
            parse_adb_device_line(
                "ZY22MGW35T device usb:1-1 product:foo model:bar device:baz transport_id:1"
            ),
            Some(("ZY22MGW35T".to_string(), "device".to_string()))
        );
    }

    #[test]
    fn test_extract_device_model_replaces_underscores_with_spaces() {
        assert_eq!(
            extract_device_model(
                "ZY22MGW35T device usb:1-1 product:foo model:SM_S911B device:baz transport_id:1"
            ),
            Some("SM S911B".to_string())
        );
    }

    #[test]
    fn test_extract_device_model_missing_field_returns_none() {
        assert_eq!(extract_device_model("ZY22MGW35T device"), None);
    }

    #[test]
    fn test_parse_adb_device_line_offline_and_unauthorized() {
        assert_eq!(
            parse_adb_device_line("ZY22MGW35T offline"),
            Some(("ZY22MGW35T".to_string(), "offline".to_string()))
        );
        assert_eq!(
            parse_adb_device_line("192.168.1.50:5555 unauthorized"),
            Some(("192.168.1.50:5555".to_string(), "unauthorized".to_string()))
        );
    }

    #[test]
    fn test_parse_adb_device_line_unparsable_returns_none() {
        assert_eq!(parse_adb_device_line(""), None);
        assert_eq!(parse_adb_device_line("List of devices attached"), None);
    }

    #[test]
    fn test_get_devices_filtering_keeps_only_device_state() {
        // Mirrors the filter chain in get_devices: only the exact "device"
        // state is kept, regardless of serial shape, including mDNS wireless
        // debugging serials (._tcp/._udp) that must no longer be excluded.
        let output = "List of devices attached\n\
             ZY22MGW35T device usb:1-1 product:foo model:bar transport_id:1\n\
             192.168.1.50:5555 device product:foo model:bar transport_id:2\n\
             adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp device product:foo transport_id:3\n\
             adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp device product:foo transport_id:4\n\
             emulator-5554 offline\n\
             0123456789ABCDEF unauthorized\n";

        let devices: Vec<String> = output
            .lines()
            .skip(1)
            .filter_map(parse_adb_device_line)
            .filter(|(_, state)| state == "device")
            .map(|(serial, _)| serial)
            .collect();

        assert_eq!(
            devices,
            vec![
                "ZY22MGW35T".to_string(),
                "192.168.1.50:5555".to_string(),
                "adb-ZY22MGW35T-Z3uXXq._adb-tls-connect._tcp".to_string(),
                "adb-ZY22MGW35T-Z3uXXq (2)._adb-tls-connect._tcp".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_camera_id_accepts_digits_only() {
        assert_eq!(parse_camera_id("0"), Some(0));
        assert_eq!(parse_camera_id(" 12 "), Some(12));
        assert_eq!(parse_camera_id("back"), None);
        assert_eq!(parse_camera_id("1 --no-audio"), None);
        assert_eq!(parse_camera_id("-1"), None);
        assert_eq!(parse_camera_id(""), None);
    }

    #[test]
    fn test_camera_id_numeric_wins_over_facing() {
        let mut config = base_config("camera");
        config.camera_id = Some("2".to_string());
        config.camera_facing = Some("front".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(args.contains(&"--camera-id=2".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--camera-facing")));
    }

    #[test]
    fn test_camera_id_label_falls_back_to_facing() {
        // Old configs wrote "back"/"front"/"external" into camera_id.
        let mut config = base_config("camera");
        config.camera_id = Some("back".to_string());
        config.camera_facing = Some("front".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--camera-id")));
        assert!(args.contains(&"--camera-facing=front".to_string()));
    }

    #[test]
    fn test_camera_id_flag_injection_is_ignored() {
        let mut config = base_config("camera");
        config.camera_id = Some("1 --no-audio".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--camera-id")));
        assert!(!args.contains(&"--no-audio".to_string()));
    }

    #[test]
    fn test_camera_facing_allowlist() {
        assert_eq!(resolve_camera_facing(" FRONT "), Some("front"));
        assert_eq!(resolve_camera_facing("external"), Some("external"));
        assert_eq!(resolve_camera_facing("--no-audio"), None);

        let mut config = base_config("camera");
        config.camera_facing = Some("--no-audio".to_string());

        let args = build_scrcpy_args(&config, None, None);
        assert!(!args.iter().any(|a| a.starts_with("--camera-facing")));
    }

    #[test]
    fn test_sanitize_record_component_whitelist() {
        assert_eq!(sanitize_record_component("a/b\\c d"), "a_b_c_d");
        assert_eq!(
            sanitize_record_component("192.168.1.5:5555"),
            "192.168.1.5_5555"
        );
        assert_eq!(sanitize_record_component("../../evil"), ".._.._evil");
        assert_eq!(sanitize_record_component("é"), "_");
        assert_eq!(sanitize_record_component(""), "device");
        assert!(sanitize_record_component(&"x".repeat(100)).len() <= 64);
    }

    #[test]
    fn test_record_filename_uses_sanitized_device() {
        let mut config = base_config("mirror");
        config.device = "192.168.1.5:5555".to_string();
        config.record = Some(true);
        config.record_path = Some(std::env::temp_dir().to_string_lossy().to_string());

        let args = build_scrcpy_args(&config, None, None);
        let record = args
            .iter()
            .find(|arg| arg.starts_with("--record="))
            .expect("record flag");
        let filename = std::path::Path::new(record.trim_start_matches("--record="))
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        assert!(filename.starts_with("scrcpy_192.168.1.5_5555_"));
        assert!(filename.ends_with(".mkv"));
        assert!(
            filename
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        );
    }

    #[test]
    fn test_split_args_rejects_unclosed_quote() {
        assert!(split_args("adb shell \"unterminated").is_err());
        assert_eq!(
            split_args("shell \"hello world\"").unwrap(),
            vec!["shell".to_string(), "hello world".to_string()]
        );
    }

    #[tokio::test]
    async fn test_read_capped_keeps_limit_and_drains() {
        let input = vec![b'a'; 4096];
        let text = read_capped(&input[..], 1024).await;
        assert!(text.starts_with("aaaa"));
        assert!(text.contains("[output truncated]"));
        assert!(text.len() < 2048);
    }

    #[test]
    fn test_progress_throttle() {
        assert!(should_emit_progress(None, 1, Duration::ZERO));
        assert!(!should_emit_progress(
            Some(1),
            1,
            Duration::from_millis(500)
        ));
        assert!(!should_emit_progress(Some(1), 2, Duration::from_millis(10)));
        assert!(should_emit_progress(Some(1), 2, Duration::from_millis(150)));
    }

    #[test]
    fn test_download_progress_event_contract() {
        let (event, payload) = download_progress_event(42);
        assert_eq!(event, "scrcpy-status");
        assert_eq!(payload["type"], "download-progress");
        assert_eq!(payload["percent"], 42);
    }

    #[test]
    fn test_audio_fallback_requires_failed_exit() {
        assert!(should_audio_fallback(true, false, true, 0));
        // A normal window close (status 0) is not retried even if stderr
        // happened to contain an audio error pattern.
        assert!(!should_audio_fallback(true, true, true, 0));
        assert!(!should_audio_fallback(false, false, true, 0));
        assert!(!should_audio_fallback(true, false, false, 0));
        assert!(!should_audio_fallback(
            true,
            false,
            true,
            AUDIO_FALLBACK_CHAIN.len()
        ));
    }

    #[test]
    fn test_sha256_of_reader_matches_known_digest() {
        assert_eq!(
            sha256_of_reader(&b"abc"[..]).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_verify_downloaded_file_checks_length_and_hash() {
        let workspace = TempWorkspace::create(&std::env::temp_dir()).unwrap();
        let path = workspace.root.join("archive.bin");
        std::fs::write(&path, b"abc").unwrap();

        let file = std::fs::File::open(&path).unwrap();
        assert!(verify_downloaded_file(file, 4).is_err());

        let file = std::fs::File::open(&path).unwrap();
        assert_eq!(
            verify_downloaded_file(file, 3).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_temp_workspace_is_unique_and_cleaned_up() {
        let base = std::env::temp_dir();
        let first = TempWorkspace::create(&base).unwrap();
        let second = TempWorkspace::create(&base).unwrap();
        assert_ne!(first.root, second.root);
        assert!(first.root.exists());

        let first_root = first.root.clone();
        drop(first);
        assert!(!first_root.exists());
    }

    #[test]
    fn test_cwd_binary_fallback_is_debug_only() {
        assert_eq!(include_cwd_scrcpy_bin(), cfg!(debug_assertions));
    }

    #[test]
    fn test_binary_filename_has_platform_extension() {
        let name = binary_filename("scrcpy");
        assert!(name.starts_with("scrcpy"));
        #[cfg(target_os = "windows")]
        assert_eq!(name, "scrcpy.exe");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_system32_tool_is_absolute() {
        let tool = system32_tool("taskkill.exe");
        assert!(tool.is_absolute());
        assert_eq!(tool.file_name().unwrap().to_string_lossy(), "taskkill.exe");
        assert_eq!(
            tool.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase(),
            "system32"
        );
    }

    #[test]
    fn test_friendly_name_from_getprop() {
        assert_eq!(
            friendly_name_from_getprop("ZY22MGW35T", "Galaxy S23\nsamsung\nSM-S911B\n"),
            Some("Galaxy S23".to_string())
        );
        assert_eq!(
            friendly_name_from_getprop("ZY22MGW35T", "\nGoogle\nPixel 7\n"),
            Some("Google Pixel 7".to_string())
        );
        assert_eq!(
            friendly_name_from_getprop("ZY22MGW35T", "\nsamsung\nSamsung SM-S911B\n"),
            Some("Samsung SM-S911B".to_string())
        );
        assert_eq!(
            friendly_name_from_getprop("emulator-5554", "Pixel_7_API_34\n"),
            Some("Pixel 7 API 34 (emulator-5554)".to_string())
        );
        assert_eq!(friendly_name_from_getprop("ZY22MGW35T", "\n\n"), None);
    }

    #[test]
    fn test_is_emulator_serial() {
        assert!(is_emulator_serial("emulator-5554"));
        assert!(!is_emulator_serial("emulator-"));
        assert!(!is_emulator_serial("emulator-abc"));
        assert!(!is_emulator_serial("ZY22MGW35T"));
    }

    #[test]
    fn test_generation_and_is_running_registry() {
        let device = "unit-test-generation-device";
        let first = next_generation(device);
        let second = next_generation(device);
        assert!(second > first);
        assert!(!is_running(device));

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = EventSink::new(tx);
        {
            let mut sessions = registry().sessions.lock().unwrap();
            sessions.insert(
                device.to_string(),
                Session {
                    generation: second,
                    pid: None,
                    mirror_window: false,
                    child: None,
                    readers: Vec::new(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    sink,
                    requested_pos: None,
                    pos: Arc::new(Mutex::new(WindowPosTrack::default())),
                },
            );
        }
        assert!(is_running(device));

        registry().sessions.lock().unwrap().remove(device);
        assert!(!is_running(device));
    }

    #[tokio::test]
    async fn test_stop_scrcpy_cancels_placeholder_and_emits_stopped() {
        let device = "unit-test-stop-placeholder";
        let generation = next_generation(device);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = EventSink::new(tx);
        {
            let mut sessions = registry().sessions.lock().unwrap();
            sessions.insert(
                device.to_string(),
                Session {
                    generation,
                    pid: None,
                    mirror_window: false,
                    child: None,
                    readers: Vec::new(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    sink,
                    requested_pos: None,
                    pos: Arc::new(Mutex::new(WindowPosTrack::default())),
                },
            );
        }
        assert!(is_running(device));

        stop_scrcpy(device).await.unwrap();
        assert!(!is_running(device));

        let mut saw_stopped = false;
        while let Ok(event) = rx.try_recv() {
            if event.name == "scrcpy-status"
                && event.payload.get("device").and_then(|v| v.as_str()) == Some(device)
                && event.payload.get("running").and_then(|v| v.as_bool()) == Some(false)
            {
                saw_stopped = true;
            }
        }
        assert!(saw_stopped);
    }
}

pub async fn stop_scrcpy(device: &str) -> Result<(), String> {
    let registry = registry();
    // Invalidate the generation first: an in-flight start/monitor must not be
    // able to resurrect this session after the stop.
    registry.bump_generation(device);

    // Sample the mirror window while the process is still alive, before the
    // registry entry (and its pid) is removed. Camera/desktop sessions have no
    // remembered mirror position, so they are skipped.
    let snapshot = {
        let sessions = registry.sessions.lock().unwrap();
        sessions
            .get(device)
            .map(|session| (session.pid, session.mirror_window))
    };
    let fresh_pos = match snapshot {
        Some((Some(pid), true)) => capture_window_pos(pid).await,
        _ => None,
    };

    let Some(mut session) = registry.sessions.lock().unwrap().remove(device) else {
        return Ok(());
    };
    session.cancel.store(true, Ordering::SeqCst);

    let kill_result = match session.pid {
        Some(pid) => Some(terminate_process_tree(pid).await),
        None => None,
    };

    // Wait for and verify the exit. taskkill /F /T should be immediate; a
    // process that ignores it falls back to Child::start_kill.
    let wait_result: Result<(), String> = if let Some(mut child) = session.child.take() {
        match timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("Failed to wait for scrcpy: {}", e)),
            Err(_) => {
                let _ = child.start_kill();
                match timeout(Duration::from_secs(2), child.wait()).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(format!("Failed to kill scrcpy: {}", e)),
                    Err(_) => Err("scrcpy did not exit after taskkill".to_string()),
                }
            }
        }
    } else {
        Ok(())
    };

    join_readers(std::mem::take(&mut session.readers)).await;
    emit_session_stopped_if_owned(&session, device, fresh_pos);

    match (kill_result, wait_result) {
        (_, Err(e)) => Err(e),
        (Some(Err(e)), Ok(())) => {
            // taskkill may say "not found" for a process that exited on its own
            // between sampling and killing; Child::wait already confirmed exit.
            session.sink.emit("scrcpy-log", format!("[SYSTEM] {}", e));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Windows asset suffix published by the scrcpy release workflow.
fn windows_arch_tag(target_arch: &str) -> &'static str {
    match target_arch {
        "x86_64" => "win64",
        "aarch64" => "winarm64",
        _ => "win32",
    }
}

/// Find the expected lowercase SHA-256 for `filename` in a release's
/// `SHA256SUMS.txt`. Lines are `<hash>  <filename>` (optionally `*filename`).
fn parse_sha256_manifest(manifest: &str, filename: &str) -> Option<String> {
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        if name.trim_start_matches('*') == filename
            && hash.len() == 64
            && hash.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Scratch directory for one download. The unique name means two instances (or
/// a retry) never share `scrcpy_temp.zip` / `temp_extract`; Drop removes it on
/// every exit path, including errors.
struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    fn create(base: &Path) -> Result<Self, String> {
        for _ in 0..16 {
            let root = base.join(format!(".scrcpy-download-{}", unique_suffix()));
            match std::fs::create_dir(&root) {
                Ok(()) => return Ok(Self { root }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!("Failed to create temporary download folder: {}", e));
                }
            }
        }
        Err("Failed to create a unique temporary download folder".to_string())
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sha256_of_reader<R: std::io::Read>(mut reader: R) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("Failed to read downloaded archive: {}", e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Verifies the file through the same handle that was written (rewound), not a
/// detached in-memory buffer or a re-opened path.
fn verify_downloaded_file(mut file: std::fs::File, expected_len: u64) -> Result<String, String> {
    use std::io::{Seek, SeekFrom};

    let actual_len = file
        .metadata()
        .map_err(|e| format!("Failed to stat downloaded archive: {}", e))?
        .len();
    if actual_len != expected_len {
        return Err(format!(
            "Downloaded archive length mismatch: expected {} bytes, got {}",
            expected_len, actual_len
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Failed to rewind downloaded archive: {}", e))?;
    sha256_of_reader(file)
}

/// At most one progress event per whole percent and per 100 ms.
fn should_emit_progress(last_percent: Option<u64>, percent: u64, elapsed: Duration) -> bool {
    match last_percent {
        None => true,
        Some(last) => percent >= last.saturating_add(1) && elapsed >= Duration::from_millis(100),
    }
}

/// Download progress travels as the top-level `scrcpy-status` event with a
/// `download-progress` type, matching `Ui::handle_status`.
fn download_progress_event(percent: u64) -> (&'static str, serde_json::Value) {
    (
        "scrcpy-status",
        json!({ "type": "download-progress", "percent": percent }),
    )
}

fn extract_archive(archive: &Path, dest: &Path, extension: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("Failed to create extraction directory: {}", e))?;

    if extension == ".zip" {
        let file =
            std::fs::File::open(archive).map_err(|e| format!("Failed to open zip: {}", e))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip archive: {}", e))?;
        zip.extract(dest)
            .map_err(|e| format!("Failed to extract: {}", e))?;
    } else {
        let file =
            std::fs::File::open(archive).map_err(|e| format!("Failed to open tar.gz: {}", e))?;
        let tar = GzDecoder::new(file);
        let mut archive = Archive::new(tar);
        archive
            .unpack(dest)
            .map_err(|e| format!("Failed to extract tar: {}", e))?;
    }
    Ok(())
}

/// Moves the extracted tree into `<exe dir>/scrcpy-bin`. The current install is
/// renamed aside first and restored if the swap fails, so a failed update
/// leaves the working installation untouched.
fn install_extracted(staging: &Path, extract_path: &Path) -> Result<(), String> {
    let mut entries =
        std::fs::read_dir(staging).map_err(|e| format!("Failed to read archive: {}", e))?;
    let first = entries
        .next()
        .transpose()
        .map_err(|e| format!("Failed to read archive entry: {}", e))?;
    let has_more = entries.next().is_some();
    let source = match (first, has_more) {
        (Some(entry), false) if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) => {
            entry.path()
        }
        _ => staging.to_path_buf(),
    };

    let expected = binary_filename("scrcpy");
    if !source.join(&expected).is_file() {
        return Err(format!("Downloaded archive does not contain {}", expected));
    }

    let parent = extract_path
        .parent()
        .ok_or_else(|| "Invalid scrcpy installation path".to_string())?;
    let backup = parent.join(format!("scrcpy-bin.old-{}", unique_suffix()));
    let had_old = extract_path.exists();
    if had_old {
        std::fs::rename(extract_path, &backup)
            .map_err(|e| format!("Failed to stage the current installation: {}", e))?;
    }

    match std::fs::rename(&source, extract_path) {
        Ok(()) => {
            if had_old {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = std::fs::rename(&backup, extract_path);
            }
            Err(format!("Failed to install scrcpy binaries: {}", e))
        }
    }
}

async fn download_scrcpy_inner(sink: Arc<EventSink>) -> Result<(), String> {
    let (os_tag, arch_tag, extension) = if cfg!(target_os = "windows") {
        let arch = windows_arch_tag(std::env::consts::ARCH);
        (arch, arch, ".zip")
    } else if cfg!(target_os = "linux") {
        ("linux", "linux-x86_64", ".tar.gz")
    } else if cfg!(target_os = "macos") {
        let arch = if cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else {
            "macos-x86_64"
        };
        ("macos", arch, ".tar.gz")
    } else {
        return Err("Unsupported OS for auto-download".to_string());
    };

    sink.emit(
        "scrcpy-log",
        format!("[SYSTEM] Detecting platform: {} ({})", os_tag, arch_tag),
    );
    sink.emit("scrcpy-status", json!({ "type": "downloading", "success": true, "message": format!("Fetching latest {} release...", arch_tag) }));

    let client = reqwest::Client::builder()
        .user_agent("ScrcpyGui-Downloader")
        .build()
        .map_err(|e| e.to_string())?;

    // Attempt to get the latest release via API, but fallback to redirect scraping if rate limited
    let mut download_url = String::new();
    let mut filename = String::new();
    let mut release_tag = String::new();

    let api_url = "https://api.github.com/repos/Genymobile/scrcpy/releases/latest";
    let api_resp = client.get(api_url).send().await;

    let mut used_fallback = false;

    if let Ok(resp) = api_resp {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                release_tag = json["tag_name"].as_str().unwrap_or("").to_string();
                if let Some(assets) = json["assets"].as_array() {
                    for asset in assets {
                        let name = asset["name"].as_str().unwrap_or("");
                        if name.contains(arch_tag) && name.ends_with(extension) {
                            download_url = asset["browser_download_url"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            filename = name.to_string();
                            break;
                        }
                    }
                }
            }
        } else if resp.status() == reqwest::StatusCode::FORBIDDEN {
            sink.emit(
                "scrcpy-log",
                "[SYSTEM] API rate limited, attempting fallback discovery...",
            );
            used_fallback = true;
        }
    } else {
        used_fallback = true;
    }

    if used_fallback || download_url.is_empty() {
        // Fallback: Follow redirect of /releases/latest to get the tag name
        let redirect_res = client
            .get("https://github.com/Genymobile/scrcpy/releases/latest")
            .send()
            .await
            .map_err(|e| format!("Fallback failed: {}", e))?;

        let final_url = redirect_res.url().as_str();
        // URL is like https://github.com/Genymobile/scrcpy/releases/tag/v2.7
        if let Some(tag) = final_url.split('/').next_back() {
            if tag.starts_with('v') {
                release_tag = tag.to_string();
                filename = format!("scrcpy-{}-{}{}", arch_tag, tag, extension);
                download_url = format!(
                    "https://github.com/Genymobile/scrcpy/releases/download/{}/{}",
                    tag, filename
                );
                sink.emit(
                    "scrcpy-log",
                    format!("[SYSTEM] Discovered latest tag via fallback: {}", tag),
                );
            }
        }
    }

    if download_url.is_empty() {
        return Err(format!(
            "Could not find {} binary. (API rate limit might be active)",
            arch_tag
        ));
    }

    if release_tag.is_empty() {
        release_tag = download_url
            .split("/download/")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("")
            .to_string();
    }
    if release_tag.is_empty() {
        return Err(
            "Could not determine the scrcpy release tag for checksum verification".to_string(),
        );
    }

    sink.emit("scrcpy-log", format!("[SYSTEM] Found asset: {}", filename));

    // Verify the archive against the release's published SHA256SUMS.txt before
    // extracting or executing anything. Fail closed: an unverifiable download is
    // not installed.
    let checksum_url = format!(
        "https://github.com/Genymobile/scrcpy/releases/download/{}/SHA256SUMS.txt",
        release_tag
    );
    let checksum_resp = client
        .get(&checksum_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch SHA256SUMS.txt: {}", e))?;
    if !checksum_resp.status().is_success() {
        return Err(format!(
            "Could not fetch SHA256SUMS.txt for {} (HTTP {})",
            release_tag,
            checksum_resp.status()
        ));
    }
    let checksum_text = checksum_resp
        .text()
        .await
        .map_err(|e| format!("Failed to read SHA256SUMS.txt: {}", e))?;
    let expected_sha256 = parse_sha256_manifest(&checksum_text, &filename).ok_or_else(|| {
        format!(
            "SHA-256 checksum for {} not found in SHA256SUMS.txt",
            filename
        )
    })?;

    sink.emit(
        "scrcpy-log",
        format!("[SYSTEM] Verifying SHA-256 for {}", filename),
    );

    let current_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .ok_or_else(|| "Could not locate the application directory".to_string())?;

    let workspace = TempWorkspace::create(&current_dir)?;
    let archive_path = workspace.root.join(if extension == ".zip" {
        "package.zip"
    } else {
        "package.tar.gz"
    });
    let extract_path = current_dir.join("scrcpy-bin");

    let mut file = tokio::fs::File::create(&archive_path)
        .await
        .map_err(|e| format!("Failed to create archive file: {}", e))?;
    let mut download_resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to download URL: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download request failed: {}", e))?;
    let total_size = download_resp.content_length().unwrap_or(0);

    sink.emit(
        "scrcpy-log",
        format!("[SYSTEM] Downloading: {} MB", total_size / 1024 / 1024),
    );

    let mut downloaded: u64 = 0;
    let mut last_percent: Option<u64> = None;
    let mut last_emit = Instant::now();
    while let Some(chunk) = download_resp
        .chunk()
        .await
        .map_err(|e| format!("Download interrupted: {}", e))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write chunk: {}", e))?;
        downloaded += chunk.len() as u64;
        if let Some(percent) = (downloaded * 100).checked_div(total_size) {
            if should_emit_progress(last_percent, percent, last_emit.elapsed()) {
                let (event, payload) = download_progress_event(percent);
                sink.emit(event, payload);
                last_percent = Some(percent);
                last_emit = Instant::now();
            }
        }
    }
    file.flush()
        .await
        .map_err(|e| format!("Failed to flush archive file: {}", e))?;
    file.sync_all()
        .await
        .map_err(|e| format!("Failed to sync archive file: {}", e))?;
    if total_size > 0 && last_percent != Some(100) {
        let (event, payload) = download_progress_event(100);
        sink.emit(event, payload);
    }

    let std_file = file.into_std().await;
    let actual_sha256 =
        tokio::task::spawn_blocking(move || verify_downloaded_file(std_file, downloaded))
            .await
            .map_err(|e| format!("Checksum task failed: {}", e))??;

    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            filename, expected_sha256, actual_sha256
        ));
    }

    sink.emit(
        "scrcpy-log",
        "[SYSTEM] SHA-256 verified. Starting extraction...",
    );
    sink.emit(
        "scrcpy-status",
        json!({ "type": "downloading", "success": true, "message": "Extracting binaries..." }),
    );

    let staging = workspace.root.join("extract");
    let archive_for_task = archive_path.clone();
    let staging_for_task = staging.clone();
    let extension_for_task = extension.to_string();
    tokio::task::spawn_blocking(move || {
        extract_archive(&archive_for_task, &staging_for_task, &extension_for_task)
    })
    .await
    .map_err(|e| format!("Extraction task failed: {}", e))??;

    let install_target = extract_path.clone();
    tokio::task::spawn_blocking(move || install_extracted(&staging, &install_target))
        .await
        .map_err(|e| format!("Installation task failed: {}", e))??;

    sink.emit(
        "scrcpy-status",
        json!({ "type": "download-complete", "success": true, "message": extract_path.to_string_lossy() }),
    );
    Ok(())
}

pub async fn download_scrcpy(sink: Arc<EventSink>) -> Result<(), String> {
    let result = download_scrcpy_inner(sink.clone()).await;
    if let Err(message) = &result {
        sink.emit(
            "scrcpy-status",
            json!({
                "type": "download-complete",
                "success": false,
                "message": message,
            }),
        );
    }
    result
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub async fn get_videos_dir() -> Result<String, String> {
    super::video_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "Videos directory not found".to_string())
}

pub async fn save_report(content: String, name: String) -> Result<String, String> {
    use std::fs;

    let downloads = super::downloads_dir().ok_or_else(|| "no Downloads folder".to_string())?;

    let path = downloads.join(&name);
    fs::write(&path, content).map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
}

#[allow(dead_code)] // legacy/unwired API, kept for follow-up
pub async fn get_scrcpy_bin_dir() -> Result<String, String> {
    let current_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let extract_path = current_dir.join("scrcpy-bin");

    if extract_path.exists() {
        Ok(extract_path.to_string_lossy().to_string())
    } else {
        Ok(current_dir.to_string_lossy().to_string())
    }
}

pub fn get_local_scrcpy_version(custom_path: Option<String>) -> Option<String> {
    let exe_path = get_binary_path("scrcpy", custom_path);
    let mut cmd = std::process::Command::new(&exe_path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.arg("--version").output().ok()?;
    if output.status.success() {
        let out_str = String::from_utf8_lossy(&output.stdout);
        for line in out_str.lines() {
            if line.contains("scrcpy") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(idx) = parts.iter().position(|&x| x == "scrcpy") {
                    if let Some(version) = parts.get(idx + 1) {
                        return Some(version.trim_start_matches('v').to_string());
                    }
                }
            }
        }
    }
    None
}

pub async fn get_latest_scrcpy_version() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("ScrcpyGui-Updater")
        .build()
        .map_err(|e| e.to_string())?;

    let api_url = "https://api.github.com/repos/Genymobile/scrcpy/releases/latest";
    let api_resp = client.get(api_url).send().await;

    let mut tag_name = String::new();
    let mut used_fallback = false;

    if let Ok(resp) = api_resp {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(tag) = json["tag_name"].as_str() {
                    tag_name = tag.trim_start_matches('v').to_string();
                }
            }
        } else if resp.status() == reqwest::StatusCode::FORBIDDEN {
            used_fallback = true;
        }
    } else {
        used_fallback = true;
    }

    if used_fallback || tag_name.is_empty() {
        let redirect_res = client
            .get("https://github.com/Genymobile/scrcpy/releases/latest")
            .send()
            .await
            .map_err(|e| format!("Fallback failed: {}", e))?;

        let final_url = redirect_res.url().as_str();
        if let Some(tag) = final_url.split('/').next_back() {
            if tag.starts_with('v') || tag.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                tag_name = tag.trim_start_matches('v').to_string();
            }
        }
    }

    if tag_name.is_empty() {
        return Err("Could not determine latest version".to_string());
    }

    Ok(tag_name)
}

fn compare_versions(local: &str, remote: &str) -> bool {
    let local_parts: Vec<u32> = local
        .split('.')
        .filter_map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect();
    let remote_parts: Vec<u32> = remote
        .split('.')
        .filter_map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect();

    for i in 0..std::cmp::max(local_parts.len(), remote_parts.len()) {
        let local_val = local_parts.get(i).cloned().unwrap_or(0);
        let remote_val = remote_parts.get(i).cloned().unwrap_or(0);
        if remote_val > local_val {
            return true;
        } else if local_val > remote_val {
            return false;
        }
    }
    false
}

pub async fn check_scrcpy_update(custom_path: Option<String>) -> serde_json::Value {
    // `get_local_scrcpy_version` runs `scrcpy --version` synchronously; keep it
    // off the async reactor.
    let local_version = match tokio::task::spawn_blocking(move || {
        get_local_scrcpy_version(custom_path)
    })
    .await
    {
        Ok(Some(version)) => version,
        _ => {
            return json!({ "update_available": false, "local_version": null, "latest_version": null, "message": "Scrcpy not installed or not working" });
        }
    };

    let latest_version = match get_latest_scrcpy_version().await {
        Ok(v) => v,
        Err(e) => {
            return json!({ "update_available": false, "local_version": local_version, "latest_version": null, "message": format!("Could not fetch latest release: {}", e) });
        }
    };

    let update_available = compare_versions(&local_version, &latest_version);

    json!({
        "update_available": update_available,
        "local_version": local_version,
        "latest_version": latest_version
    })
}
