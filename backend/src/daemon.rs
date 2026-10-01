//! The infer daemon the app drives every chat through: the binding it listens
//! on, starting it when nothing listens, the browser-use settings that put the
//! extension behind it, and the extension's state the daemon reports.
//! Wire contract: cli/docs/browser-extension-protocol.md.

use crate::env::{agent_cwd, home_dir, infer_bin_path, infer_env};
use serde::Serialize;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub(crate) const DEFAULT_PORT: u16 = 52789;
pub(crate) const PROTOCOL_VERSION: u64 = 2;
pub(crate) const EVENT: &str = "browser-bridge";
const DAEMON_FILE: &str = "daemon.yaml";
const BROWSER_USE_FILE: &str = "browser_use.yaml";
const BOOT_WAIT: Duration = Duration::from_secs(15);
const PROBE: Duration = Duration::from_millis(300);

/// Where the daemon's binding listens and the token its clients present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) port: u16,
    pub(crate) token: String,
}

/// The extension's connection to the daemon, as the last
/// `browser_extension_status` frame reported it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExtensionStatus {
    pub(crate) connected: bool,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BridgeStatus {
    pub(crate) enabled: bool,
    pub(crate) connected: bool,
    pub(crate) port: u16,
    pub(crate) token: String,
}

fn read_yaml(path: &Path) -> Option<serde_norway::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_norway::from_str(&text).ok()
}

fn port_of(section: Option<&serde_norway::Value>) -> Option<u16> {
    section?
        .get("port")?
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
}

fn token_of(section: Option<&serde_norway::Value>) -> Option<String> {
    section?
        .get("token")?
        .as_str()
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// The binding from `daemon.yaml`, falling back to `browser_use.yaml`'s
/// extension port and token. Without a token anywhere, `daemon.yaml` is
/// seeded with the binding on and a fresh token.
pub(crate) fn binding() -> Result<Binding, String> {
    binding_in(&home_dir())
}

fn binding_in(home: &Path) -> Result<Binding, String> {
    let daemon_path = home.join(".infer").join(DAEMON_FILE);
    let daemon = read_yaml(&daemon_path).and_then(|v| v.get("binding").cloned());
    let browser = read_yaml(&home.join(".infer").join(BROWSER_USE_FILE))
        .and_then(|v| v.get("extension").cloned());
    let port = port_of(daemon.as_ref())
        .or_else(|| port_of(browser.as_ref()))
        .unwrap_or(DEFAULT_PORT);
    if let Some(token) = token_of(daemon.as_ref()).or_else(|| token_of(browser.as_ref())) {
        return Ok(Binding { port, token });
    }
    let token = hex::encode(rand::random::<[u8; 16]>());
    crate::config::update_file(&daemon_path, |existing| {
        merge_binding(existing, port, &token)
    })?;
    Ok(Binding { port, token })
}

fn merge_binding(existing: Option<&str>, port: u16, token: &str) -> Result<String, String> {
    let mut yaml = crate::config::config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    crate::config::set_section(
        map,
        "binding",
        vec![
            ("enabled", true.into()),
            ("port", u64::from(port).into()),
            ("token", token.into()),
        ],
    );
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

pub(crate) fn reachable(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), PROBE).is_ok()
}

/// Makes sure a daemon listens on the binding: one that already does is
/// reused, else `infer daemon` starts with the app's env and the binding
/// switched on, and the port is awaited.
pub(crate) fn ensure_running(state: &crate::AppState) -> Result<Binding, String> {
    let binding = binding()?;
    if reachable(binding.port) {
        return Ok(binding);
    }
    let log = Arc::clone(&state.scheduler_log);
    let spawn_binding = binding.clone();
    state.processes.restart_scheduler(move || {
        log.lock()
            .map_err(|error| format!("scheduler log mutex poisoned: {error}"))?
            .clear();
        let mut child = std::process::Command::new(infer_bin_path())
            .arg("daemon")
            .current_dir(agent_cwd())
            .envs(infer_env())
            .env("INFER_DAEMON_BINDING_ENABLED", "true")
            .env("INFER_DAEMON_BINDING_PORT", spawn_binding.port.to_string())
            .env("INFER_DAEMON_BINDING_TOKEN", &spawn_binding.token)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start infer daemon: {e}"))?;
        if let Some(stdout) = child.stdout.take() {
            crate::scheduler::pipe_logger(stdout, Arc::clone(&log));
        }
        if let Some(stderr) = child.stderr.take() {
            crate::scheduler::pipe_logger(stderr, log);
        }
        Ok(child)
    })?;
    let deadline = Instant::now() + BOOT_WAIT;
    while !reachable(binding.port) {
        if Instant::now() >= deadline {
            return Err(format!(
                "infer daemon did not start listening on port {} (another daemon may hold the pid file)",
                binding.port
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(binding)
}

/// Stops the daemon the app started and closes every thread, so the next chat
/// starts a daemon that sees the new env (auth keys, an updated binary).
pub(crate) fn restart(state: &crate::AppState) -> Result<(), String> {
    crate::thread::close_all(state);
    state.processes.stop_scheduler()?;
    ensure_running(state).map(|_| ())
}

/// Records the extension's state from a `browser_extension_status` frame and
/// tells every window.
pub(crate) fn extension_status(app: &tauri::AppHandle, status: ExtensionStatus) {
    let state = tauri::Manager::state::<crate::AppState>(app);
    if let Ok(mut current) = state.extension.lock() {
        *current = status;
    }
    let _ = app.emit(EVENT, bridge_status(&state));
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) backend: String,
    pub(crate) port: u16,
    pub(crate) token: String,
}

impl Settings {
    pub(crate) fn active(&self) -> bool {
        self.enabled && self.backend == "extension"
    }
}

fn browser_use_path(home: &Path) -> PathBuf {
    home.join(".infer").join(BROWSER_USE_FILE)
}

fn parse_settings(text: &str) -> Settings {
    let Ok(value) = serde_norway::from_str::<serde_norway::Value>(text) else {
        return Settings::default();
    };
    let ext = value.get("extension");
    Settings {
        enabled: value
            .get("enabled")
            .and_then(serde_norway::Value::as_bool)
            .unwrap_or(false),
        backend: value
            .get("backend")
            .and_then(serde_norway::Value::as_str)
            .unwrap_or("")
            .to_owned(),
        port: port_of(ext).unwrap_or(DEFAULT_PORT),
        token: token_of(ext).unwrap_or_default(),
    }
}

pub(crate) fn read_settings() -> Settings {
    std::fs::read_to_string(browser_use_path(&home_dir()))
        .map(|text| parse_settings(&text))
        .unwrap_or_default()
}

/// Flip `enabled`, and on enable also force `backend: extension` and seed a
/// token if none is set. Every other key the CLI owns is preserved.
fn merge_enabled(existing: Option<&str>, enabled: bool, seed: &str) -> Result<String, String> {
    let mut yaml = crate::config::config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    map.insert("enabled".into(), enabled.into());
    if enabled {
        map.insert("backend".into(), "extension".into());
        let has_token = map
            .get("extension")
            .and_then(|e| e.get("token"))
            .and_then(serde_norway::Value::as_str)
            .is_some_and(|t| !t.is_empty());
        if !has_token {
            crate::config::set_section(map, "extension", vec![("token", seed.into())]);
        }
    }
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn write_enabled(path: &Path, enabled: bool) -> Result<(), String> {
    let seed = hex::encode(rand::random::<[u8; 16]>());
    crate::config::update_file(path, |existing| merge_enabled(existing, enabled, &seed))
}

fn bridge_status(state: &crate::AppState) -> BridgeStatus {
    let settings = read_settings();
    let binding = binding().unwrap_or(Binding {
        port: settings.port,
        token: settings.token.clone(),
    });
    let connected = state.extension.lock().map(|s| s.connected).unwrap_or(false);
    BridgeStatus {
        enabled: settings.active(),
        connected,
        port: binding.port,
        token: binding.token,
    }
}

#[tauri::command]
pub(crate) fn browser_use_status(state: tauri::State<'_, crate::AppState>) -> BridgeStatus {
    bridge_status(&state)
}

/// Switches browser use through the extension on or off in browser_use.yaml.
/// The daemon reads it when it starts, so the one the app owns is restarted.
#[tauri::command]
pub(crate) fn set_browser_use_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<BridgeStatus, String> {
    write_enabled(&browser_use_path(&home_dir()), enabled)?;
    restart(&state)?;
    let current = bridge_status(&state);
    let _ = app.emit(EVENT, current.clone());
    Ok(current)
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_prefers_daemon_yaml_then_browser_use_then_seeds() {
        let home = tempfile_dir();
        let infer = home.join(".infer");
        std::fs::create_dir_all(&infer).unwrap();

        let seeded = binding_in(&home).unwrap();
        assert_eq!(seeded.port, DEFAULT_PORT);
        assert_eq!(seeded.token.len(), 32);
        let written = std::fs::read_to_string(infer.join(DAEMON_FILE)).unwrap();
        assert!(written.contains("enabled: true"));
        assert!(written.contains(&seeded.token));
        assert_eq!(binding_in(&home).unwrap(), seeded);

        std::fs::write(
            infer.join(BROWSER_USE_FILE),
            "enabled: true\nbackend: extension\nextension:\n  port: 6000\n  token: ext\n",
        )
        .unwrap();
        std::fs::remove_file(infer.join(DAEMON_FILE)).unwrap();
        assert_eq!(
            binding_in(&home).unwrap(),
            Binding {
                port: 6000,
                token: "ext".into()
            }
        );

        std::fs::write(
            infer.join(DAEMON_FILE),
            "binding:\n  enabled: true\n  port: 7000\n  token: dae\n",
        )
        .unwrap();
        assert_eq!(
            binding_in(&home).unwrap(),
            Binding {
                port: 7000,
                token: "dae".into()
            }
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn merge_enabled_seeds_token_and_preserves_other_keys() {
        let existing = "enabled: false\nheadless: true\nextension:\n  port: 1234\n";
        let out = merge_enabled(Some(existing), true, "seed").unwrap();
        let v: serde_norway::Value = serde_norway::from_str(&out).unwrap();
        assert_eq!(v["enabled"].as_bool(), Some(true));
        assert_eq!(v["backend"].as_str(), Some("extension"));
        assert_eq!(v["headless"].as_bool(), Some(true));
        assert_eq!(v["extension"]["port"].as_u64(), Some(1234));
        assert_eq!(v["extension"]["token"].as_str(), Some("seed"));

        let kept = merge_enabled(Some(&out), true, "other").unwrap();
        let v: serde_norway::Value = serde_norway::from_str(&kept).unwrap();
        assert_eq!(v["extension"]["token"].as_str(), Some("seed"));

        let off = merge_enabled(Some(&kept), false, "x").unwrap();
        let v: serde_norway::Value = serde_norway::from_str(&off).unwrap();
        assert_eq!(v["enabled"].as_bool(), Some(false));
        assert_eq!(v["extension"]["token"].as_str(), Some("seed"));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "desktop-daemon-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
