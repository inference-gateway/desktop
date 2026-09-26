//! Browser-use bridge: a persistent loopback WebSocket server the opentask
//! extension dials into, plus a per-turn relay that dials the child CLI's own
//! extension bridge (moved to RELAY_PORT via INFER_BROWSER_USE_EXTENSION_PORT)
//! and forwards browser_command / browser_result frames between the two.
//! The extension's side-panel frames (conversations, chat, models, mode,
//! approvals) are answered by the desktop itself from the same conversation
//! store and agent sessions the sidebar uses.
//! Wire contract: cli/docs/browser-extension-protocol.md.
//!
//! Threading: every WebSocket is owned by exactly one thread running `pump`,
//! which polls the socket with a short read timeout and drains an mpsc queue
//! for outbound frames, so no lock is ever held across socket I/O. `Links` and
//! `Panel` are the only shared locks; lock order is `Bridge::running` ->
//! `Links` -> `Panel`, and none is held while calling out.

use crate::env::{agent_cwd, home_dir, infer_bin_path, infer_env};
use serde::Serialize;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::Emitter;
use tungstenite::{Message, WebSocket};

pub(crate) const RELAY_PORT: u16 = 52790;
const DEFAULT_PORT: u16 = 52789;
const CONFIG_FILE: &str = "browser_use.yaml";
pub(crate) const EVENT: &str = "browser-bridge";
/// Frontend-bound commands originating from the extension's side panel.
const PANEL_EVENT: &str = "browser-panel";
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);
const PING_EVERY: Duration = Duration::from_secs(20);
const RELAY_DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const CONVERSATION_LIMIT: usize = 50;
const TOOL_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);
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
    let seed = hex::encode(rand::random::<[u8; 16]>());
    crate::config::update_file(path, |existing| merge_enabled(existing, enabled, &seed))
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

/// Which peer a frame is relayed to verbatim; `None` means it is either a
/// side-panel frame the desktop answers itself or noise to drop.
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

/// Persist one side-panel attachment (`{filename, mime_type, data}`) and return
/// the `[Attached image: <path>]` line the agent already understands.
fn save_panel_attachment(a: &serde_json::Value) -> Option<String> {
    let mime = a.get("mime_type")?.as_str()?.to_owned();
    let data = a.get("data")?.as_str()?.to_owned();
    match crate::agent::save_upload(data, mime) {
        Ok(path) => Some(format!("[Attached image: {path}]")),
        Err(e) => {
            let name = a
                .get("filename")
                .and_then(|x| x.as_str())
                .unwrap_or("attachment");
            eprintln!("browser bridge: dropped attachment {name}: {e}");
            None
        }
    }
}

fn new_session_id() -> String {
    let b = hex::encode(rand::random::<[u8; 16]>());
    format!(
        "{}-{}-{}-{}-{}",
        &b[0..8],
        &b[8..12],
        &b[12..16],
        &b[16..20],
        &b[20..32]
    )
}

/// Arguments for `infer tools execute` answering an extension `tool_request`.
fn tool_exec_args(name: &str, args: &str, session: Option<&str>, approved: bool) -> Vec<String> {
    let mut out: Vec<String> = ["tools", "execute", name, args, "--format", "json"]
        .map(String::from)
        .into();
    if let Some(id) = session {
        out.extend(["--session-id".into(), id.into()]);
    }
    if approved {
        out.push("--approved".into());
    }
    out
}

/// What `infer tools execute --format json` reported.
#[derive(Debug, PartialEq)]
enum ToolOutcome {
    NeedsApproval,
    Done {
        success: bool,
        output: String,
        error: String,
    },
}

fn parse_tool_outcome(stdout: &str) -> Result<ToolOutcome, String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("unexpected infer tools output: {e}"))?;
    if v["approval_required"].as_bool() == Some(true) {
        return Ok(ToolOutcome::NeedsApproval);
    }
    let text = |key: &str| v[key].as_str().unwrap_or("").to_owned();
    Ok(ToolOutcome::Done {
        success: v["success"].as_bool() == Some(true),
        output: text("output"),
        error: text("error"),
    })
}

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

/// What the side panel is looking at: the desktop session its transcript
/// mirrors, the project cwd of every conversation it has listed, and the
/// agent mode it last chose.
#[derive(Default)]
struct Panel {
    current: Option<String>,
    projects: HashMap<String, String>,
    auto: bool,
}

/// Everything a connection thread needs, shared behind one Arc.
struct Host {
    settings: Settings,
    links: Mutex<Links>,
    panel: Mutex<Panel>,
    pending_tools: Mutex<HashMap<String, mpsc::Sender<bool>>>,
    stop: AtomicBool,
    app: Option<tauri::AppHandle>,
    processes: Option<Arc<crate::process_manager::ProcessManager>>,
}

struct Running {
    addr: SocketAddr,
    host: Arc<Host>,
    accept: JoinHandle<()>,
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

impl Host {
    fn notify(&self, connected: bool) {
        let Some(app) = &self.app else { return };
        let _ = app.emit(
            EVENT,
            BridgeStatus {
                enabled: true,
                connected,
                port: self.settings.port,
                token: self.settings.token.clone(),
            },
        );
    }

    fn to_ext(&self, frame: String) {
        let tx = lock(&self.links).ext.as_ref().map(|h| h.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.send(frame);
        }
    }

    fn to_cli(&self, frame: String) {
        let tx = lock(&self.links).cli.as_ref().map(|h| h.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.send(frame);
        }
    }

    fn to_panel_ui(&self, payload: serde_json::Value) {
        if let Some(app) = &self.app {
            let _ = app.emit(PANEL_EVENT, payload);
        }
    }

    fn send_json(&self, value: serde_json::Value) {
        self.to_ext(value.to_string());
    }

    fn send_snapshot(&self, id: Option<&str>) {
        let (messages, tool_results) = id.map(|id| self.snapshot(id)).unwrap_or_default();
        self.send_json(serde_json::json!({
            "type": "conversation_snapshot",
            "messages": messages,
            "tool_results": tool_results,
        }));
    }

    /// The CLI's per-cwd JSONL store, read directly (ponytail: a flat
    /// `storage.directory` override is not resolved - the panel then shows an
    /// empty transcript; route through `infer conversations show` if needed).
    fn snapshot(
        &self,
        id: &str,
    ) -> (
        Vec<serde_json::Value>,
        serde_json::Map<String, serde_json::Value>,
    ) {
        let cwd = lock(&self.panel)
            .projects
            .get(id)
            .map(PathBuf::from)
            .unwrap_or_else(agent_cwd);
        let path = crate::agent::conversation_store_dir(&cwd).join(format!("{id}.jsonl"));
        let Ok(text) = std::fs::read_to_string(path) else {
            return Default::default();
        };
        let mut messages = Vec::new();
        let mut tool_results = serde_json::Map::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(entry) = v.get("entry") else {
                continue;
            };
            let Some(m) = entry.get("message") else {
                continue;
            };
            if let (Some(call), Some(exec)) = (
                m.get("tool_call_id").and_then(|c| c.as_str()),
                entry.get("tool_execution"),
            ) {
                let ok = exec
                    .get("success")
                    .and_then(|s| s.as_bool())
                    .unwrap_or(false);
                tool_results.insert(call.to_owned(), ok.into());
            }
            messages.push(m.clone());
        }
        (messages, tool_results)
    }

    fn send_conversations(&self) {
        let output = std::process::Command::new(infer_bin_path())
            .args([
                "conversations",
                "list",
                "--all-projects",
                "--format",
                "json",
            ])
            .env("HOME", home_dir())
            .envs(infer_env())
            .current_dir(agent_cwd())
            .output();
        let list = output
            .ok()
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
            .and_then(|v| v.get("conversations").and_then(|c| c.as_array()).cloned())
            .unwrap_or_default();
        let mut panel = lock(&self.panel);
        for c in &list {
            if let (Some(id), Some(project)) = (
                c.get("id").and_then(|i| i.as_str()),
                c.get("project").and_then(|p| p.as_str()),
            ) {
                panel.projects.insert(id.to_owned(), project.to_owned());
            }
        }
        drop(panel);
        let conversations: Vec<serde_json::Value> =
            list.into_iter().take(CONVERSATION_LIMIT).collect();
        self.send_json(
            serde_json::json!({ "type": "conversations", "conversations": conversations }),
        );
    }

    fn send_models(&self) {
        let models = crate::agent::fetch_models().unwrap_or_default();
        let current = crate::config::read_config().default_model;
        self.send_json(
            serde_json::json!({ "type": "models", "models": models, "current": current }),
        );
    }

    fn send_mode(&self) {
        let mode = if lock(&self.panel).auto {
            "auto"
        } else {
            "standard"
        };
        self.send_json(serde_json::json!({ "type": "mode", "mode": mode }));
    }

    fn send_approval(&self, request_id: &str, approved: bool) {
        let Some(session) = lock(&self.panel).current.clone() else {
            return;
        };
        let line = format!(
            "{}\n",
            serde_json::json!({ "type": "approval_response", "tool_call_id": request_id, "approved": approved })
        );
        if let Some(p) = &self.processes {
            let _ = p.write_agent(&session, line.as_bytes());
        }
        if let Some(app) = &self.app {
            let status = if approved { "approved" } else { "denied" };
            let _ = app.emit(
                "approval-resolved",
                serde_json::json!({ "sessionId": session, "callId": request_id, "status": status }),
            );
        }
        self.send_json(
            serde_json::json!({ "type": "approval_resolved", "request_id": request_id }),
        );
    }

    /// Answer an extension-initiated tool call the way the CLI bridge does:
    /// the approval policy first (the panel is the prompt, auto mode skips
    /// it), then the run as user-approved, recorded in the panel's session.
    fn run_tool_request(&self, id: &str, name: &str, args: &str) {
        let (session, cwd, auto) = {
            let panel = lock(&self.panel);
            let session = panel.current.clone();
            let cwd = session
                .as_ref()
                .and_then(|s| panel.projects.get(s).cloned());
            (session, cwd, panel.auto)
        };
        let cwd = cwd.or_else(|| {
            session
                .as_deref()
                .and_then(crate::projects::assigned_dir)
                .map(|dir| dir.to_string_lossy().into_owned())
        });
        let run = |approved: bool| {
            let argv = tool_exec_args(name, args, session.as_deref(), approved);
            crate::agent::run_infer_blocking(cwd.clone(), &argv)
                .and_then(|out| parse_tool_outcome(&out))
        };
        let outcome = match run(auto) {
            Ok(ToolOutcome::NeedsApproval) if self.await_tool_approval(name, args) => run(true),
            Ok(ToolOutcome::NeedsApproval) => Err("tool call denied".into()),
            other => other,
        };
        let (success, output, error) = match outcome {
            Ok(ToolOutcome::Done {
                success,
                output,
                error,
            }) => (success, output, error),
            Ok(ToolOutcome::NeedsApproval) => (false, String::new(), "tool call denied".into()),
            Err(e) => (false, String::new(), e),
        };
        self.send_json(serde_json::json!({
            "type": "tool_result", "id": id, "success": success, "output": output, "error": error,
        }));
    }

    /// Ask the panel to approve a tool call and wait for its answer. Anything
    /// but an explicit approve within the timeout is a denial.
    fn await_tool_approval(&self, name: &str, args: &str) -> bool {
        let request_id = new_session_id();
        let (tx, rx) = mpsc::channel();
        lock(&self.pending_tools).insert(request_id.clone(), tx);
        self.send_json(serde_json::json!({
            "type": "approval_request", "request_id": request_id,
            "tool_name": name, "tool_args": args,
        }));
        let approved = rx.recv_timeout(TOOL_APPROVAL_TIMEOUT).unwrap_or(false);
        lock(&self.pending_tools).remove(&request_id);
        approved
    }

    /// Resolve an approval_response that belongs to a pending tool_request.
    /// Returns false when the id is not ours.
    fn answer_tool_approval(&self, request_id: &str, approved: bool) -> bool {
        let Some(tx) = lock(&self.pending_tools).remove(request_id) else {
            return false;
        };
        let _ = tx.send(approved);
        self.send_json(
            serde_json::json!({ "type": "approval_resolved", "request_id": request_id }),
        );
        true
    }

    /// Answer a side-panel frame. Anything that shells out runs on its own
    /// thread so the extension pump keeps relaying browser frames meanwhile.
    fn handle_panel_frame(self: &Arc<Self>, frame: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(frame) else {
            return;
        };
        let text = |key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let host = Arc::clone(self);
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "list_conversations" => {
                std::thread::spawn(move || host.send_conversations());
            }
            "list_models" => {
                std::thread::spawn(move || host.send_models());
            }
            "list_skills" => {
                let skills: Vec<serde_json::Value> = crate::skills::list_installed_skills()
                    .into_iter()
                    .map(|name| {
                        serde_json::json!({ "name": name, "description": "", "scope": "user" })
                    })
                    .collect();
                self.send_json(serde_json::json!({ "type": "skills", "skills": skills }));
            }
            "list_history" => {
                let history = crate::agent::read_history(None).unwrap_or_default();
                self.send_json(serde_json::json!({ "type": "history", "history": history }));
            }
            "resume_conversation" => {
                let id = text("id");
                if id.is_empty() {
                    return;
                }
                lock(&self.panel).current = Some(id.clone());
                self.to_panel_ui(serde_json::json!({ "kind": "open", "id": id }));
                std::thread::spawn(move || host.send_snapshot(Some(&id)));
            }
            "new_session" => {
                let id = new_session_id();
                lock(&self.panel).current = Some(id.clone());
                self.to_panel_ui(serde_json::json!({ "kind": "new", "id": id }));
                self.send_snapshot(None);
            }
            "user_message" => {
                let refs: Vec<String> = v["attachments"]
                    .as_array()
                    .map(|a| a.iter().filter_map(save_panel_attachment).collect())
                    .unwrap_or_default();
                let content = if refs.is_empty() {
                    text("content")
                } else {
                    format!("{}\n{}", text("content"), refs.join("\n"))
                };
                let id = lock(&self.panel)
                    .current
                    .get_or_insert_with(new_session_id)
                    .clone();
                self.to_panel_ui(serde_json::json!({
                    "kind": "send", "sessionId": id, "text": content,
                }));
            }
            "interrupt" => {
                let current = lock(&self.panel).current.clone();
                if let (Some(p), Some(id)) = (&self.processes, current) {
                    let _ = p.cancel_agent(&id);
                }
                self.send_json(serde_json::json!({ "type": "interrupted" }));
            }
            "select_model" => {
                let model = text("model");
                let _ = crate::config::update_config_file(|existing| {
                    crate::config::merge_default_model(existing, &model)
                });
                self.to_panel_ui(serde_json::json!({ "kind": "select_model", "model": model }));
                std::thread::spawn(move || {
                    host.send_models();
                    host.send_mode();
                });
            }
            "set_mode" => {
                let auto = text("mode") == "auto";
                lock(&self.panel).auto = auto;
                self.to_panel_ui(serde_json::json!({ "kind": "set_mode", "auto": auto }));
                self.send_mode();
            }
            "approval_response" => {
                let (id, approved) = (text("request_id"), text("action") == "approve");
                if !self.answer_tool_approval(&id, approved) {
                    self.send_approval(&id, approved);
                }
            }
            "tool_request" => {
                let (id, name, args) = (text("id"), text("tool_name"), text("tool_args"));
                std::thread::spawn(move || host.run_tool_request(&id, &name, &args));
            }
            _ => {}
        }
    }
}

fn serve_extension(stream: TcpStream, host: &Arc<Host>) {
    let Ok(sock) = stream.try_clone() else { return };
    let id = sock.peer_addr().ok();
    let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
    let Ok(mut ws) = tungstenite::accept_hdr(stream, check_request) else {
        return;
    };
    let authed = match ws.read() {
        Ok(Message::Text(text)) => {
            hello_token(&text).is_some_and(|got| token_matches(&got, &host.settings.token))
        }
        _ => false,
    };
    if !authed {
        let _ = ws.close(None);
        return;
    }
    let (tx, rx) = mpsc::channel();
    if let Some(old) = lock(&host.links).ext.replace(PeerHandle { tx, sock, id }) {
        old.close();
    }
    host.notify(true);
    let _ = ws.send(Message::text(HELLO_ACK));
    pump(ws, rx, |frame| match relay_target(frame) {
        Some(Peer::Cli) => host.to_cli(frame.to_owned()),
        Some(Peer::Extension) => {}
        None => host.handle_panel_frame(frame),
    });
    let mut l = lock(&host.links);
    if l.ext.as_ref().is_some_and(|h| h.id == id) {
        l.ext = None;
        drop(l);
        host.notify(false);
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

fn relay_cli(port: u16, host: &Arc<Host>) {
    let Some(mut ws) = dial(port, &host.stop) else {
        return;
    };
    if ws
        .send(Message::text(hello_frame(&host.settings.token)))
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
    if let Some(old) = lock(&host.links).cli.replace(PeerHandle { tx, sock, id }) {
        old.close();
    }
    let ext_port = host.settings.port;
    pump(ws, rx, |frame| {
        if relay_target(frame) != Some(Peer::Extension) {
            return;
        }
        let ext_tx = lock(&host.links).ext.as_ref().map(|h| h.tx.clone());
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
    let mut l = lock(&host.links);
    if l.cli.as_ref().is_some_and(|h| h.id == id) {
        l.cli = None;
    }
}

impl Bridge {
    pub(crate) fn start(
        &self,
        settings: Settings,
        app: Option<tauri::AppHandle>,
        processes: Option<Arc<crate::process_manager::ProcessManager>>,
    ) -> Result<u16, String> {
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
        let host = Arc::new(Host {
            settings,
            links: Mutex::default(),
            panel: Mutex::default(),
            pending_tools: Mutex::default(),
            stop: AtomicBool::new(false),
            app,
            processes,
        });
        let accept = {
            let host = Arc::clone(&host);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if host.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let host = Arc::clone(&host);
                    std::thread::spawn(move || serve_extension(stream, &host));
                }
            })
        };
        *lock(&self.running) = Some(Running { addr, host, accept });
        Ok(addr.port())
    }

    pub(crate) fn stop(&self) {
        let Some(running) = lock(&self.running).take() else {
            return;
        };
        running.host.stop.store(true, Ordering::SeqCst);
        {
            let mut l = lock(&running.host.links);
            if let Some(h) = l.ext.take() {
                h.close();
            }
            if let Some(h) = l.cli.take() {
                h.close();
            }
        }
        let _ = TcpStream::connect(running.addr);
        let _ = running.accept.join();
        running.host.notify(false);
    }

    fn host(&self) -> Option<Arc<Host>> {
        lock(&self.running).as_ref().map(|r| Arc::clone(&r.host))
    }

    pub(crate) fn connected(&self) -> bool {
        self.host().is_some_and(|h| lock(&h.links).ext.is_some())
    }

    /// Dial the CLI bridge of the turn just spawned and relay until it closes.
    pub(crate) fn connect_relay(&self) {
        self.connect_relay_to(RELAY_PORT);
    }

    fn connect_relay_to(&self, port: u16) {
        let Some(host) = self.host() else { return };
        std::thread::spawn(move || relay_cli(port, &host));
    }

    fn for_session(&self, session_id: &str) -> Option<Arc<Host>> {
        let host = self.host()?;
        let matches = lock(&host.panel).current.as_deref() == Some(session_id);
        matches.then_some(host)
    }

    /// Mirror one raw AG-UI line of the panel's session to the extension.
    pub(crate) fn chat_event(&self, session_id: &str, line: &str) {
        let Some(host) = self.for_session(session_id) else {
            return;
        };
        if !line.trim_start().starts_with('{') {
            return;
        }
        host.to_ext(format!(r#"{{"type":"chat_event","event":{line}}}"#));
    }

    /// Echo the user's turn the way the CLI does, so the panel shows it.
    pub(crate) fn user_turn(&self, session_id: &str, prompt: &str) {
        let Some(host) = self.for_session(session_id) else {
            return;
        };
        let id = new_session_id();
        for event in [
            serde_json::json!({ "type": "TEXT_MESSAGE_START", "messageId": id, "role": "user" }),
            serde_json::json!({ "type": "TEXT_MESSAGE_CONTENT", "messageId": id, "delta": prompt }),
            serde_json::json!({ "type": "TEXT_MESSAGE_END", "messageId": id }),
        ] {
            host.to_ext(serde_json::json!({ "type": "chat_event", "event": event }).to_string());
        }
    }

    pub(crate) fn approval_request(&self, session_id: &str, call_id: &str, tool: &str, args: &str) {
        let Some(host) = self.for_session(session_id) else {
            return;
        };
        host.send_json(serde_json::json!({
            "type": "approval_request", "request_id": call_id, "tool_name": tool, "tool_args": args,
        }));
    }

    /// The desktop UI answered an approval; tell the panel to drop its card.
    pub(crate) fn approval_resolved(&self, call_id: &str) {
        if let Some(host) = self.host() {
            host.send_json(
                serde_json::json!({ "type": "approval_resolved", "request_id": call_id }),
            );
        }
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

fn start_with_app(state: &crate::AppState, app: tauri::AppHandle) -> Result<u16, String> {
    state.browser_bridge.start(
        read_settings(),
        Some(app),
        Some(Arc::clone(&state.processes)),
    )
}

/// Start the extension server if browser use is enabled; a bind failure is
/// reported, not fatal, so the app still launches.
pub(crate) fn start_if_enabled(state: &crate::AppState, app: tauri::AppHandle) {
    if !read_settings().active() {
        return;
    }
    if let Err(e) = start_with_app(state, app) {
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
    if enabled {
        start_with_app(&state, app.clone())?;
    } else {
        state.browser_bridge.stop();
    }
    let current = status(&state.browser_bridge);
    let _ = app.emit(EVENT, current.clone());
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_exec_args_pass_session_and_approval() {
        assert_eq!(
            tool_exec_args("Bash", "{}", None, false),
            ["tools", "execute", "Bash", "{}", "--format", "json"]
        );
        assert_eq!(
            tool_exec_args("Bash", "{}", Some("s1"), true)[6..],
            ["--session-id", "s1", "--approved"]
        );
    }

    #[test]
    fn parse_tool_outcome_reads_every_result_shape() {
        assert_eq!(
            parse_tool_outcome("{\"approval_required\":true,\"success\":false,\"output\":\"\"}\n"),
            Ok(ToolOutcome::NeedsApproval)
        );
        assert_eq!(
            parse_tool_outcome(r#"{"success":true,"output":"HTTP/2.0 200 OK"}"#),
            Ok(ToolOutcome::Done {
                success: true,
                output: "HTTP/2.0 200 OK".into(),
                error: String::new()
            })
        );
        assert_eq!(
            parse_tool_outcome(r#"{"success":false,"output":"x","error":"exit status 1: x"}"#),
            Ok(ToolOutcome::Done {
                success: false,
                output: "x".into(),
                error: "exit status 1: x".into()
            })
        );
        assert!(parse_tool_outcome("not json").is_err());
    }

    #[test]
    fn save_panel_attachment_skips_unsupported_or_malformed_entries() {
        let unsupported =
            serde_json::json!({"filename": "a.txt", "mime_type": "text/plain", "data": "aGk="});
        assert_eq!(save_panel_attachment(&unsupported), None);
        let missing_data = serde_json::json!({"filename": "a.png", "mime_type": "image/png"});
        assert_eq!(save_panel_attachment(&missing_data), None);
        let bad_base64 =
            serde_json::json!({"filename": "a.png", "mime_type": "image/png", "data": "***"});
        assert_eq!(save_panel_attachment(&bad_base64), None);
    }

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

    #[test]
    fn session_ids_look_like_uuids() {
        let id = new_session_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
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
        let port = bridge.start(settings, None, None).unwrap();

        let mut bad = connect_ext(port, r#"{"type":"browser_hello","token":"nope"}"#);
        assert_eq!(read_text(&mut bad), None);
        assert!(!bridge.connected());

        let mut ext = connect_ext(port, r#"{"type":"browser_hello","token":"tok"}"#);
        assert_eq!(read_text(&mut ext).as_deref(), Some(HELLO_ACK));
        assert!(bridge.connected());

        ext.send(Message::text(r#"{"type":"new_session"}"#))
            .unwrap();
        assert!(
            read_text(&mut ext)
                .unwrap()
                .contains(r#""type":"conversation_snapshot""#)
        );
        ext.send(Message::text(r#"{"type":"set_mode","mode":"auto"}"#))
            .unwrap();
        assert_eq!(
            read_text(&mut ext).as_deref(),
            Some(r#"{"mode":"auto","type":"mode"}"#)
        );
        let session = lock(&bridge.host().unwrap().panel).current.clone().unwrap();
        bridge.user_turn(&session, "hi");
        assert!(read_text(&mut ext).unwrap().contains(r#""role":"user""#));
        read_text(&mut ext).unwrap();
        read_text(&mut ext).unwrap();
        bridge.chat_event("other-session", r#"{"type":"RUN_STARTED"}"#);
        bridge.chat_event(&session, r#"{"type":"RUN_STARTED"}"#);
        assert!(read_text(&mut ext).unwrap().contains("RUN_STARTED"));

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
        ext.send(Message::text(r#"{"type":"ping"}"#)).unwrap();
        ext.send(Message::text(
            r#"{"type":"browser_result","id":"c1","error":""}"#,
        ))
        .unwrap();
        let back = read_text(&mut cli).unwrap();
        assert!(back.contains(r#""id":"c1""#));

        drop(ext);
        let gone = Instant::now() + Duration::from_secs(3);
        while bridge.connected() && Instant::now() < gone {
            std::thread::sleep(POLL);
        }
        assert!(!bridge.connected());
        cli.send(Message::text(
            r#"{"type":"browser_command","id":"c2","action":"tabs"}"#,
        ))
        .unwrap();
        let err = read_text(&mut cli).unwrap();
        assert!(err.contains(r#""id":"c2""#) && err.contains("no browser extension"));

        bridge.stop();
        assert!(bridge.host().is_none());
    }
}
