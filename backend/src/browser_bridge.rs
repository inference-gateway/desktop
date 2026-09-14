//! Browser-use bridge: a persistent loopback WebSocket server the opentask
//! extension dials into, plus a per-turn relay that dials the child CLI's own
//! extension bridge (moved to RELAY_PORT via INFER_BROWSER_USE_EXTENSION_PORT)
//! and forwards browser_command / browser_result frames between the two.
//! Wire contract: cli/docs/browser-extension-protocol.md.
//!
//! Threading: every WebSocket is owned by exactly one thread running `pump`,
//! which polls the socket with a short read timeout and drains an mpsc queue
//! for outbound frames, so no lock is ever held across socket I/O. `Links` is
//! the single shared lock; `Bridge::running` is taken before `Links` when both
//! are needed.

use crate::env::home_dir;
use serde::Serialize;
use std::io::ErrorKind;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

pub(crate) const RELAY_PORT: u16 = 52790;
const DEFAULT_PORT: u16 = 52789;
const CONFIG_FILE: &str = "browser_use.yaml";
pub(crate) const EVENT: &str = "browser-bridge";
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);
const PING_EVERY: Duration = Duration::from_secs(20);
const RELAY_DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const ALLOWED_ORIGINS: [&str; 3] = [
    "chrome-extension://",
    "moz-extension://",
    "safari-web-extension://",
];

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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BridgeStatus {
    pub(crate) enabled: bool,
    pub(crate) connected: bool,
    pub(crate) port: u16,
    pub(crate) token: String,
}

fn config_path(home: &Path) -> PathBuf {
    home.join(".infer").join(CONFIG_FILE)
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
        port: ext
            .and_then(|e| e.get("port"))
            .and_then(serde_norway::Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .unwrap_or(DEFAULT_PORT),
        token: ext
            .and_then(|e| e.get("token"))
            .and_then(serde_norway::Value::as_str)
            .unwrap_or("")
            .to_owned(),
    }
}

pub(crate) fn read_settings() -> Settings {
    std::fs::read_to_string(config_path(&home_dir()))
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(path).ok();
    let seed = hex::encode(rand::random::<[u8; 16]>());
    let merged = merge_enabled(existing.as_deref(), enabled, &seed)?;
    std::fs::write(path, merged).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Peer {
    Extension,
    Cli,
}

fn frame_type(frame: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(frame).ok()?;
    value.get("type")?.as_str().map(str::to_owned)
}

/// Which peer a frame is relayed to; `None` means drop it (panel chat frames,
/// keepalive pings, anything unknown).
fn relay_target(frame: &str) -> Option<Peer> {
    match frame_type(frame)?.as_str() {
        "browser_command" => Some(Peer::Extension),
        "browser_result" => Some(Peer::Cli),
        _ => None,
    }
}

fn hello_token(frame: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(frame).ok()?;
    if value.get("type")?.as_str()? != "browser_hello" {
        return None;
    }
    value.get("token")?.as_str().map(str::to_owned)
}

fn token_matches(got: &str, want: &str) -> bool {
    if got.len() != want.len() || want.is_empty() {
        return false;
    }
    got.bytes()
        .zip(want.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn no_extension_result(command: &str, port: u16) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(command).ok()?;
    let id = value.get("id")?.as_str()?;
    Some(
        serde_json::json!({
            "id": id,
            "error": format!("no browser extension connected on port {port} - install the opentask extension and set its bridge port/token to match the desktop Settings"),
        })
        .to_string(),
    )
}

fn hello_frame(token: &str) -> String {
    serde_json::json!({ "type": "browser_hello", "token": token, "extension_version": "desktop" })
        .to_string()
}

const HELLO_ACK: &str = r#"{"type":"browser_hello_ack"}"#;

struct PeerHandle {
    tx: mpsc::Sender<String>,
    sock: TcpStream,
    id: Option<SocketAddr>,
}

impl PeerHandle {
    fn close(&self) {
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

#[derive(Default)]
struct Links {
    ext: Option<PeerHandle>,
    cli: Option<PeerHandle>,
}

type Notify = Arc<dyn Fn(bool) + Send + Sync>;

struct Running {
    addr: SocketAddr,
    settings: Settings,
    links: Arc<Mutex<Links>>,
    stop: Arc<AtomicBool>,
    accept: JoinHandle<()>,
    notify: Notify,
}

#[derive(Default)]
pub(crate) struct Bridge {
    running: Mutex<Option<Running>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Own `ws` until it closes: deliver inbound text frames to `on_frame`, write
/// queued outbound frames, ping every PING_EVERY.
fn pump(mut ws: WebSocket<TcpStream>, rx: mpsc::Receiver<String>, mut on_frame: impl FnMut(&str)) {
    let _ = ws.get_ref().set_read_timeout(Some(POLL));
    let mut last_ping = Instant::now();
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => on_frame(&text),
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
        loop {
            match rx.try_recv() {
                Ok(frame) => {
                    if ws.send(Message::text(frame)).is_err() {
                        return;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            if ws.send(Message::Ping(Vec::new().into())).is_err() {
                return;
            }
            last_ping = Instant::now();
        }
    }
}

#[allow(clippy::result_large_err)]
fn check_request(
    request: &tungstenite::handshake::server::Request,
    response: tungstenite::handshake::server::Response,
) -> Result<tungstenite::handshake::server::Response, tungstenite::handshake::server::ErrorResponse>
{
    let origin_ok = match request
        .headers()
        .get("origin")
        .and_then(|o| o.to_str().ok())
    {
        None => true,
        Some(origin) => ALLOWED_ORIGINS.iter().any(|p| origin.starts_with(p)),
    };
    if request.uri().path() == "/ws" && origin_ok {
        return Ok(response);
    }
    Err(tungstenite::http::Response::builder()
        .status(403)
        .body(None)
        .expect("static 403 response"))
}

fn serve_extension(stream: TcpStream, token: &str, links: &Arc<Mutex<Links>>, notify: &Notify) {
    let Ok(sock) = stream.try_clone() else { return };
    let id = sock.peer_addr().ok();
    let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
    let Ok(mut ws) = tungstenite::accept_hdr(stream, check_request) else {
        return;
    };
    let authed = match ws.read() {
        Ok(Message::Text(text)) => hello_token(&text).is_some_and(|got| token_matches(&got, token)),
        _ => false,
    };
    if !authed {
        let _ = ws.close(None);
        return;
    }
    if ws.send(Message::text(HELLO_ACK)).is_err() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    if let Some(old) = lock(links).ext.replace(PeerHandle { tx, sock, id }) {
        old.close();
    }
    notify(true);
    pump(ws, rx, |frame| {
        if relay_target(frame) != Some(Peer::Cli) {
            return;
        }
        let cli_tx = lock(links).cli.as_ref().map(|h| h.tx.clone());
        if let Some(tx) = cli_tx {
            let _ = tx.send(frame.to_owned());
        }
    });
    let mut l = lock(links);
    if l.ext.as_ref().is_some_and(|h| h.id == id) {
        l.ext = None;
        drop(l);
        notify(false);
    }
}

fn dial(port: u16, stop: &AtomicBool) -> Option<WebSocket<TcpStream>> {
    let deadline = Instant::now() + RELAY_DIAL_TIMEOUT;
    loop {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
            if let Ok((ws, _)) = tungstenite::client(format!("ws://127.0.0.1:{port}/ws"), stream) {
                return Some(ws);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn relay_cli(port: u16, settings: &Settings, links: &Arc<Mutex<Links>>, stop: &AtomicBool) {
    let Some(mut ws) = dial(port, stop) else {
        return;
    };
    if ws
        .send(Message::text(hello_frame(&settings.token)))
        .is_err()
    {
        return;
    }
    match ws.read() {
        Ok(Message::Text(text)) if frame_type(&text).as_deref() == Some("browser_hello_ack") => {}
        _ => return,
    }
    let Ok(sock) = ws.get_ref().try_clone() else {
        return;
    };
    let id = sock.local_addr().ok();
    let (tx, rx) = mpsc::channel();
    let own_tx = tx.clone();
    if let Some(old) = lock(links).cli.replace(PeerHandle { tx, sock, id }) {
        old.close();
    }
    let ext_port = settings.port;
    pump(ws, rx, |frame| {
        if relay_target(frame) != Some(Peer::Extension) {
            return;
        }
        let ext_tx = lock(links).ext.as_ref().map(|h| h.tx.clone());
        match ext_tx {
            Some(tx) => {
                let _ = tx.send(frame.to_owned());
            }
            None => {
                if let Some(reply) = no_extension_result(frame, ext_port) {
                    let _ = own_tx.send(reply);
                }
            }
        }
    });
    let mut l = lock(links);
    if l.cli.as_ref().is_some_and(|h| h.id == id) {
        l.cli = None;
    }
}

impl Bridge {
    pub(crate) fn start(&self, settings: Settings, notify: Notify) -> Result<u16, String> {
        if settings.token.is_empty() {
            return Err("browser_use.extension.token is empty".into());
        }
        self.stop();
        let listener = TcpListener::bind(("127.0.0.1", settings.port)).map_err(|e| {
            format!(
                "browser bridge failed to listen on port {}: {e}",
                settings.port
            )
        })?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let links: Arc<Mutex<Links>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let accept = {
            let (links, stop, notify, token) = (
                Arc::clone(&links),
                Arc::clone(&stop),
                Arc::clone(&notify),
                settings.token.clone(),
            );
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (links, notify, token) =
                        (Arc::clone(&links), Arc::clone(&notify), token.clone());
                    std::thread::spawn(move || serve_extension(stream, &token, &links, &notify));
                }
            })
        };
        *lock(&self.running) = Some(Running {
            addr,
            settings,
            links,
            stop,
            accept,
            notify,
        });
        Ok(addr.port())
    }

    pub(crate) fn stop(&self) {
        let Some(running) = lock(&self.running).take() else {
            return;
        };
        running.stop.store(true, Ordering::SeqCst);
        {
            let mut l = lock(&running.links);
            if let Some(h) = l.ext.take() {
                h.close();
            }
            if let Some(h) = l.cli.take() {
                h.close();
            }
        }
        let _ = TcpStream::connect(running.addr);
        let _ = running.accept.join();
        (running.notify)(false);
    }

    pub(crate) fn connected(&self) -> bool {
        lock(&self.running)
            .as_ref()
            .is_some_and(|r| lock(&r.links).ext.is_some())
    }

    /// Dial the CLI bridge of the turn just spawned and relay until it closes.
    pub(crate) fn connect_relay(&self) {
        self.connect_relay_to(RELAY_PORT);
    }

    fn connect_relay_to(&self, port: u16) {
        let snapshot = lock(&self.running).as_ref().map(|r| {
            (
                r.settings.clone(),
                Arc::clone(&r.links),
                Arc::clone(&r.stop),
            )
        });
        let Some((settings, links, stop)) = snapshot else {
            return;
        };
        std::thread::spawn(move || relay_cli(port, &settings, &links, &stop));
    }
}

fn status(bridge: &Bridge) -> BridgeStatus {
    let settings = read_settings();
    BridgeStatus {
        enabled: settings.active(),
        connected: bridge.connected(),
        port: settings.port,
        token: settings.token,
    }
}

fn notifier(app: tauri::AppHandle) -> Notify {
    Arc::new(move |connected| {
        use tauri::Emitter;
        let settings = read_settings();
        let _ = app.emit(
            EVENT,
            BridgeStatus {
                enabled: settings.active(),
                connected,
                port: settings.port,
                token: settings.token,
            },
        );
    })
}

/// Start the extension server if browser use is enabled; a bind failure is
/// reported, not fatal, so the app still launches.
pub(crate) fn start_if_enabled(bridge: &Bridge, app: tauri::AppHandle) {
    let settings = read_settings();
    if !settings.active() {
        return;
    }
    if let Err(e) = bridge.start(settings, notifier(app)) {
        eprintln!("browser bridge autostart failed: {e}");
    }
}

#[tauri::command]
pub(crate) fn browser_use_status(state: tauri::State<'_, crate::AppState>) -> BridgeStatus {
    status(&state.browser_bridge)
}

#[tauri::command]
pub(crate) fn set_browser_use_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<BridgeStatus, String> {
    write_enabled(&config_path(&home_dir()), enabled)?;
    let bridge = &state.browser_bridge;
    if enabled {
        bridge.start(read_settings(), notifier(app))?;
    } else {
        bridge.stop();
    }
    Ok(status(bridge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_target_passes_browser_frames_and_drops_the_rest() {
        assert_eq!(
            relay_target(r#"{"type":"browser_command","id":"1","action":"tabs"}"#),
            Some(Peer::Extension)
        );
        assert_eq!(
            relay_target(r#"{"type":"browser_result","id":"1","error":""}"#),
            Some(Peer::Cli)
        );
        for dropped in [
            r#"{"type":"ping"}"#,
            r#"{"type":"chat_event","event":{}}"#,
            r#"{"type":"list_conversations"}"#,
            r#"{"type":"browser_hello_ack"}"#,
            "not json",
        ] {
            assert_eq!(relay_target(dropped), None, "{dropped}");
        }
    }

    #[test]
    fn hello_gate_requires_matching_token() {
        let hello = r#"{"type":"browser_hello","token":"s3cret","extension_version":"1.0"}"#;
        assert_eq!(hello_token(hello).as_deref(), Some("s3cret"));
        assert_eq!(hello_token(r#"{"type":"ping","token":"s3cret"}"#), None);
        assert!(token_matches("s3cret", "s3cret"));
        assert!(!token_matches("s3cret", "s3cre7"));
        assert!(!token_matches("s3cret", "s3cre"));
        assert!(!token_matches("", ""));
    }

    #[test]
    fn merge_enabled_seeds_token_and_preserves_other_keys() {
        let existing = "browser:\n  timeout_seconds: 45\nextension:\n  port: 6000\n";
        let enabled = merge_enabled(Some(existing), true, "abc").unwrap();
        let s = parse_settings(&enabled);
        assert!(s.active());
        assert_eq!(s.port, 6000);
        assert_eq!(s.token, "abc");
        assert!(enabled.contains("timeout_seconds: 45"));

        let disabled = merge_enabled(Some(&enabled), false, "zzz").unwrap();
        let s = parse_settings(&disabled);
        assert!(!s.enabled);
        assert_eq!(s.token, "abc");
        assert_eq!(parse_settings("").port, DEFAULT_PORT);
    }

    fn connect_ext(port: u16, hello: &str) -> WebSocket<TcpStream> {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (mut ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/ws"), stream).unwrap();
        ws.send(Message::text(hello)).unwrap();
        ws
    }

    fn read_text(ws: &mut WebSocket<TcpStream>) -> Option<String> {
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => return Some(t.to_string()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    #[test]
    fn bridge_relays_between_extension_and_cli() {
        let bridge = Bridge::default();
        let settings = Settings {
            enabled: true,
            backend: "extension".into(),
            port: 0,
            token: "tok".into(),
        };
        let port = bridge.start(settings, Arc::new(|_| {})).unwrap();

        let mut bad = connect_ext(port, r#"{"type":"browser_hello","token":"nope"}"#);
        assert_eq!(read_text(&mut bad), None);
        assert!(!bridge.connected());

        let mut ext = connect_ext(port, r#"{"type":"browser_hello","token":"tok"}"#);
        assert_eq!(read_text(&mut ext).as_deref(), Some(HELLO_ACK));
        assert!(bridge.connected());

        let fake_cli = TcpListener::bind("127.0.0.1:0").unwrap();
        bridge.connect_relay_to(fake_cli.local_addr().unwrap().port());
        let (stream, _) = fake_cli.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut cli = tungstenite::accept(stream).unwrap();
        assert_eq!(
            hello_token(&read_text(&mut cli).unwrap()).as_deref(),
            Some("tok")
        );
        cli.send(Message::text(HELLO_ACK)).unwrap();

        cli.send(Message::text(r#"{"type":"chat_event","event":{}}"#))
            .unwrap();
        cli.send(Message::text(
            r#"{"type":"browser_command","id":"c1","action":"tabs"}"#,
        ))
        .unwrap();
        let got = read_text(&mut ext).unwrap();
        assert_eq!(relay_target(&got), Some(Peer::Extension));
        ext.send(Message::text(r#"{"type":"list_conversations"}"#))
            .unwrap();
        ext.send(Message::text(
            r#"{"type":"browser_result","id":"c1","error":""}"#,
        ))
        .unwrap();
        let back = read_text(&mut cli).unwrap();
        assert!(back.contains(r#""id":"c1""#));

        drop(ext);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!bridge.connected());
        cli.send(Message::text(
            r#"{"type":"browser_command","id":"c2","action":"tabs"}"#,
        ))
        .unwrap();
        let err = read_text(&mut cli).unwrap();
        assert!(err.contains(r#""id":"c2""#) && err.contains("no browser extension"));

        bridge.stop();
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }
}
