use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri::ipc::Channel;
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
enum ProgressEvent {
    Checking,
    Downloading {
        received: u64,
        total: u64,
    },
    Verifying,
    Installing,
    Initializing,
    Ready,
    #[allow(dead_code)]
    Error {
        message: String,
    },
}

/// Map the running platform to the CLI release asset name.
/// Returns `None` for unsupported platforms.
fn asset_name() -> Option<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Some("infer-linux-amd64"),
        ("linux", "aarch64") => Some("infer-linux-arm64"),
        ("macos", "x86_64") => Some("infer-darwin-amd64"),
        ("macos", "aarch64") => Some("infer-darwin-arm64"),
        ("windows", "x86_64") => Some("infer-windows-amd64"),
        ("windows", "aarch64") => Some("infer-windows-arm64"),
        _ => None,
    }
}

fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "infer.exe"
    } else {
        "infer"
    }
}

fn home_dir() -> PathBuf {
    #[cfg(unix)]
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    #[cfg(windows)]
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".into());
    PathBuf::from(home)
}

/// Path of the infer binary to spawn. INFER_BIN overrides the installed one
/// so test harnesses can point at a specific build without touching
/// ~/.infer/bin.
fn infer_bin_path() -> PathBuf {
    match std::env::var("INFER_BIN") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => home_dir().join(".infer").join("bin").join(binary_name()),
    }
}

/// Token-free e2e testing: DESKTOP_MOCK=true skips the desktop-owned gateway,
/// serves a canned model list, and spawns infer children with
/// INFER_GATEWAY_MOCK=true - the CLI's own mock mode, where infer serves its
/// embedded scenario gateway (see cli/internal/mockgateway).
fn mock_mode() -> bool {
    std::env::var("DESKTOP_MOCK").is_ok_and(|v| v == "true" || v == "1")
}

/// The Go OTLP/HTTP exporters in the CLI and gateway only speak
/// http/protobuf (the Go SDK never implemented http/json), so the collector
/// must accept protobuf; no protocol pin is set here.
fn collector_env() -> Vec<(String, String)> {
    vec![(
        "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
        "http://localhost:4318/".into(),
    )]
}

/// Extra env vars for spawned infer processes: provider keys, plus the CLI
/// mock switch when the desktop runs in mock mode.
fn infer_env() -> Vec<(String, String)> {
    let mut env = auth_env();
    env.extend(collector_env());
    if mock_mode() {
        env.push(("INFER_GATEWAY_MOCK".into(), "true".into()));
    }
    env
}

/// Prompt customisation env vars for the spawned agent. The CLI applies these
/// after loading prompts.yaml, so they win over any file config: the override
/// replaces the base system prompt, extras are appended after it
/// (prompts.agent.custom_instructions).
fn prompt_env(
    system_prompt: Option<&str>,
    extra_instructions: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut env = Vec::new();
    if let Some(sp) = system_prompt.filter(|s| !s.trim().is_empty()) {
        env.push(("INFER_PROMPTS_AGENT_SYSTEM_PROMPT", sp.to_string()));
    }
    if let Some(ei) = extra_instructions.filter(|s| !s.trim().is_empty()) {
        env.push(("INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS", ei.to_string()));
    }
    env
}

/// Append the agent's actual working directory to the custom instructions so
/// the model states it instead of guessing (Finder launches land in $HOME).
fn compose_extras(extra_instructions: Option<&str>, cwd: &Path) -> String {
    let cwd_note = format!("Current working directory: {}", cwd.display());
    match extra_instructions.filter(|s| !s.trim().is_empty()) {
        Some(ei) => format!("{ei}\n\n{cwd_note}"),
        None => cwd_note,
    }
}

fn config_path() -> PathBuf {
    home_dir().join(".infer").join("config.yaml")
}

/// Pure core of `agent_cwd`, split out so the fallback is testable.
fn resolve_agent_cwd(current: Option<PathBuf>, home: PathBuf) -> PathBuf {
    match current {
        Some(dir) if dir.ends_with("src-tauri") => {
            dir.parent().map(Path::to_path_buf).unwrap_or(dir)
        }
        Some(dir) if dir.as_path() != std::path::Path::new("/") => dir,
        _ => home,
    }
}

/// Working directory for spawned infer processes. `cargo tauri dev` runs the
/// app from src-tauri/ regardless of where it was invoked, but agent sessions
/// should work in the project root - one level up. Finder-launched `.app`
/// bundles inherit cwd `/` (read-only), so infer's cwd-relative `.infer`
/// storage would land at `/.infer` and panic; fall back to the home dir there.
fn agent_cwd() -> PathBuf {
    resolve_agent_cwd(std::env::current_dir().ok(), home_dir())
}

/// Download a file from `url` to `dest`, sending progress through the channel.
fn download(
    url: &str,
    dest: &std::path::Path,
    on_event: &Channel<ProgressEvent>,
) -> Result<(), String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("Failed to download {}: {}", url, e))?;

    let total: u64 = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 8192];
    let mut received = 0u64;

    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        received += n as u64;
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        let _ = on_event.send(ProgressEvent::Downloading { received, total });
    }
    Ok(())
}

/// Compute SHA256 hex digest of a file.
fn sha256_digest(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Parse checksums.txt and find the expected hash for `asset_name`.
fn find_checksum(checksums_text: &str, asset_name: &str) -> Option<String> {
    for line in checksums_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == asset_name {
            return Some(parts[0].to_string());
        }
    }
    None
}

/// Try to download a release asset using `gh` CLI (authenticated).
/// Returns `Ok(true)` if downloaded, `Ok(false)` to fall back to ureq.
fn try_gh_download(asset: &str, dest: &std::path::Path) -> Result<bool, String> {
    let available = std::process::Command::new("gh")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        return Ok(false);
    }
    let authed = std::process::Command::new("gh")
        .args(["auth", "status"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !authed {
        return Ok(false);
    }
    let status = std::process::Command::new("gh")
        .args([
            "release",
            "download",
            "latest",
            "--repo",
            "inference-gateway/cli",
            "--pattern",
            asset,
            "--output",
            dest.to_str().unwrap_or(""),
            "--clobber",
        ])
        .status()
        .map_err(|e| format!("gh release download failed: {}", e))?;
    if !status.success() {
        return Err("gh release download exited with non-zero status".into());
    }
    Ok(true)
}

/// Install the CLI if it is missing. `force` reinstalls over an existing binary,
/// which is how an update is applied.
#[tauri::command]
async fn check_and_install_cli(
    on_event: Channel<ProgressEvent>,
    force: bool,
) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Checking);

    let bin_path = infer_bin_path();

    if bin_path.exists() && !force {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&bin_path).map_err(|e| e.to_string())?;
            if meta.permissions().mode() & 0o111 != 0 {
                let _ = on_event.send(ProgressEvent::Ready);
                return Ok(());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = on_event.send(ProgressEvent::Ready);
            return Ok(());
        }
    }

    let asset = asset_name().ok_or_else(|| {
        format!(
            "Unsupported platform: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let release_url = "https://github.com/inference-gateway/cli/releases/latest/download";

    let bin_dir = home_dir().join(".infer").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let temp_path = bin_dir.join(format!("{}.tmp", binary_name()));

    let _ = on_event.send(ProgressEvent::Downloading {
        received: 0,
        total: 0,
    });
    if !try_gh_download(asset, &temp_path)? {
        download(&format!("{}/{}", release_url, asset), &temp_path, &on_event)?;
    }

    let _ = on_event.send(ProgressEvent::Verifying);
    let checksums_url = format!("{}/checksums.txt", release_url);
    let checksums_text = {
        let checksums_temp = bin_dir.join("checksums.txt.tmp");
        if try_gh_download("checksums.txt", &checksums_temp)? {
            let text = std::fs::read_to_string(&checksums_temp).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&checksums_temp);
            text
        } else {
            let checksums_resp = ureq::get(&checksums_url)
                .call()
                .map_err(|e| format!("Failed to download checksums.txt: {}", e))?;
            let mut text = String::new();
            checksums_resp
                .into_body()
                .into_reader()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            text
        }
    };

    let expected_hash = find_checksum(&checksums_text, asset)
        .ok_or_else(|| format!("Checksum not found for {}", asset))?;
    let actual_hash = sha256_digest(&temp_path)?;

    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!(
            "Checksum mismatch for {}: expected {}, got {}",
            asset, expected_hash, actual_hash
        ));
    }

    let _ = on_event.send(ProgressEvent::Installing);
    std::fs::rename(&temp_path, &bin_path).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).map_err(|e| e.to_string())?;
    }

    let cfg_path = config_path();
    if !cfg_path.exists() {
        let _ = on_event.send(ProgressEvent::Initializing);
        let status = std::process::Command::new(&bin_path)
            .args(["init", "--userspace"])
            .env("HOME", home_dir().to_str().unwrap_or(""))
            .status()
            .map_err(|e| format!("Failed to run infer init: {}", e))?;
        if !status.success() {
            return Err("infer init --userspace failed".into());
        }

        let mut config = String::new();
        let _ = std::fs::File::open(&cfg_path).and_then(|mut f| f.read_to_string(&mut config));
        let mut needs_write = false;
        if !config.contains("gateway.run:") {
            config.push_str("\ngateway.run: true\n");
            needs_write = true;
        }
        if !config.contains("gateway.standalone_binary:") {
            config.push_str("gateway.standalone_binary: true\n");
            needs_write = true;
        }
        if needs_write {
            std::fs::write(&cfg_path, &config).map_err(|e| e.to_string())?;
        }
    }

    let _ = on_event.send(ProgressEvent::Ready);
    Ok(())
}

// --- Agent streaming types ---

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
enum AgentEvent {
    #[allow(dead_code)]
    SessionId {
        session_id: String,
    },
    AssistantMessage {
        content: String,
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCallInfo>,
    },
    ToolResult {
        content: String,
        tool_call_id: String,
    },
    ApprovalRequest {
        tool_name: String,
        tool_args: String,
        tool_call_id: String,
    },
    Info {
        message: String,
    },
    AgentError {
        message: String,
    },
    RawLine {
        line: String,
    },
    Done {
        exit_code: i32,
        stderr: String,
    },
    TokenUsage {
        input: i64,
        output: i64,
        cached_read: i64,
        total_tool_calls: i64,
    },
    #[allow(dead_code)]
    Cancelled,
}

#[derive(Clone, serde::Serialize)]
struct ToolCallInfo {
    id: String,
    name: String,
    args: String,
}

/// Stored trace span extracted from an OTLP export.
#[derive(Clone, serde::Serialize)]
struct StoredSpan {
    trace_id: String,
    span_id: String,
    parent_span_id: String,
    name: String,
    kind: i32,
    start_time_unix_nano: u64,
    end_time_unix_nano: u64,
    attributes: Vec<(String, String)>,
    status_code: i32,
    service_name: String,
}

/// Stored metric data point extracted from an OTLP export. `value` is the
/// point value for sums/gauges and the bucket sum for histograms; `count` is
/// 1 for sums/gauges and the observation count for histograms. The CLI
/// exports delta temporality, so totals are sums over stored points.
#[derive(Clone, serde::Serialize)]
struct StoredMetric {
    name: String,
    unit: String,
    value: f64,
    count: u64,
    attributes: Vec<(String, String)>,
    time_unix_nano: u64,
}

struct AppState {
    running_children: Mutex<std::collections::HashMap<String, std::process::Child>>,
    child_stdins: Mutex<std::collections::HashMap<String, std::process::ChildStdin>>,
    gateway_child: Mutex<Option<std::process::Child>>,
    stored_traces: std::sync::Arc<std::sync::Mutex<VecDeque<StoredSpan>>>,
    stored_metrics: std::sync::Arc<std::sync::Mutex<VecDeque<StoredMetric>>>,
}

/// Folds AG-UI event-stream lines into complete AgentEvents, accumulating
/// message/tool-call triads internally.
struct AgentParser {
    session_id: Option<String>,
    msg_from_user: bool,
    tc_id: String,
    tc_name: String,
    tc_args: String,
}

/// Reads the text delta of a streaming message event, tolerating producers that
/// use `content` instead of `delta`.
fn delta_of(val: &serde_json::Value) -> Option<&str> {
    val.get("delta")
        .or_else(|| val.get("content"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

impl AgentParser {
    fn new(session_id: Option<String>) -> Self {
        Self {
            session_id,
            msg_from_user: false,
            tc_id: String::new(),
            tc_name: String::new(),
            tc_args: String::new(),
        }
    }

    /// Emits an assistant text delta unless the current message is the echoed
    /// user prompt.
    fn assistant_text(&self, val: &serde_json::Value) -> Option<AgentEvent> {
        if self.msg_from_user {
            return None;
        }
        Some(AgentEvent::AssistantMessage {
            content: delta_of(val)?.to_string(),
            reasoning_content: None,
            tool_calls: Vec::new(),
        })
    }

    /// Take the accumulated session_id out.
    fn take_session_id(&mut self) -> Option<String> {
        self.session_id.take()
    }

    fn parse_line(&mut self, line: &str) -> Option<AgentEvent> {
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return Some(AgentEvent::RawLine {
                    line: line.to_string(),
                });
            }
        };

        let event_type = match val.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                return Some(AgentEvent::RawLine {
                    line: line.to_string(),
                });
            }
        };

        match event_type {
            "RUN_STARTED" => {
                let sid = val.get("threadId").and_then(|v| v.as_str()).unwrap_or("");
                if !sid.is_empty() && self.session_id.is_none() {
                    self.session_id = Some(sid.to_string());
                }
                Some(AgentEvent::Info {
                    message: "Session started".into(),
                })
            }
            "MESSAGES_SNAPSHOT" | "STATE_SNAPSHOT" => None,
            "TEXT_MESSAGE_START" => {
                self.msg_from_user = val.get("role").and_then(|v| v.as_str()) == Some("user");
                self.assistant_text(&val)
            }
            "TEXT_MESSAGE_CONTENT" => self.assistant_text(&val),
            "TEXT_MESSAGE_END" => {
                self.msg_from_user = false;
                None
            }
            // Reasoning/thinking phases are assistant-only; each content event is
            // one streamed delta. THINKING_* are the deprecated aliases of the
            // REASONING_* events - accept both.
            "REASONING_MESSAGE_CONTENT" | "THINKING_TEXT_MESSAGE_CONTENT" => {
                delta_of(&val).map(|d| AgentEvent::AssistantMessage {
                    content: String::new(),
                    reasoning_content: Some(d.to_string()),
                    tool_calls: Vec::new(),
                })
            }
            "REASONING_MESSAGE_START"
            | "REASONING_MESSAGE_END"
            | "REASONING_START"
            | "REASONING_END"
            | "THINKING_TEXT_MESSAGE_START"
            | "THINKING_TEXT_MESSAGE_END"
            | "THINKING_START"
            | "THINKING_END" => None,
            "TOOL_CALL_START" => {
                self.tc_id = val
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.tc_name = val
                    .get("toolCallName")
                    .or_else(|| val.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.tc_args = String::new();
                None
            }
            "TOOL_CALL_ARGS" => {
                if let Some(args) = val
                    .get("delta")
                    .or_else(|| val.get("args"))
                    .and_then(|v| v.as_str())
                {
                    self.tc_args.push_str(args);
                }
                None
            }
            "TOOL_CALL_END" => {
                if self.tc_id.is_empty() {
                    return None;
                }
                Some(AgentEvent::AssistantMessage {
                    content: String::new(),
                    reasoning_content: None,
                    tool_calls: vec![ToolCallInfo {
                        id: std::mem::take(&mut self.tc_id),
                        name: std::mem::take(&mut self.tc_name),
                        args: std::mem::take(&mut self.tc_args),
                    }],
                })
            }
            "TOOL_CALL_RESULT" => {
                let content = val
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = val
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    return None;
                }
                Some(AgentEvent::ToolResult {
                    content,
                    tool_call_id,
                })
            }
            "CUSTOM" => {
                if val.get("name").and_then(|v| v.as_str()) == Some("approval_request")
                    && let Some(data) = val.get("value")
                {
                    let tool_name = data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_args = data
                        .get("tool_args")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_call_id = data
                        .get("tool_call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Some(AgentEvent::ApprovalRequest {
                        tool_name,
                        tool_args,
                        tool_call_id,
                    });
                }
                Some(AgentEvent::RawLine {
                    line: line.to_string(),
                })
            }
            "RUN_FINISHED" => {
                self.flush_message();
                let stats = val.get("stats");
                let input = stats
                    .and_then(|s| s.get("inputTokens"))
                    .map(json_val_i64)
                    .unwrap_or(0);
                let output = stats
                    .and_then(|s| s.get("outputTokens"))
                    .map(json_val_i64)
                    .unwrap_or(0);
                let cached = stats
                    .and_then(|s| s.get("cacheReadTokens"))
                    .map(json_val_i64)
                    .unwrap_or(0);
                let tools = stats
                    .and_then(|s| s.get("totalToolCalls"))
                    .map(json_val_i64)
                    .unwrap_or(0);
                Some(AgentEvent::TokenUsage {
                    input,
                    output,
                    cached_read: cached,
                    total_tool_calls: tools,
                })
            }
            "RUN_ERROR" => {
                self.flush_message();
                let message = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(AgentEvent::AgentError { message })
            }
            _ => Some(AgentEvent::RawLine {
                line: line.to_string(),
            }),
        }
    }

    fn flush_message(&mut self) {
        self.msg_from_user = false;
    }
}

#[tauri::command]
async fn send_message(
    prompt: String,
    model: String,
    session_id: String,
    on_event: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
    system_prompt: Option<String>,
    extra_instructions: Option<String>,
) -> Result<Option<String>, String> {
    let bin_path = infer_bin_path();

    let mut cmd = std::process::Command::new(&bin_path);
    cmd.arg("headless")
        .arg("--format")
        .arg("ag-ui")
        .arg("--session-id")
        .arg(&session_id)
        .arg("--require-approval")
        .arg("-m")
        .arg(&model);

    let cwd = agent_cwd();
    let extras = compose_extras(extra_instructions.as_deref(), &cwd);
    cmd.arg(&prompt)
        .envs(infer_env())
        .envs(prompt_env(system_prompt.as_deref(), Some(&extras)))
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn infer agent: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let child_stdin = child.stdin.take().unwrap();

    {
        let mut guard = state.running_children.lock().map_err(|e| e.to_string())?;
        guard.insert(session_id.clone(), child);
    }
    {
        let mut guard = state.child_stdins.lock().map_err(|e| e.to_string())?;
        guard.insert(session_id.clone(), child_stdin);
    }

    let on_event_clone = on_event.clone();
    let had_error = Arc::new(Mutex::new(false));
    let had_error_clone = Arc::clone(&had_error);
    let parser = Arc::new(Mutex::new(AgentParser::new(Some(session_id.clone()))));
    let parser_clone = Arc::clone(&parser);

    let stdout_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut p = parser_clone.lock().unwrap();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(event) = p.parse_line(&line) {
                if matches!(event, AgentEvent::AgentError { .. }) {
                    *had_error_clone.lock().unwrap() = true;
                }
                let _ = on_event_clone.send(event);
            }
        }
    });

    let stderr_handle: std::thread::JoinHandle<String> = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        let mut all = String::new();
        for l in reader.lines().map_while(Result::ok) {
            all.push_str(&l);
            all.push('\n');
        }
        all
    });

    let _ = tokio::task::spawn_blocking(move || stdout_handle.join())
        .await
        .map_err(|e| format!("Join error: {}", e))?;

    let child = {
        let mut guard = state.running_children.lock().map_err(|e| e.to_string())?;
        guard.remove(&session_id)
    };
    let status = match child {
        Some(mut child) => Some(
            child
                .wait()
                .map_err(|e| format!("Failed to wait for process: {}", e))?,
        ),
        None => None,
    };
    {
        let mut guard = state.child_stdins.lock().map_err(|e| e.to_string())?;
        guard.remove(&session_id);
    }

    let stderr_text = stderr_handle.join().unwrap_or_default();

    let had_error_val = *had_error.lock().unwrap();
    let exit_code = status.as_ref().and_then(|s| s.code()).unwrap_or(-1);
    if !status.as_ref().is_some_and(|s| s.success()) && !had_error_val {
        let msg = if stderr_text.is_empty() {
            format!("Process exited with code {}", exit_code)
        } else {
            format!(
                "Process exited with code {}: {}",
                exit_code,
                stderr_text.trim()
            )
        };
        let _ = on_event.send(AgentEvent::AgentError { message: msg });
    }

    let _ = on_event.send(AgentEvent::Done {
        exit_code,
        stderr: stderr_text,
    });

    let new_session = parser.lock().unwrap().take_session_id();
    Ok(new_session)
}

#[tauri::command]
async fn send_approval(
    session_id: String,
    tool_call_id: String,
    approved: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut guard = state.child_stdins.lock().map_err(|e| e.to_string())?;
    let stdin = guard.get_mut(&session_id).ok_or("No running agent")?;
    let response = serde_json::json!({
        "type": "approval_response",
        "tool_call_id": tool_call_id,
        "approved": approved,
    });
    let line = format!(
        "{}\n",
        serde_json::to_string(&response).map_err(|e| e.to_string())?
    );
    stdin
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn cancel_agent(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    {
        let mut guard = state.child_stdins.lock().map_err(|e| e.to_string())?;
        guard.remove(&session_id);
    }
    let child = {
        let mut guard = state.running_children.lock().map_err(|e| e.to_string())?;
        guard.remove(&session_id)
    };
    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

/// Resolve the local gateway URL from infer's config, defaulting to localhost:8080.
fn gateway_url() -> String {
    read_config().gateway_url
}

/// Run `infer <args>` to completion and return stdout, erroring on non-zero exit.
/// The subprocess runs on the blocking pool so the async runtime keeps serving
/// other commands while `infer` works.
async fn run_infer(args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new(infer_bin_path())
            .args(&args)
            .env("HOME", home_dir().to_str().unwrap_or(""))
            .current_dir(agent_cwd())
            .output()
            .map_err(|e| format!("Failed to run infer: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "infer {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    })
    .await
    .map_err(|e| format!("infer task failed: {}", e))?
}

#[tauri::command]
async fn list_conversations() -> Result<String, String> {
    run_infer(&["conversations", "list", "--format", "json"]).await
}

#[tauri::command]
async fn get_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "show", &session_id, "--format", "json"]).await
}

#[tauri::command]
async fn delete_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "delete", &session_id]).await
}

#[tauri::command]
async fn list_models() -> Result<Vec<String>, String> {
    if mock_mode() {
        return Ok(vec![
            "openai/gpt-4o".into(),
            "anthropic/claude-sonnet-4-5".into(),
            "openai/gpt-image-2".into(),
        ]);
    }
    let url = format!("{}/v1/models", gateway_url().trim_end_matches('/'));
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("Gateway unreachable at {}: {}", url, e))?;
    let mut text = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let models = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("Unexpected /v1/models response shape")?
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    Ok(models)
}

/// App-owned provider key store at ~/.infer/auth.json (Codex-style).
/// infer does not read this file; the desktop injects its values as env vars
/// when spawning `infer`.
fn auth_path() -> PathBuf {
    home_dir().join(".infer").join("auth.json")
}

fn read_auth() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(auth_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Non-empty (KEY, value) pairs to inject as environment variables.
fn auth_env() -> Vec<(String, String)> {
    read_auth()
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[tauri::command]
async fn get_auth() -> Result<serde_json::Value, String> {
    Ok(serde_json::Value::Object(read_auth()))
}

#[tauri::command]
async fn set_auth(keys: std::collections::HashMap<String, String>) -> Result<(), String> {
    let path = auth_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let map: serde_json::Map<String, serde_json::Value> = keys
        .into_iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let json =
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Desktop-facing config fields read from ~/.infer/config.yaml.
/// Only the fields the Settings UI manages are exposed; the rest of the
/// config (gateway.run, gateway.standalone_binary, etc.) passes through.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DesktopConfig {
    storage_backend: String,
    storage_directory: String,
    gateway_url: String,
    default_model: String,
    sqlite_path: String,
    postgres_host: String,
    postgres_port: String,
    postgres_database: String,
    postgres_username: String,
    postgres_password: String,
    postgres_ssl_mode: String,
    redis_host: String,
    redis_port: String,
    redis_password: String,
    redis_db: String,
    d1_account_id: String,
    d1_database_id: String,
    d1_api_token: String,
    d1_base_url: String,
    /// Extra instructions appended to the default system prompt on every invocation.
    extra_instructions: String,
    /// Override the entire default system prompt with user-supplied text.
    /// When both are set, override replaces the default and extra_instructions are
    /// appended after it.
    system_prompt: String,
}

fn default_storage_directory(home: &std::path::Path) -> String {
    home.join(".infer")
        .join("conversations")
        .to_string_lossy()
        .to_string()
}

fn default_config() -> DesktopConfig {
    DesktopConfig {
        storage_backend: "jsonl".into(),
        storage_directory: default_storage_directory(&home_dir()),
        gateway_url: "http://localhost:8080".into(),
        default_model: String::new(),
        sqlite_path: ".infer/conversations.db".into(),
        postgres_host: "localhost".into(),
        postgres_port: "5432".into(),
        postgres_database: "infer_conversations".into(),
        postgres_username: String::new(),
        postgres_password: String::new(),
        postgres_ssl_mode: "prefer".into(),
        redis_host: "localhost".into(),
        redis_port: "6379".into(),
        redis_password: String::new(),
        redis_db: "0".into(),
        d1_account_id: String::new(),
        d1_database_id: String::new(),
        d1_api_token: String::new(),
        d1_base_url: "https://api.cloudflare.com/client/v4".into(),
        extra_instructions: String::new(),
        system_prompt: String::new(),
    }
}

/// Pure core of `read_config`, split out so field extraction is testable
/// without touching ~/.infer.
fn config_from_value(val: &serde_norway::Value, home: &std::path::Path) -> DesktopConfig {
    let str_at = |path: &[&str]| -> Option<String> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_str().filter(|s| !s.is_empty()).map(str::to_string)
    };
    let int_at = |path: &[&str]| -> Option<i64> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_i64()
    };
    let d = default_config();
    DesktopConfig {
        storage_backend: str_at(&["storage", "type"]).unwrap_or_else(|| "jsonl".into()),
        storage_directory: str_at(&["storage", "jsonl", "path"])
            .map(|s| match s.strip_prefix('~') {
                Some(rest) => format!("{}{}", home.display(), rest),
                None => s,
            })
            .unwrap_or_else(|| default_storage_directory(home)),
        gateway_url: str_at(&["gateway", "url"]).unwrap_or_else(|| "http://localhost:8080".into()),
        default_model: str_at(&["default_model"]).unwrap_or_default(),
        sqlite_path: str_at(&["storage", "sqlite", "path"]).unwrap_or(d.sqlite_path),
        postgres_host: str_at(&["storage", "postgres", "host"]).unwrap_or(d.postgres_host),
        postgres_port: int_at(&["storage", "postgres", "port"])
            .map(|p| p.to_string())
            .unwrap_or(d.postgres_port),
        postgres_database: str_at(&["storage", "postgres", "database"])
            .unwrap_or(d.postgres_database),
        postgres_username: str_at(&["storage", "postgres", "username"])
            .unwrap_or(d.postgres_username),
        postgres_password: str_at(&["storage", "postgres", "password"])
            .unwrap_or(d.postgres_password),
        postgres_ssl_mode: str_at(&["storage", "postgres", "ssl_mode"])
            .unwrap_or(d.postgres_ssl_mode),
        redis_host: str_at(&["storage", "redis", "host"]).unwrap_or(d.redis_host),
        redis_port: int_at(&["storage", "redis", "port"])
            .map(|p| p.to_string())
            .unwrap_or(d.redis_port),
        redis_password: str_at(&["storage", "redis", "password"]).unwrap_or(d.redis_password),
        redis_db: int_at(&["storage", "redis", "db"])
            .map(|p| p.to_string())
            .unwrap_or(d.redis_db),
        d1_account_id: str_at(&["storage", "d1", "account_id"]).unwrap_or(d.d1_account_id),
        d1_database_id: str_at(&["storage", "d1", "database_id"]).unwrap_or(d.d1_database_id),
        d1_api_token: str_at(&["storage", "d1", "api_token"]).unwrap_or(d.d1_api_token),
        d1_base_url: str_at(&["storage", "d1", "base_url"]).unwrap_or(d.d1_base_url),
        extra_instructions: str_at(&["extra_instructions"]).unwrap_or_default(),
        system_prompt: str_at(&["system_prompt"]).unwrap_or_default(),
    }
}

fn read_config() -> DesktopConfig {
    match std::fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
    {
        Some(val) => config_from_value(&val, &home_dir()),
        None => default_config(),
    }
}

/// Parse `text` as a YAML mapping, or start from a fresh one. A malformed or
/// non-mapping existing config is replaced rather than aborting the write.
fn config_mapping(text: Option<&str>) -> serde_norway::Value {
    text.and_then(|t| serde_norway::from_str::<serde_norway::Value>(t).ok())
        .filter(serde_norway::Value::is_mapping)
        .unwrap_or_else(|| serde_norway::Value::Mapping(Default::default()))
}

/// Parse `s` as an integer, falling back to `default` for empty/garbage input so
/// we never write a non-numeric value where infer's schema expects an int.
fn parse_int_or(s: &str, default: i64) -> i64 {
    s.trim().parse().unwrap_or(default)
}

/// Insert `kvs` into `storage.<name>` (creating the sub-mapping if absent),
/// leaving any keys infer manages but the desktop doesn't touch.
fn set_section(
    smap: &mut serde_norway::Mapping,
    name: &str,
    kvs: Vec<(&str, serde_norway::Value)>,
) {
    let section = smap
        .entry(name.into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(m) = section.as_mapping_mut() {
        for (k, v) in kvs {
            m.insert(k.into(), v);
        }
    }
}

/// Merge the Settings-managed storage + gateway fields into the existing config
/// text, returning serialized YAML. `default_model` is owned by
/// `merge_default_model` (written from the composer too), so it is deliberately
/// not touched here - otherwise a stale Settings form would clobber a newer
/// model chosen from the composer.
fn merge_config(existing: Option<&str>, cfg: &DesktopConfig) -> Result<String, String> {
    let mut yaml = config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;

    let storage = map
        .entry("storage".into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(smap) = storage.as_mapping_mut() {
        smap.insert("enabled".into(), true.into());
        smap.insert("type".into(), cfg.storage_backend.clone().into());
        smap.remove("backend");
        smap.remove("directory");

        set_section(
            smap,
            "jsonl",
            vec![("path", cfg.storage_directory.clone().into())],
        );
        set_section(
            smap,
            "sqlite",
            vec![("path", cfg.sqlite_path.clone().into())],
        );
        set_section(
            smap,
            "postgres",
            vec![
                ("host", cfg.postgres_host.clone().into()),
                ("port", parse_int_or(&cfg.postgres_port, 5432).into()),
                ("database", cfg.postgres_database.clone().into()),
                ("username", cfg.postgres_username.clone().into()),
                ("password", cfg.postgres_password.clone().into()),
                ("ssl_mode", cfg.postgres_ssl_mode.clone().into()),
            ],
        );
        set_section(
            smap,
            "redis",
            vec![
                ("host", cfg.redis_host.clone().into()),
                ("port", parse_int_or(&cfg.redis_port, 6379).into()),
                ("password", cfg.redis_password.clone().into()),
                ("db", parse_int_or(&cfg.redis_db, 0).into()),
            ],
        );
        set_section(
            smap,
            "d1",
            vec![
                ("account_id", cfg.d1_account_id.clone().into()),
                ("database_id", cfg.d1_database_id.clone().into()),
                ("api_token", cfg.d1_api_token.clone().into()),
                ("base_url", cfg.d1_base_url.clone().into()),
            ],
        );
    }

    let gateway = map
        .entry("gateway".into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(gmap) = gateway.as_mapping_mut() {
        gmap.insert("url".into(), cfg.gateway_url.clone().into());
    }

    // Desktop round-trip: write individual fields at top level so the Settings
    // UI can read them back via config_from_value.
    map.insert(
        "extra_instructions".into(),
        cfg.extra_instructions.clone().into(),
    );
    map.insert("system_prompt".into(), cfg.system_prompt.clone().into());

    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// Merge only the top-level `default_model` key into the existing config text.
fn merge_default_model(existing: Option<&str>, model: &str) -> Result<String, String> {
    let mut yaml = config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    map.insert("default_model".into(), model.to_string().into());
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// Read ~/.infer/config.yaml, apply `f` to its text, and write the result back,
/// creating the parent dir as needed.
fn update_config_file(
    f: impl FnOnce(Option<&str>) -> Result<String, String>,
) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    let text = f(existing.as_deref())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_config() -> Result<DesktopConfig, String> {
    Ok(read_config())
}

#[tauri::command]
async fn set_config(cfg: DesktopConfig) -> Result<(), String> {
    update_config_file(|existing| merge_config(existing, &cfg))
}

#[tauri::command]
async fn set_default_model(model: String) -> Result<(), String> {
    update_config_file(|existing| merge_default_model(existing, &model))
}

/// Read ~/.infer/history (line-delimited prompt history shared with the CLI).
/// Returns an empty vec when the file does not exist yet.
#[tauri::command]
fn read_history() -> Result<Vec<String>, String> {
    let path = home_dir().join(".infer").join("history");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(content.lines().map(|l| l.to_string()).collect())
}

/// Append a line to ~/.infer/history (creates the file if missing).
#[tauri::command]
fn append_history(line: String) -> Result<(), String> {
    let path = home_dir().join(".infer").join("history");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{}", line).map_err(|e| e.to_string())?;
    Ok(())
}

/// Validate a generated-image path before copying it out: absolute, under
/// `home`, inside an `.infer/tmp` dir, no `..` traversal. Split from
/// `save_image` so the guard is testable without touching the filesystem.
fn safe_image_source(path: &str, home: &Path) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let ok = p.starts_with(home)
        && !p.components().any(|c| c == Component::ParentDir)
        && path.contains("/.infer/tmp/");
    if !ok {
        return Err(format!("Refusing to save image outside .infer/tmp: {path}"));
    }
    Ok(p.to_path_buf())
}

/// Copy a generated image (the absolute path `infer` reported) to the user's
/// Downloads folder. Returns the destination path on success.
#[tauri::command]
fn save_image(path: String) -> Result<String, String> {
    let home = home_dir();
    let src = safe_image_source(&path, &home)?;
    if !src.exists() {
        return Err(format!("Image not found: {path}"));
    }
    let name = src.file_name().ok_or("invalid image path")?;
    let downloads = home.join("Downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let dest = downloads.join(name);
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

// --- A2A Agent configuration ---
// A2A agents live in the `infer` CLI's own config (~/.infer/agents.yaml). Rather
// than reimplement the CLI's agent knowledge — it auto-fills oci/run/model/env
// for known agents (browser-agent, documentation-agent, ...) and distinguishes
// local containers from external URL agents — the desktop shells out to
// `infer agents add|remove|list` so the two stay perfectly in sync. Keyed by
// name (the CLI's primary key).

#[derive(Clone, serde::Serialize)]
struct A2aAgent {
    name: String,
    url: String,
    run: bool,
    model: String,
}

// `infer agents list --format json` fields are PascalCase (Go struct names),
// split into `local` (container) and `external` (URL-only) groups.
#[derive(serde::Deserialize)]
struct CliAgent {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "Run")]
    run: bool,
    #[serde(rename = "Model", default)]
    model: String,
}

#[derive(serde::Deserialize)]
struct CliAgentList {
    local: Option<Vec<CliAgent>>,
    external: Option<Vec<CliAgent>>,
}

fn parse_agent_list(json: &[u8]) -> Result<Vec<A2aAgent>, String> {
    let parsed: CliAgentList = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    Ok([parsed.local, parsed.external]
        .into_iter()
        .flatten()
        .flatten()
        .map(|a| A2aAgent {
            name: a.name,
            url: a.url,
            run: a.run,
            model: a.model,
        })
        .collect())
}

async fn run_infer_agents(args: &[&str]) -> Result<std::process::Output, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(infer_bin_path())
            .arg("agents")
            .args(&args)
            .arg("--no-colors")
            .env("HOME", home_dir().to_str().unwrap_or(""))
            .current_dir(agent_cwd())
            .output()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("infer agents task failed: {}", e))?
}

fn agents_command_ok(out: std::process::Output) -> Result<(), String> {
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command]
async fn list_a2a_agents() -> Result<Vec<A2aAgent>, String> {
    let out = run_infer_agents(&["list", "--format", "json"]).await?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_agent_list(&out.stdout)
}

#[tauri::command]
async fn add_a2a_agent(name: String, url: String) -> Result<(), String> {
    let mut args = vec!["add", name.as_str()];
    let url = url.trim();
    if !url.is_empty() {
        args.push(url);
    }
    agents_command_ok(run_infer_agents(&args).await?)
}

#[tauri::command]
async fn remove_a2a_agent(name: String) -> Result<(), String> {
    agents_command_ok(run_infer_agents(&["remove", name.as_str()]).await?)
}

#[tauri::command]
async fn set_a2a_agent_model(name: String, model: String) -> Result<(), String> {
    agents_command_ok(
        run_infer_agents(&["update", name.as_str(), "--model", model.as_str()]).await?,
    )
}

// --- Gateway lifecycle (desktop-owned) ---
// The desktop downloads and runs the inference-gateway binary itself so /v1/models
// stays served. Once it's up, `infer agent` detects it (its own isBinaryRunning
// health check) and won't start a competing gateway.

fn gateway_bin_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "inference-gateway.exe"
    } else {
        "inference-gateway"
    };
    home_dir().join(".infer").join("bin").join(name)
}

/// Release asset name for the gateway binary, matching goreleaser's naming.
fn gateway_asset_name() -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        "arm" => "armv7",
        _ => return None,
    };
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    Some(format!("inference-gateway_{}_{}.{}", os, arch, ext))
}

fn gateway_reachable() -> bool {
    let url = format!("{}/v1/models", gateway_url().trim_end_matches('/'));
    ureq::get(&url).call().is_ok()
}

/// Download and extract the gateway binary if it isn't already present.
/// `force` re-downloads over an existing binary; the caller must have stopped it
/// first, otherwise the extraction hits ETXTBSY.
fn ensure_gateway_binary(force: bool) -> Result<PathBuf, String> {
    let bin = gateway_bin_path();
    if bin.exists() && !force {
        return Ok(bin);
    }
    if cfg!(target_os = "windows") {
        return Err("Automatic gateway download is not supported on Windows yet".into());
    }

    let asset = gateway_asset_name().ok_or_else(|| {
        format!(
            "Unsupported platform for gateway binary: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let bin_dir = home_dir().join(".infer").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let url = format!(
        "https://github.com/inference-gateway/inference-gateway/releases/latest/download/{}",
        asset
    );
    let archive = bin_dir.join(&asset);
    let _ = std::fs::remove_file(&bin);

    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("Failed to download gateway: {}", e))?;
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    drop(file);

    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&bin_dir)
        .arg("inference-gateway")
        .status()
        .map_err(|e| format!("Failed to extract gateway: {}", e))?;
    let _ = std::fs::remove_file(&archive);

    if !status.success() || !bin.exists() {
        return Err("Failed to extract gateway binary from release archive".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).map_err(|e| e.to_string())?;
    }
    Ok(bin)
}

/// Start (or restart) the gateway. `force` re-downloads the binary first, so an
/// update lands on the next spawn. Images are enabled here via `ENABLE_IMAGES=true`
/// (the gateway defaults them off, which otherwise 404s the `/v1/images` endpoints),
/// and the upstream response-header timeout is raised from its 10s default so
/// non-streaming image generation (which OpenAI answers in 20-60s) doesn't 502.
#[tauri::command]
async fn start_gateway(state: tauri::State<'_, AppState>, force: bool) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }
    let we_own_one = state
        .gateway_child
        .lock()
        .map_err(|e| e.to_string())?
        .is_some();

    if !we_own_one && gateway_reachable() {
        return Ok(());
    }

    {
        let mut guard = state.gateway_child.lock().map_err(|e| e.to_string())?;
        if let Some(mut old) = guard.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
    }

    let bin = ensure_gateway_binary(force)?;
    let child = std::process::Command::new(&bin)
        .envs(auth_env())
        .envs(collector_env())
        .env("TELEMETRY_ENABLED", "true")
        .env("TELEMETRY_TRACING_ENABLED", "true")
        .env("ENABLE_IMAGES", "true")
        .env("CLIENT_RESPONSE_HEADER_TIMEOUT", "120s")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start gateway: {}", e))?;

    *state.gateway_child.lock().map_err(|e| e.to_string())? = Some(child);
    Ok(())
}

// --- Update checks ---
// GitHub allows 60 API requests/hour unauthenticated, 5000 through an authenticated
// `gh`. The frontend caches results for 6h, so neither ceiling is anywhere near.

#[derive(Clone, serde::Serialize)]
struct UpdateInfo {
    name: String,
    current: String,
    latest: String,
    outdated: bool,
}

/// Last whitespace token of the first line, minus a leading `v`. Covers
/// `infer v0.158.0`, `infer version v0.158.0` and the gateway's bare `0.44.0`.
fn parse_version(output: &str) -> Option<String> {
    let token = output.lines().next()?.split_whitespace().next_back()?;
    let version = token.trim_start_matches('v');
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

fn installed_version(bin: &std::path::Path, arg: &str) -> Option<String> {
    let out = std::process::Command::new(bin).arg(arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

/// Latest release tag for a repo, via `gh` when it is available and authenticated,
/// falling back to the public GitHub API.
fn latest_tag(repo: &str) -> Option<String> {
    let gh = std::process::Command::new("gh")
        .args([
            "release",
            "list",
            "--repo",
            repo,
            "--limit",
            "1",
            "--json",
            "tagName",
            "-q",
            ".[0].tagName",
        ])
        .output();
    if let Ok(out) = gh
        && out.status.success()
        && let Some(tag) = parse_version(&String::from_utf8_lossy(&out.stdout))
    {
        return Some(tag);
    }

    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let resp = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .call()
        .ok()?;
    let mut text = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut text)
        .ok()?;
    let body: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_version(body.get("tag_name")?.as_str()?)
}

/// A component is outdated when both versions are known and differ. Locally built
/// (`dev`) binaries and failed lookups never report an update.
fn is_outdated(current: &str, latest: &str) -> bool {
    current != "dev" && !latest.is_empty() && current != latest
}

#[tauri::command]
async fn check_updates(app: tauri::AppHandle) -> Result<Vec<UpdateInfo>, String> {
    let mut updates: Vec<UpdateInfo> = tokio::task::spawn_blocking(|| {
        let components = [
            ("CLI", infer_bin_path(), "inference-gateway/cli", "version"),
            (
                "Gateway",
                gateway_bin_path(),
                "inference-gateway/inference-gateway",
                "--version",
            ),
        ];
        components
            .into_iter()
            .filter_map(|(name, bin, repo, arg)| {
                let current = installed_version(&bin, arg)?;
                let latest = latest_tag(repo).unwrap_or_default();
                Some(UpdateInfo {
                    outdated: is_outdated(&current, &latest),
                    name: name.to_string(),
                    current,
                    latest,
                })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    updates.push(desktop_update_info(&app).await);
    Ok(updates)
}

// --- Desktop app self-update ---
// The Tauri updater plugin reads `latest.json` published with every GitHub
// release, verifies the bundle against the minisign public key baked into
// `plugins.updater`, swaps it in place and relaunches. Everything runs from
// Rust, so no updater/process capability is needed in the webview.

/// Updater for the app itself, with the same 10s ceiling the CLI/gateway
/// version lookups use so a slow endpoint cannot stall an update check.
fn desktop_updater(app: &tauri::AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    app.updater_builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

/// Version pair for the desktop app. Debug builds report `dev`, so a locally
/// built app never offers to replace itself with a release bundle.
async fn desktop_update_info(app: &tauri::AppHandle) -> UpdateInfo {
    let current = if cfg!(debug_assertions) {
        "dev".to_string()
    } else {
        app.package_info().version.to_string()
    };
    let latest = match desktop_updater(app) {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => update.version,
            Ok(None) => current.clone(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };
    UpdateInfo {
        outdated: is_outdated(&current, &latest),
        name: "Desktop".to_string(),
        current,
        latest,
    }
}

/// Download, verify and install the desktop app update, then relaunch. Does not
/// return on success.
#[tauri::command]
async fn install_desktop_update(app: tauri::AppHandle) -> Result<(), String> {
    let update = desktop_updater(&app)?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Desktop app is already up to date")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

// --- Speech-to-text (desktop-owned whisper.cpp) ---
// The desktop owns its own STT: it resolves a whisper.cpp binary (PATH first,
// like the CLI's whisper-cli/whisper-cpp candidates; download fallback on the
// platforms that have a prebuilt asset) and downloads the GGML model from
// HuggingFace into ~/.infer/models/whisper. Audio is captured in the WebView and
// handed here as WAV bytes; whisper turns it into text. No ffmpeg, no CGO.

const WHISPER_MODEL_FILE: &str = "ggml-tiny.bin";
const WHISPER_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
const STT_BINARIES_BASE: &str =
    "https://github.com/inference-gateway/stt-binaries/releases/download/v0.2.0";
/// Filename for the downloaded whisper binary; Windows needs the `.exe` to run.
const WHISPER_BIN_NAME: &str = if cfg!(windows) {
    "whisper-cli.exe"
} else {
    "whisper-cli"
};

fn whisper_model_path() -> PathBuf {
    home_dir()
        .join(".infer")
        .join("models")
        .join("whisper")
        .join(WHISPER_MODEL_FILE)
}

/// Prebuilt whisper-cli asset for this platform, or `None` where none is published.
fn stt_bin_asset() -> Option<String> {
    stt_bin_asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn stt_bin_asset_for(os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    match os {
        "linux" => Some(format!("whisper-cli-linux-{}", arch)),
        "macos" => Some(format!("whisper-cli-darwin-{}", arch)),
        "windows" if arch == "amd64" => Some("whisper-cli-windows-amd64.exe".into()),
        _ => None,
    }
}

fn is_executable_file(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// First matching executable named `name` on PATH.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{}.exe", name));
            if is_executable_file(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

/// Resolve the whisper binary: WHISPER_BIN override, then whisper-cli/whisper-cpp
/// on PATH, then the desktop-downloaded copy in ~/.infer/bin. `None` drives the
/// greyed-out mic in the UI.
fn whisper_bin_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WHISPER_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    for name in ["whisper-cli", "whisper-cpp"] {
        if let Some(p) = find_on_path(name) {
            return Some(p);
        }
    }
    let owned = home_dir().join(".infer").join("bin").join(WHISPER_BIN_NAME);
    is_executable_file(&owned).then_some(owned)
}

/// Stream `reader` into `tmp`, reporting (received, total) progress. Leaves `tmp`
/// on error for the caller to remove.
fn stream_to_file(
    mut reader: impl Read,
    tmp: &std::path::Path,
    total: u64,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let mut file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 8192];
    let mut received = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        received += n as u64;
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        on_progress(received, total);
    }
    Ok(())
}

/// Ensure the GGML model exists, downloading it once (atomic temp -> rename).
/// Not checksum-verified: HuggingFace's resolve/ layout has no per-file digest,
/// matching the CLI.
fn ensure_whisper_model(on_progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let dest = whisper_model_path();
    if dest.exists() {
        return Ok(dest);
    }
    let dir = dest.parent().ok_or("bad model path")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.partial", WHISPER_MODEL_FILE));
    let resp = ureq::get(WHISPER_MODEL_URL)
        .call()
        .map_err(|e| format!("Failed to download model: {}", e))?;
    let total: u64 = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let reader = resp.into_body().into_reader();
    if let Err(e) = stream_to_file(reader, &tmp, total, on_progress) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Download and checksum-verify the prebuilt whisper-cli into ~/.infer/bin.
/// Copies check_and_install_cli's verify-then-rename-then-chmod flow.
fn download_whisper_binary(on_event: &Channel<ProgressEvent>) -> Result<PathBuf, String> {
    let asset = stt_bin_asset().ok_or_else(|| {
        format!(
            "No prebuilt whisper-cli for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let bin_dir = home_dir().join(".infer").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    let dest = bin_dir.join(WHISPER_BIN_NAME);
    let tmp = bin_dir.join("whisper-cli.tmp");

    let _ = on_event.send(ProgressEvent::Downloading {
        received: 0,
        total: 0,
    });
    download(&format!("{}/{}", STT_BINARIES_BASE, asset), &tmp, on_event)?;

    let _ = on_event.send(ProgressEvent::Verifying);
    let checksums_resp = ureq::get(&format!("{}/checksums.txt", STT_BINARIES_BASE))
        .call()
        .map_err(|e| format!("Failed to download checksums.txt: {}", e))?;
    let mut checksums_text = String::new();
    checksums_resp
        .into_body()
        .into_reader()
        .read_to_string(&mut checksums_text)
        .map_err(|e| e.to_string())?;

    let expected = find_checksum(&checksums_text, &asset)
        .ok_or_else(|| format!("Checksum not found for {}", asset))?;
    let actual = sha256_digest(&tmp)?;
    if actual != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "Checksum mismatch for {}: expected {}, got {}",
            asset, expected, actual
        ));
    }

    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms).map_err(|e| e.to_string())?;
    }
    Ok(dest)
}

/// Strip whisper's bracketed non-speech markers ([BLANK_AUDIO], (music), ...)
/// and collapse whitespace. A no-speech clip therefore yields "".
fn clean_transcript(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut sq = 0i32;
    let mut rp = 0i32;
    for c in raw.chars() {
        match c {
            '[' => sq += 1,
            ']' if sq > 0 => sq -= 1,
            '(' => rp += 1,
            ')' if rp > 0 => rp -= 1,
            _ if sq == 0 && rp == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn next_tmp_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn run_whisper(
    bin: &std::path::Path,
    model: &std::path::Path,
    wav: &std::path::Path,
) -> Result<String, String> {
    let output = std::process::Command::new(bin)
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .arg("-nt")
        .arg("-np")
        .output()
        .map_err(|e| format!("Failed to run whisper: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "whisper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(clean_transcript(&String::from_utf8_lossy(&output.stdout)))
}

#[derive(Clone, serde::Serialize)]
struct SttStatus {
    binary: bool,
    model: bool,
    downloadable: bool,
    hint: String,
}

/// Whether STT is usable and, if not, why — so the frontend can grey the mic and
/// show the right tooltip.
#[tauri::command]
async fn stt_status() -> Result<SttStatus, String> {
    if mock_mode() {
        return Ok(SttStatus {
            binary: true,
            model: true,
            downloadable: false,
            hint: String::new(),
        });
    }
    let binary = whisper_bin_path().is_some();
    let downloadable = stt_bin_asset().is_some();
    let hint = if !binary && !downloadable {
        "Voice input isn't available on this platform".into()
    } else {
        String::new()
    };
    Ok(SttStatus {
        binary,
        model: whisper_model_path().exists(),
        downloadable,
        hint,
    })
}

/// Ensure the whisper binary (download where a prebuilt exists) and model are
/// present, streaming progress through the same channel as the CLI install UI.
#[tauri::command]
async fn prepare_stt(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if mock_mode() {
        let _ = on_event.send(ProgressEvent::Ready);
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        let _ = on_event.send(ProgressEvent::Checking);
        if whisper_bin_path().is_none() {
            let _ = on_event.send(ProgressEvent::Installing);
            download_whisper_binary(&on_event)?;
        }
        ensure_whisper_model(|received, total| {
            let _ = on_event.send(ProgressEvent::Downloading { received, total });
        })?;
        let _ = on_event.send(ProgressEvent::Ready);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Transcribe WAV bytes (16kHz mono, produced by the WebView) to text.
#[tauri::command]
async fn transcribe_audio(wav: Vec<u8>) -> Result<String, String> {
    if mock_mode() {
        return Ok("this is a mock transcription".into());
    }
    tokio::task::spawn_blocking(move || {
        let bin = whisper_bin_path().ok_or("whisper-cli not found")?;
        let model = ensure_whisper_model(|_, _| {})?;
        let tmp = std::env::temp_dir().join(format!(
            "infer-stt-{}-{}.wav",
            std::process::id(),
            next_tmp_id()
        ));
        std::fs::write(&tmp, &wav).map_err(|e| e.to_string())?;
        let result = run_whisper(&bin, &model, &tmp);
        let _ = std::fs::remove_file(&tmp);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- OTLP collector ---
// stdlib TCP listener, no server framework. Accepts OTLP/HTTP in both
// http/protobuf (what the Go SDK exporters in the CLI and gateway send -
// Go never implemented http/json) and http/json, dispatched by Content-Type.
// The protobuf path is a hand-rolled wire-format walker over just the trace
// fields we store; no proto codegen dependency.

const COLLECTOR_PORT: u16 = 4318;
const MAX_SPANS: usize = 5000;
const MAX_METRICS: usize = 5000;
const MAX_BODY: usize = 32 * 1024 * 1024;

/// Push into a bounded queue, evicting the oldest entries.
fn push_bounded<T>(queue: &Mutex<VecDeque<T>>, items: Vec<T>, cap: usize) {
    if items.is_empty() {
        return;
    }
    let Ok(mut guard) = queue.lock() else {
        return;
    };
    for item in items {
        if guard.len() >= cap {
            guard.pop_front();
        }
        guard.push_back(item);
    }
}

/// Extract an i64 from a JSON value that may be a number or a string.
fn json_val_i64(val: &serde_json::Value) -> i64 {
    val.as_i64()
        .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

/// Extract a string value from an OTLP AnyValue.
fn attr_value(val: &serde_json::Value) -> String {
    if let Some(s) = val.get("stringValue").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(s) = val.get("intValue").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(n) = val.get("intValue").and_then(|v| v.as_i64()) {
        return n.to_string();
    }
    if let Some(n) = val.get("doubleValue").and_then(|v| v.as_f64()) {
        return n.to_string();
    }
    if let Some(b) = val.get("boolValue").and_then(|v| v.as_bool()) {
        return b.to_string();
    }
    String::new()
}

/// Parse OTLP ExportTraceServiceRequest JSON body, extract StoredSpans.
fn extract_spans(body: &str) -> Vec<StoredSpan> {
    let val: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut spans = Vec::new();
    let Some(rs_arr) = val.get("resourceSpans").and_then(|v| v.as_array()) else {
        return spans;
    };
    for rs_item in rs_arr {
        let resource_attrs = rs_item
            .get("resource")
            .and_then(|r| r.get("attributes"))
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|attr| {
                        let key = attr.get("key").and_then(|k| k.as_str())?;
                        let val = attr_value(attr.get("value")?);
                        Some((key.to_string(), val))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let service_name = resource_attrs
            .iter()
            .find(|(k, _)| k == "service.name")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let Some(ss_arr) = rs_item.get("scopeSpans").and_then(|v| v.as_array()) else {
            continue;
        };
        for ss_item in ss_arr {
            let Some(spans_arr) = ss_item.get("spans").and_then(|v| v.as_array()) else {
                continue;
            };
            for span_val in spans_arr {
                let trace_id = span_val
                    .get("traceId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let span_id = span_val
                    .get("spanId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let parent_span_id = span_val
                    .get("parentSpanId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = span_val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let kind = span_val.get("kind").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let start_time = span_val
                    .get("startTimeUnixNano")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let end_time = span_val
                    .get("endTimeUnixNano")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let status_code = span_val
                    .get("status")
                    .and_then(|s| s.get("code"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let attributes = span_val
                    .get("attributes")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|attr| {
                                let key = attr.get("key").and_then(|k| k.as_str())?;
                                let val = attr_value(attr.get("value")?);
                                Some((key.to_string(), val))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                spans.push(StoredSpan {
                    trace_id,
                    span_id,
                    parent_span_id,
                    name,
                    kind,
                    start_time_unix_nano: start_time,
                    end_time_unix_nano: end_time,
                    attributes,
                    status_code,
                    service_name: service_name.clone(),
                });
            }
        }
    }
    spans
}

// --- OTLP protobuf decoding ---
// Hand-rolled reader for the protobuf wire format: a message is a flat
// sequence of (field number, wire type, payload) records, and nested messages
// are just length-delimited payloads decoded the same way. `pb_fields` turns
// one message into that record list; the decoders below then read like the
// .proto definitions they mirror. Unknown fields are skipped; malformed input
// yields whatever parsed before the error. Field numbers are from
// opentelemetry-proto v1 (trace.proto, metrics.proto, common.proto) and named
// as MESSAGE_FIELD below.

const WIRE_VARINT: u64 = 0;
const WIRE_FIXED64: u64 = 1;
const WIRE_LEN_DELIMITED: u64 = 2;
const WIRE_FIXED32: u64 = 5;

// trace.proto
const EXPORT_TRACE_REQUEST_RESOURCE_SPANS: u32 = 1;
const RESOURCE_SPANS_RESOURCE: u32 = 1;
const RESOURCE_SPANS_SCOPE_SPANS: u32 = 2;
const SCOPE_SPANS_SPANS: u32 = 2;
const SPAN_TRACE_ID: u32 = 1;
const SPAN_SPAN_ID: u32 = 2;
const SPAN_PARENT_SPAN_ID: u32 = 4;
const SPAN_NAME: u32 = 5;
const SPAN_KIND: u32 = 6;
const SPAN_START_TIME_UNIX_NANO: u32 = 7;
const SPAN_END_TIME_UNIX_NANO: u32 = 8;
const SPAN_ATTRIBUTES: u32 = 9;
const SPAN_STATUS: u32 = 15;
const STATUS_CODE: u32 = 3;

// common.proto / resource.proto
const RESOURCE_ATTRIBUTES: u32 = 1;
const KEY_VALUE_KEY: u32 = 1;
const KEY_VALUE_VALUE: u32 = 2;
const ANY_VALUE_STRING: u32 = 1;
const ANY_VALUE_BOOL: u32 = 2;
const ANY_VALUE_INT: u32 = 3;
const ANY_VALUE_DOUBLE: u32 = 4;

// metrics.proto
const EXPORT_METRICS_REQUEST_RESOURCE_METRICS: u32 = 1;
const RESOURCE_METRICS_SCOPE_METRICS: u32 = 2;
const SCOPE_METRICS_METRICS: u32 = 2;
const METRIC_NAME: u32 = 1;
const METRIC_UNIT: u32 = 3;
const METRIC_GAUGE: u32 = 5;
const METRIC_SUM: u32 = 7;
const METRIC_HISTOGRAM: u32 = 9;
const DATA_POINTS: u32 = 1;
const NUMBER_POINT_TIME_UNIX_NANO: u32 = 3;
const NUMBER_POINT_AS_DOUBLE: u32 = 4;
const NUMBER_POINT_AS_INT: u32 = 6;
const NUMBER_POINT_ATTRIBUTES: u32 = 7;
const HISTOGRAM_POINT_TIME_UNIX_NANO: u32 = 3;
const HISTOGRAM_POINT_COUNT: u32 = 4;
const HISTOGRAM_POINT_SUM: u32 = 5;
const HISTOGRAM_POINT_ATTRIBUTES: u32 = 9;

/// One decoded protobuf field payload.
enum PbValue<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
}

/// Read one varint: 7 payload bits per byte, least-significant group first;
/// a set high bit means another byte follows.
fn pb_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        val |= u64::from(byte & 0x7f) << shift;
        let has_more_bytes = byte & 0x80 != 0;
        if !has_more_bytes {
            return Some(val);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

/// Decode one message into its list of (field number, payload) records.
/// Each record starts with a tag varint packing `field_number << 3 | wire_type`.
fn pb_fields(buf: &[u8]) -> Vec<(u32, PbValue<'_>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(tag) = pb_varint(buf, &mut pos) else {
            break;
        };
        let field_number = (tag >> 3) as u32;
        let wire_type = tag & 7;
        match wire_type {
            WIRE_VARINT => match pb_varint(buf, &mut pos) {
                Some(v) => out.push((field_number, PbValue::Varint(v))),
                None => break,
            },
            WIRE_FIXED64 => {
                let Some(bytes) = buf.get(pos..pos + 8) else {
                    break;
                };
                let v = u64::from_le_bytes(bytes.try_into().expect("slice is 8 bytes"));
                out.push((field_number, PbValue::Fixed64(v)));
                pos += 8;
            }
            WIRE_LEN_DELIMITED => {
                let Some(len) = pb_varint(buf, &mut pos) else {
                    break;
                };
                let end = pos + len as usize;
                let Some(bytes) = buf.get(pos..end) else {
                    break;
                };
                out.push((field_number, PbValue::Bytes(bytes)));
                pos = end;
            }
            WIRE_FIXED32 => {
                if buf.len() < pos + 4 {
                    break;
                }
                pos += 4;
            }
            _ => break,
        }
    }
    out
}

fn pb_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Render an OTLP AnyValue message to a string, mirroring `attr_value`.
fn pb_any_value(buf: &[u8]) -> String {
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (ANY_VALUE_STRING, PbValue::Bytes(b)) => {
                return String::from_utf8_lossy(b).into_owned();
            }
            (ANY_VALUE_BOOL, PbValue::Varint(v)) => return (v != 0).to_string(),
            (ANY_VALUE_INT, PbValue::Varint(v)) => return (v as i64).to_string(),
            (ANY_VALUE_DOUBLE, PbValue::Fixed64(v)) => return f64::from_bits(v).to_string(),
            _ => {}
        }
    }
    String::new()
}

/// Parse a KeyValue message into a (key, value) pair.
fn pb_key_value(buf: &[u8]) -> Option<(String, String)> {
    let mut key = None;
    let mut value = String::new();
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (KEY_VALUE_KEY, PbValue::Bytes(b)) => {
                key = Some(String::from_utf8_lossy(b).into_owned())
            }
            (KEY_VALUE_VALUE, PbValue::Bytes(b)) => value = pb_any_value(b),
            _ => {}
        }
    }
    key.map(|k| (k, value))
}

fn pb_span(buf: &[u8], service_name: &str) -> StoredSpan {
    let mut span = StoredSpan {
        trace_id: String::new(),
        span_id: String::new(),
        parent_span_id: String::new(),
        name: String::new(),
        kind: 0,
        start_time_unix_nano: 0,
        end_time_unix_nano: 0,
        attributes: Vec::new(),
        status_code: 0,
        service_name: service_name.to_string(),
    };
    for (field, val) in pb_fields(buf) {
        match (field, val) {
            (SPAN_TRACE_ID, PbValue::Bytes(b)) => span.trace_id = pb_hex(b),
            (SPAN_SPAN_ID, PbValue::Bytes(b)) => span.span_id = pb_hex(b),
            (SPAN_PARENT_SPAN_ID, PbValue::Bytes(b)) => span.parent_span_id = pb_hex(b),
            (SPAN_NAME, PbValue::Bytes(b)) => span.name = String::from_utf8_lossy(b).into_owned(),
            (SPAN_KIND, PbValue::Varint(v)) => span.kind = v as i32,
            (SPAN_START_TIME_UNIX_NANO, PbValue::Fixed64(v)) => span.start_time_unix_nano = v,
            (SPAN_END_TIME_UNIX_NANO, PbValue::Fixed64(v)) => span.end_time_unix_nano = v,
            (SPAN_ATTRIBUTES, PbValue::Bytes(b)) => span.attributes.extend(pb_key_value(b)),
            (SPAN_STATUS, PbValue::Bytes(status)) => {
                for (field, val) in pb_fields(status) {
                    if let (STATUS_CODE, PbValue::Varint(code)) = (field, val) {
                        span.status_code = code as i32;
                    }
                }
            }
            _ => {}
        }
    }
    span
}

/// Parse a protobuf ExportTraceServiceRequest body, extract StoredSpans.
fn extract_spans_proto(body: &[u8]) -> Vec<StoredSpan> {
    let mut spans = Vec::new();
    for (field, val) in pb_fields(body) {
        let (EXPORT_TRACE_REQUEST_RESOURCE_SPANS, PbValue::Bytes(resource_spans)) = (field, val)
        else {
            continue;
        };
        let mut service_name = String::new();
        let fields = pb_fields(resource_spans);
        for (field, val) in &fields {
            if let (RESOURCE_SPANS_RESOURCE, PbValue::Bytes(resource)) = (*field, val) {
                for (field, val) in pb_fields(resource) {
                    if let (RESOURCE_ATTRIBUTES, PbValue::Bytes(kv)) = (field, val)
                        && let Some((k, v)) = pb_key_value(kv)
                        && k == "service.name"
                    {
                        service_name = v;
                    }
                }
            }
        }
        for (field, val) in &fields {
            if let (RESOURCE_SPANS_SCOPE_SPANS, PbValue::Bytes(scope_spans)) = (*field, val) {
                for (field, val) in pb_fields(scope_spans) {
                    if let (SCOPE_SPANS_SPANS, PbValue::Bytes(span)) = (field, val) {
                        spans.push(pb_span(span, &service_name));
                    }
                }
            }
        }
    }
    spans
}

/// Which OTLP data-point message a metric carries. Number points come from
/// gauges and sums; histogram points share field numbers for time but not
/// for value, count, or attributes.
#[derive(Clone, Copy)]
enum PointKind {
    Number,
    Histogram,
}

/// Parse a NumberDataPoint or HistogramDataPoint into a StoredMetric.
fn pb_data_point(buf: &[u8], kind: PointKind, name: &str, unit: &str) -> StoredMetric {
    let mut point = StoredMetric {
        name: name.to_string(),
        unit: unit.to_string(),
        value: 0.0,
        count: 1,
        attributes: Vec::new(),
        time_unix_nano: 0,
    };
    for (field, val) in pb_fields(buf) {
        match (kind, field, val) {
            (PointKind::Number, NUMBER_POINT_TIME_UNIX_NANO, PbValue::Fixed64(v))
            | (PointKind::Histogram, HISTOGRAM_POINT_TIME_UNIX_NANO, PbValue::Fixed64(v)) => {
                point.time_unix_nano = v
            }
            (PointKind::Number, NUMBER_POINT_AS_DOUBLE, PbValue::Fixed64(v)) => {
                point.value = f64::from_bits(v)
            }
            (PointKind::Number, NUMBER_POINT_AS_INT, PbValue::Fixed64(v)) => {
                point.value = v as i64 as f64
            }
            (PointKind::Number, NUMBER_POINT_ATTRIBUTES, PbValue::Bytes(b)) => {
                point.attributes.extend(pb_key_value(b))
            }
            (PointKind::Histogram, HISTOGRAM_POINT_COUNT, PbValue::Fixed64(v)) => point.count = v,
            (PointKind::Histogram, HISTOGRAM_POINT_SUM, PbValue::Fixed64(v)) => {
                point.value = f64::from_bits(v)
            }
            (PointKind::Histogram, HISTOGRAM_POINT_ATTRIBUTES, PbValue::Bytes(b)) => {
                point.attributes.extend(pb_key_value(b))
            }
            _ => {}
        }
    }
    point
}

/// Parse one Metric message. Exponential histograms and summaries are skipped.
fn pb_metric(metric: &[u8]) -> Vec<StoredMetric> {
    let fields = pb_fields(metric);
    let mut name = String::new();
    let mut unit = String::new();
    for (field, val) in &fields {
        match (*field, val) {
            (METRIC_NAME, PbValue::Bytes(b)) => name = String::from_utf8_lossy(b).into_owned(),
            (METRIC_UNIT, PbValue::Bytes(b)) => unit = String::from_utf8_lossy(b).into_owned(),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (field, val) in &fields {
        let kind = match *field {
            METRIC_GAUGE | METRIC_SUM => PointKind::Number,
            METRIC_HISTOGRAM => PointKind::Histogram,
            _ => continue,
        };
        let PbValue::Bytes(data) = val else {
            continue;
        };
        for (field, val) in pb_fields(data) {
            let (DATA_POINTS, PbValue::Bytes(point)) = (field, val) else {
                continue;
            };
            out.push(pb_data_point(point, kind, &name, &unit));
        }
    }
    out
}

/// Parse a protobuf ExportMetricsServiceRequest body into StoredMetrics.
fn extract_metrics_proto(body: &[u8]) -> Vec<StoredMetric> {
    let mut out = Vec::new();
    for (field, val) in pb_fields(body) {
        let (EXPORT_METRICS_REQUEST_RESOURCE_METRICS, PbValue::Bytes(resource_metrics)) =
            (field, val)
        else {
            continue;
        };
        for (field, val) in pb_fields(resource_metrics) {
            let (RESOURCE_METRICS_SCOPE_METRICS, PbValue::Bytes(scope_metrics)) = (field, val)
            else {
                continue;
            };
            for (field, val) in pb_fields(scope_metrics) {
                let (SCOPE_METRICS_METRICS, PbValue::Bytes(metric)) = (field, val) else {
                    continue;
                };
                out.extend(pb_metric(metric));
            }
        }
    }
    out
}

/// Handle a single OTLP HTTP request on the collector's TCP socket. The
/// response carries `Connection: close`; Go's http client replays the export
/// on a fresh connection, so one-request-per-connection is safe.
fn handle_otlp_request(
    stream: &mut std::net::TcpStream,
    spans_store: &Mutex<VecDeque<StoredSpan>>,
    metrics_store: &Mutex<VecDeque<StoredMetric>>,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut data: Vec<u8> = Vec::with_capacity(8192);
    let mut buf = [0u8; 65536];
    let body_start = loop {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                if data.len() > 64 * 1024 {
                    return;
                }
            }
            Err(_) => return,
        }
    };
    let header = String::from_utf8_lossy(&data[..body_start]).to_string();
    if !header.starts_with("POST") {
        return;
    }
    let header_line = |name: &str| -> Option<String> {
        header
            .lines()
            .find(|l| l.to_lowercase().starts_with(name))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_lowercase()))
    };
    let content_len = header_line("content-length:")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_len > MAX_BODY {
        return;
    }
    while data.len() < body_start + content_len {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => return,
        }
    }
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}";
    let _ = stream.write_all(response);
    let _ = stream.flush();
    // Traces and metrics exports share the outer protobuf shape - Metric.name
    // sits at Span.trace_id's field number - so the URL path decides the
    // parser; any other path is acked but never parsed.
    let path = header.split_whitespace().nth(1).unwrap_or("");
    let body = &data[body_start..];
    if path.starts_with("/v1/traces") {
        let spans = if header_line("content-type:").is_some_and(|ct| ct.contains("json")) {
            extract_spans(&String::from_utf8_lossy(body))
        } else {
            extract_spans_proto(body)
        };
        push_bounded(spans_store, spans, MAX_SPANS);
    } else if path.starts_with("/v1/metrics") {
        push_bounded(metrics_store, extract_metrics_proto(body), MAX_METRICS);
    }
}

/// Spawn the OTLP collector thread.
fn start_collector(
    spans_store: Arc<Mutex<VecDeque<StoredSpan>>>,
    metrics_store: Arc<Mutex<VecDeque<StoredMetric>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let addr = format!("127.0.0.1:{}", COLLECTOR_PORT);
        let listener = match std::net::TcpListener::bind(&addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("desktop: OTLP collector failed to bind {}: {}", addr, e);
                return;
            }
        };
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => handle_otlp_request(&mut stream, &spans_store, &metrics_store),
                Err(e) => {
                    eprintln!("desktop: OTLP collector accept error: {}", e);
                    break;
                }
            }
        }
    })
}

/// Return all stored traces, newest first.
#[tauri::command]
fn get_traces(state: tauri::State<'_, AppState>) -> Result<Vec<StoredSpan>, String> {
    let guard = state.stored_traces.lock().map_err(|e| e.to_string())?;
    let mut spans: Vec<StoredSpan> = guard.iter().cloned().collect();
    spans.reverse();
    Ok(spans)
}

/// Return all stored metric data points in arrival order.
#[tauri::command]
fn get_metrics(state: tauri::State<'_, AppState>) -> Result<Vec<StoredMetric>, String> {
    let guard = state.stored_metrics.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}

pub fn run() {
    let stored_traces: Arc<Mutex<VecDeque<StoredSpan>>> = Arc::new(Mutex::new(VecDeque::new()));
    let stored_metrics: Arc<Mutex<VecDeque<StoredMetric>>> = Arc::new(Mutex::new(VecDeque::new()));
    let _collector = start_collector(Arc::clone(&stored_traces), Arc::clone(&stored_metrics));

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            running_children: Mutex::new(std::collections::HashMap::new()),
            child_stdins: Mutex::new(std::collections::HashMap::new()),
            gateway_child: Mutex::new(None),
            stored_traces,
            stored_metrics,
        })
        .invoke_handler(tauri::generate_handler![
            check_and_install_cli,
            send_message,
            send_approval,
            cancel_agent,
            list_conversations,
            get_conversation,
            delete_conversation,
            list_models,
            get_auth,
            set_auth,
            get_config,
            set_config,
            set_default_model,
            start_gateway,
            check_updates,
            install_desktop_update,
            stt_status,
            prepare_stt,
            transcribe_audio,
            list_a2a_agents,
            add_a2a_agent,
            remove_a2a_agent,
            set_a2a_agent_model,
            read_history,
            append_history,
            save_image,
            get_traces,
            get_metrics,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = app_handle.state::<AppState>();
                if let Ok(mut guard) = state.gateway_child.lock()
                    && let Some(mut child) = guard.take()
                {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                if let Ok(mut children) = state.running_children.lock() {
                    for (_, mut child) in children.drain() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_image_source_guards_scope() {
        let home = PathBuf::from("/Users/me");
        assert!(safe_image_source("/Users/me/proj/.infer/tmp/cat.png", &home).is_ok());
        assert!(safe_image_source("/Users/me/.infer/tmp/cat.png", &home).is_ok());
        assert!(safe_image_source("/etc/passwd", &home).is_err());
        assert!(safe_image_source("/Users/me/.infer/tmp/../../../etc/passwd", &home).is_err());
        assert!(safe_image_source("/Users/me/Pictures/cat.png", &home).is_err());
    }

    #[test]
    fn test_asset_name_mapping() {
        assert_eq!(asset_name_for("linux", "x86_64"), Some("infer-linux-amd64"));
        assert_eq!(
            asset_name_for("linux", "aarch64"),
            Some("infer-linux-arm64")
        );
        assert_eq!(
            asset_name_for("macos", "x86_64"),
            Some("infer-darwin-amd64")
        );
        assert_eq!(
            asset_name_for("macos", "aarch64"),
            Some("infer-darwin-arm64")
        );
        assert_eq!(
            asset_name_for("windows", "x86_64"),
            Some("infer-windows-amd64")
        );
        assert_eq!(
            asset_name_for("windows", "aarch64"),
            Some("infer-windows-arm64")
        );

        assert_eq!(asset_name_for("linux", "i686"), None);
        assert_eq!(asset_name_for("freebsd", "x86_64"), None);
    }

    /// Test helper that mirrors asset_name() but takes explicit os/arch strings.
    fn asset_name_for(os: &str, arch: &str) -> Option<&'static str> {
        match (os, arch) {
            ("linux", "x86_64") => Some("infer-linux-amd64"),
            ("linux", "aarch64") => Some("infer-linux-arm64"),
            ("macos", "x86_64") => Some("infer-darwin-amd64"),
            ("macos", "aarch64") => Some("infer-darwin-arm64"),
            ("windows", "x86_64") => Some("infer-windows-amd64"),
            ("windows", "aarch64") => Some("infer-windows-arm64"),
            _ => None,
        }
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("infer v0.158.0\n"), Some("0.158.0".into()));
        assert_eq!(
            parse_version("infer version v0.158.0\n"),
            Some("0.158.0".into())
        );
        assert_eq!(parse_version("0.44.0\n"), Some("0.44.0".into()));
        assert_eq!(parse_version("v0.44.0\n"), Some("0.44.0".into()));
        assert_eq!(parse_version("dev\n"), Some("dev".into()));
        assert_eq!(
            parse_version("infer v0.158.0\nextra line\n"),
            Some("0.158.0".into())
        );
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("  \n"), None);
        assert_eq!(parse_version("v\n"), None);
    }

    #[test]
    fn test_is_outdated() {
        assert!(is_outdated("0.158.0", "0.158.1"));
        assert!(!is_outdated("0.158.1", "0.158.1"));
        assert!(!is_outdated("dev", "0.158.1"));
        assert!(!is_outdated("0.158.0", ""));
    }

    #[test]
    fn test_find_checksum() {
        let text = "abc123deadbeef  infer-linux-amd64\ndef456cafebabe  infer-darwin-arm64\n";
        assert_eq!(
            find_checksum(text, "infer-linux-amd64"),
            Some("abc123deadbeef".into())
        );
        assert_eq!(
            find_checksum(text, "infer-darwin-arm64"),
            Some("def456cafebabe".into())
        );
        assert_eq!(find_checksum(text, "infer-windows-amd64"), None);
    }

    #[test]
    fn test_checksum_mismatch_detected() {
        let text =
            "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8  infer-linux-amd64\n";
        let found = find_checksum(text, "infer-linux-amd64").unwrap();
        let hello_hash = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_ne!(found, hello_hash, "mismatch should be detected");
    }

    // --- AG-UI parser tests ---

    /// Helper: feed lines to an AgentParser and collect emitted events.
    fn parse_all(lines: &[&str]) -> (Vec<AgentEvent>, Option<String>) {
        let mut p = AgentParser::new(None);
        let events: Vec<AgentEvent> = lines
            .iter()
            .filter_map(|l| p.parse_line(l.trim()))
            .collect();
        let sid = p.take_session_id();
        (events, sid)
    }

    #[test]
    fn test_parse_run_started() {
        let (events, sid) =
            parse_all(&[r#"{"type":"RUN_STARTED","threadId":"session-42","runId":"run-1"}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Info { message } if message == "Session started"));
        assert_eq!(sid, Some("session-42".into()));
    }

    #[test]
    fn test_parse_run_started_no_session_id() {
        let (events, sid) = parse_all(&[r#"{"type":"RUN_STARTED","threadId":"","runId":"run-1"}"#]);
        assert_eq!(events.len(), 1);
        assert!(sid.is_none());
    }

    #[test]
    fn test_parse_text_message() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","delta":"Hello! How can I help you?","contentType":"text"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { content, .. } if content == "Hello! How can I help you?")
        );
    }

    #[test]
    fn test_parse_text_message_streams_each_content_delta() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","content":"Hello.","contentType":"text"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","content":" I am an AI.","contentType":"text"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            2,
            "each content event streams as its own delta"
        );
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { content, .. } if content == "Hello.")
        );
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, .. } if content == " I am an AI.")
        );
    }

    #[test]
    fn test_parse_reasoning_streams_as_reasoning_content() {
        let lines = &[
            r#"{"type":"REASONING_MESSAGE_START","messageId":"r1","role":"assistant"}"#,
            r#"{"type":"REASONING_MESSAGE_CONTENT","messageId":"r1","delta":"Let me think. "}"#,
            r#"{"type":"REASONING_MESSAGE_CONTENT","messageId":"r1","delta":"Step 1."}"#,
            r#"{"type":"REASONING_MESSAGE_END","messageId":"r1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a1","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a1","delta":"Answer."}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 3, "two reasoning deltas + one content delta");
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { reasoning_content: Some(r), content, .. } if r == "Let me think. " && content.is_empty())
        );
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { reasoning_content: Some(r), .. } if r == "Step 1.")
        );
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, reasoning_content: None, .. } if content == "Answer.")
        );
    }

    #[test]
    fn test_parse_thinking_alias_maps_to_reasoning() {
        let lines =
            &[r#"{"type":"THINKING_TEXT_MESSAGE_CONTENT","messageId":"t1","delta":"pondering"}"#];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { reasoning_content: Some(r), .. } if r == "pondering")
        );
    }

    #[test]
    fn test_parse_text_message_no_content_returns_none() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_agui_real_turn_skips_user_echo_and_reads_delta() {
        let lines = &[
            r#"{"type":"RUN_STARTED","threadId":"sess-1","runId":"run-1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"u1","role":"user"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"u1","delta":"what is up?"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"u1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a1","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a1","delta":"All good!"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a1"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"sess-1","runId":"run-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            3,
            "expected Info + AssistantMessage + TokenUsage"
        );
        assert!(matches!(&events[0], AgentEvent::Info { message } if message == "Session started"));
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, .. } if content == "All good!"),
            "assistant reply must render from `delta`; the user echo must be skipped"
        );
        assert!(
            matches!(&events[2], AgentEvent::TokenUsage { .. }),
            "RUN_FINISHED emits TokenUsage"
        );
    }

    #[test]
    fn test_parse_agui_resumed_turn_renders_reply_after_snapshot() {
        let lines = &[
            r#"{"type":"RUN_STARTED","threadId":"sess-1","runId":"run-2"}"#,
            r#"{"type":"MESSAGES_SNAPSHOT","messages":[{"role":"user","content":"earlier"},{"role":"assistant","content":"earlier reply"}]}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"u2","role":"user"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"u2","delta":"and now?"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"u2"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a2","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a2","delta":"Here is more."}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a2"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"sess-1","runId":"run-2"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            3,
            "expected Info + AssistantMessage + TokenUsage"
        );
        assert!(matches!(&events[0], AgentEvent::Info { message } if message == "Session started"));
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, .. } if content == "Here is more."),
            "resumed reply must render from `delta`"
        );
        assert!(
            matches!(&events[2], AgentEvent::TokenUsage { .. }),
            "RUN_FINISHED emits TokenUsage"
        );
    }

    #[test]
    fn test_parse_tool_call_triad() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","toolCallName":"Bash"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","delta":"{\"command\":\"ls\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-1");
                assert_eq!(tool_calls[0].name, "Bash");
                assert_eq!(tool_calls[0].args, "{\"command\":\"ls\"}");
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_parse_tool_call_with_text() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1","content":"Let me check that file."}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","name":"read_file"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","args":"{\"path\":\"/tmp/test\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-2","name":"list_dir"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-2","args":"{\"path\":\"/tmp\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-2"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 3);
        // Streamed text arrives first (on TEXT_MESSAGE_START), then each tool call.
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content == "Let me check that file." && tool_calls.is_empty())
        );
        match &events[1] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-1");
                assert_eq!(tool_calls[0].name, "read_file");
            }
            _ => panic!("expected AssistantMessage for first tool call"),
        }
        match &events[2] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-2");
                assert_eq!(tool_calls[0].name, "list_dir");
            }
            _ => panic!("expected AssistantMessage for second tool call"),
        }
    }

    #[test]
    fn test_parse_tool_call_result() {
        let (events, _) = parse_all(&[
            r#"{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":"{\"tool_name\":\"read_file\",\"success\":true}"}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolResult { content, tool_call_id }
            if content == "{\"tool_name\":\"read_file\",\"success\":true}" && tool_call_id == "call-1")
        );
    }

    #[test]
    fn test_parse_tool_call_result_empty_returns_none() {
        let (events, _) =
            parse_all(&[r#"{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":""}"#]);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_run_error() {
        let (events, _) = parse_all(&[r#"{"type":"RUN_ERROR","message":"Gateway unreachable"}"#]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::AgentError { message } if message == "Gateway unreachable")
        );
    }

    #[test]
    fn test_parse_run_finished_returns_token_usage() {
        let (events, _) = parse_all(&[r#"{"type":"RUN_FINISHED","success":true,"stats":{}}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::TokenUsage { .. }));
    }

    #[test]
    fn test_parse_state_snapshot_skipped() {
        let (events, _) =
            parse_all(&[r#"{"type":"STATE_SNAPSHOT","todos":[{"id":"1","content":"test"}]}"#]);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_messages_snapshot_skipped() {
        let (events, _) = parse_all(&[r#"{"type":"MESSAGES_SNAPSHOT","messages":[]}"#]);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_custom_approval_request() {
        let (events, _) = parse_all(&[
            r#"{"type":"CUSTOM","name":"approval_request","value":{"type":"approval_request","tool_name":"read_file","tool_args":"{\"path\":\"/tmp/test\"}","tool_call_id":"call-1"}}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ApprovalRequest { tool_name, tool_args, tool_call_id }
            if tool_name == "read_file" && tool_args == "{\"path\":\"/tmp/test\"}" && tool_call_id == "call-1")
        );
    }

    #[test]
    fn test_parse_custom_unknown_type_is_raw() {
        let (events, _) =
            parse_all(&[r#"{"type":"CUSTOM","name":"other_event","value":{"key":"val"}}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_raw_line() {
        let (events, _) = parse_all(&["this is not valid json at all"]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::RawLine { line: l } if l == "this is not valid json at all")
        );
    }

    #[test]
    fn test_parse_unknown_type_is_raw() {
        let (events, _) = parse_all(&[r#"{"type":"UNKNOWN_EVENT","data":"something"}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_empty_line_returns_raw() {
        let (events, _) = parse_all(&[""]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_agui_fixture() {
        let fixture = r#"{"type":"RUN_STARTED","threadId":"session-42","runId":"run-1"}
{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1","content":"I'll look that up for you."}
{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","name":"search"}
{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","args":"{\"q\":\"test\"}"}
{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}
{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}
{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":"{\"tool_name\":\"search\",\"success\":true}"}
{"type":"RUN_ERROR","message":"API key not found"}"#;

        let mut p = AgentParser::new(None);
        let events: Vec<AgentEvent> = fixture
            .lines()
            .filter_map(|line| {
                let l = line.trim();
                if l.is_empty() { None } else { p.parse_line(l) }
            })
            .collect();

        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], AgentEvent::Info { message } if message == "Session started"));
        // Streamed text emits on TEXT_MESSAGE_START, before the tool call.
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content == "I'll look that up for you." && tool_calls.is_empty())
        );
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content.is_empty() && tool_calls.len() == 1 && tool_calls[0].name == "search")
        );
        assert!(
            matches!(&events[3], AgentEvent::ToolResult { tool_call_id, .. } if tool_call_id == "call-1")
        );
        assert!(
            matches!(&events[4], AgentEvent::AgentError { message } if message == "API key not found")
        );
        assert_eq!(p.take_session_id(), Some("session-42".into()));
    }

    #[test]
    fn test_mock_mode_parses_env() {
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
        assert!(!mock_mode());
        unsafe { std::env::set_var("DESKTOP_MOCK", "true") };
        assert!(mock_mode());
        assert!(infer_env().contains(&("INFER_GATEWAY_MOCK".to_string(), "true".to_string())));
        unsafe { std::env::set_var("DESKTOP_MOCK", "false") };
        assert!(!mock_mode());
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
    }

    /// The updater plugin looks up `{os}-{arch}` keys and hard-fails on a
    /// malformed `pub_date`, so pin the latest.json shape release.yml emits.
    /// macOS ships one universal bundle under both darwin keys.
    #[test]
    fn test_latest_json_manifest_shape() {
        let manifest = serde_json::json!({
            "version": "0.2.3",
            "pub_date": "2026-08-08T12:00:00Z",
            "platforms": {
                "darwin-aarch64": {"signature": "mac-sig", "url": "https://example.com/App_0.2.3_universal.app.tar.gz"},
                "darwin-x86_64": {"signature": "mac-sig", "url": "https://example.com/App_0.2.3_universal.app.tar.gz"},
                "linux-x86_64": {"signature": "linux-sig", "url": "https://example.com/App_0.2.3_amd64.AppImage"},
                "windows-x86_64": {"signature": "win-sig", "url": "https://example.com/App_0.2.3_x64-setup.exe"}
            }
        });

        let release: tauri_plugin_updater::RemoteRelease =
            serde_json::from_value(manifest).expect("latest.json must parse");

        assert_eq!(release.version.to_string(), "0.2.3");
        for target in [
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
            "windows-x86_64",
        ] {
            assert!(release.download_url(target).is_ok(), "no url for {target}");
            assert!(release.signature(target).is_ok(), "no sig for {target}");
        }
    }

    #[test]
    fn test_clean_transcript_strips_markers() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO] "), "");
        assert_eq!(clean_transcript("(music) hello  world\n"), "hello world");
        assert_eq!(
            clean_transcript("  Create a file  named test.txt  "),
            "Create a file named test.txt"
        );
        assert_eq!(clean_transcript("hi (typing) there [noise]"), "hi there");
    }

    #[test]
    fn test_stt_bin_asset_mapping() {
        assert_eq!(
            stt_bin_asset_for("linux", "x86_64").as_deref(),
            Some("whisper-cli-linux-amd64")
        );
        assert_eq!(
            stt_bin_asset_for("linux", "aarch64").as_deref(),
            Some("whisper-cli-linux-arm64")
        );
        assert_eq!(
            stt_bin_asset_for("macos", "aarch64").as_deref(),
            Some("whisper-cli-darwin-arm64")
        );
        assert_eq!(
            stt_bin_asset_for("macos", "x86_64").as_deref(),
            Some("whisper-cli-darwin-amd64")
        );
        assert_eq!(
            stt_bin_asset_for("windows", "x86_64").as_deref(),
            Some("whisper-cli-windows-amd64.exe")
        );
        assert_eq!(stt_bin_asset_for("windows", "aarch64"), None);
        assert_eq!(stt_bin_asset_for("linux", "riscv64"), None);
    }

    #[test]
    fn test_resolve_agent_cwd_falls_back_to_home_at_root() {
        let home = PathBuf::from("/Users/x");
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/")), home.clone()),
            home
        );
        assert_eq!(resolve_agent_cwd(None, home.clone()), home);
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/tmp/work")), home.clone()),
            PathBuf::from("/tmp/work")
        );
    }

    #[test]
    fn test_resolve_agent_cwd_escapes_src_tauri() {
        let home = PathBuf::from("/Users/x");
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/repo/desktop/src-tauri")), home.clone()),
            PathBuf::from("/repo/desktop")
        );
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/repo/src-tauri-ish")), home),
            PathBuf::from("/repo/src-tauri-ish")
        );
    }

    #[test]
    fn test_parse_agent_list_merges_local_and_external() {
        let json = br#"{"external":[{"Name":"code-reviewer","URL":"https://a.example.com","Run":false}],"local":[{"Name":"browser-agent","URL":"http://localhost:8083","OCI":"x","Run":true,"Model":"deepseek/deepseek-v4-flash"}],"total":2}"#;
        let agents = parse_agent_list(json).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "browser-agent");
        assert_eq!(agents[0].url, "http://localhost:8083");
        assert!(agents[0].run);
        assert_eq!(agents[0].model, "deepseek/deepseek-v4-flash");
        assert_eq!(agents[1].name, "code-reviewer");
        assert!(!agents[1].run);
        assert_eq!(agents[1].model, "");
    }

    #[test]
    fn test_parse_agent_list_handles_null_groups() {
        assert!(
            parse_agent_list(br#"{"external":null,"local":null,"total":0}"#)
                .unwrap()
                .is_empty()
        );
    }

    fn parse_yaml(text: &str) -> serde_norway::Value {
        serde_norway::from_str(text).unwrap()
    }

    fn str_field<'a>(val: &'a serde_norway::Value, path: &[&str]) -> Option<&'a str> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_str()
    }

    #[test]
    fn config_from_value_reads_fields_and_expands_tilde() {
        let home = PathBuf::from("/home/tester");
        let val = parse_yaml(
            "storage:\n  type: postgres\n  jsonl:\n    path: ~/conv\n  sqlite:\n    path: /db/conv.db\n  postgres:\n    host: db.example\n    port: 6543\n    database: mydb\n    username: alice\n    password: secret\n    ssl_mode: require\n  redis:\n    host: cache\n    port: 6380\n    password: rpw\n    db: 3\n  d1:\n    account_id: acc\n    database_id: dbid\n    api_token: tok\n    base_url: https://d1.example\ngateway:\n  url: http://gw:9000\ndefault_model: gpt-x\n",
        );
        let cfg = config_from_value(&val, &home);
        assert_eq!(cfg.storage_backend, "postgres");
        assert_eq!(cfg.storage_directory, "/home/tester/conv");
        assert_eq!(cfg.gateway_url, "http://gw:9000");
        assert_eq!(cfg.default_model, "gpt-x");
        assert_eq!(cfg.sqlite_path, "/db/conv.db");
        assert_eq!(cfg.postgres_host, "db.example");
        assert_eq!(cfg.postgres_port, "6543");
        assert_eq!(cfg.postgres_database, "mydb");
        assert_eq!(cfg.postgres_username, "alice");
        assert_eq!(cfg.postgres_password, "secret");
        assert_eq!(cfg.postgres_ssl_mode, "require");
        assert_eq!(cfg.redis_host, "cache");
        assert_eq!(cfg.redis_port, "6380");
        assert_eq!(cfg.redis_password, "rpw");
        assert_eq!(cfg.redis_db, "3");
        assert_eq!(cfg.d1_account_id, "acc");
        assert_eq!(cfg.d1_database_id, "dbid");
        assert_eq!(cfg.d1_api_token, "tok");
        assert_eq!(cfg.d1_base_url, "https://d1.example");
    }

    #[test]
    fn config_from_value_defaults_when_missing() {
        let home = PathBuf::from("/home/tester");
        let cfg = config_from_value(&serde_norway::Value::Mapping(Default::default()), &home);
        assert_eq!(cfg.storage_backend, "jsonl");
        assert_eq!(cfg.storage_directory, "/home/tester/.infer/conversations");
        assert_eq!(cfg.gateway_url, "http://localhost:8080");
        assert_eq!(cfg.default_model, "");
        assert_eq!(cfg.postgres_host, "localhost");
        assert_eq!(cfg.postgres_port, "5432");
        assert_eq!(cfg.postgres_database, "infer_conversations");
        assert_eq!(cfg.postgres_username, "");
        assert_eq!(cfg.postgres_password, "");
        assert_eq!(cfg.postgres_ssl_mode, "prefer");
        assert_eq!(cfg.sqlite_path, ".infer/conversations.db");
        assert_eq!(cfg.redis_host, "localhost");
        assert_eq!(cfg.redis_port, "6379");
        assert_eq!(cfg.redis_db, "0");
        assert_eq!(cfg.d1_base_url, "https://api.cloudflare.com/client/v4");
    }

    #[test]
    fn merge_config_preserves_unrelated_fields_and_skips_default_model() {
        let existing = "gateway:\n  run: true\n  standalone_binary: /x\nother_top: keep\nstorage:\n  backend: jsonl\n  directory: /old\n";
        let mut cfg = default_config();
        cfg.storage_backend = "postgres".into();
        cfg.storage_directory = "/data".into();
        cfg.gateway_url = "http://gw".into();
        cfg.default_model = "ignored".into();
        cfg.postgres_host = "pg.host".into();
        cfg.postgres_port = "6543".into();
        let val = parse_yaml(&merge_config(Some(existing), &cfg).unwrap());
        assert_eq!(str_field(&val, &["other_top"]), Some("keep"));
        assert_eq!(
            val.get("gateway")
                .and_then(|g| g.get("run"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            str_field(&val, &["gateway", "standalone_binary"]),
            Some("/x")
        );
        assert_eq!(str_field(&val, &["storage", "type"]), Some("postgres"));
        assert_eq!(
            val.get("storage")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            str_field(&val, &["storage", "jsonl", "path"]),
            Some("/data")
        );
        assert_eq!(
            str_field(&val, &["storage", "postgres", "host"]),
            Some("pg.host")
        );
        assert_eq!(
            val.get("storage")
                .and_then(|s| s.get("postgres"))
                .and_then(|p| p.get("port"))
                .and_then(|v| v.as_i64()),
            Some(6543)
        );
        assert!(val.get("storage").and_then(|s| s.get("backend")).is_none());
        assert!(
            val.get("storage")
                .and_then(|s| s.get("directory"))
                .is_none()
        );
        assert_eq!(str_field(&val, &["gateway", "url"]), Some("http://gw"));
        assert!(val.get("default_model").is_none());
        assert!(val.get("prompts").is_none());
    }

    #[test]
    fn prompt_env_maps_override_and_extras_to_cli_env_vars() {
        assert!(prompt_env(None, None).is_empty());
        assert!(prompt_env(Some("  "), Some("")).is_empty());
        assert_eq!(
            prompt_env(None, Some("be a pirate")),
            vec![(
                "INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS",
                "be a pirate".to_string()
            )]
        );
        assert_eq!(
            prompt_env(Some("full override"), Some("extras")),
            vec![
                (
                    "INFER_PROMPTS_AGENT_SYSTEM_PROMPT",
                    "full override".to_string()
                ),
                (
                    "INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS",
                    "extras".to_string()
                ),
            ]
        );
    }

    #[test]
    fn compose_extras_appends_cwd_to_instructions() {
        let cwd = Path::new("/Users/edenreich/project");
        assert_eq!(
            compose_extras(None, cwd),
            "Current working directory: /Users/edenreich/project"
        );
        assert_eq!(
            compose_extras(Some("  "), cwd),
            "Current working directory: /Users/edenreich/project"
        );
        assert_eq!(
            compose_extras(Some("be a pirate"), cwd),
            "be a pirate\n\nCurrent working directory: /Users/edenreich/project"
        );
    }

    #[test]
    fn merge_config_writes_every_backend_section() {
        let mut cfg = default_config();
        cfg.storage_backend = "redis".into();
        cfg.sqlite_path = "/db/c.db".into();
        cfg.redis_host = "cache".into();
        cfg.redis_port = "6380".into();
        cfg.redis_db = "3".into();
        cfg.d1_account_id = "acc".into();
        let val = parse_yaml(&merge_config(None, &cfg).unwrap());
        let int_at = |path: &[&str]| -> Option<i64> {
            let mut cur = &val;
            for key in path {
                cur = cur.get(key)?;
            }
            cur.as_i64()
        };
        assert_eq!(str_field(&val, &["storage", "type"]), Some("redis"));
        assert_eq!(
            str_field(&val, &["storage", "sqlite", "path"]),
            Some("/db/c.db")
        );
        assert_eq!(
            str_field(&val, &["storage", "redis", "host"]),
            Some("cache")
        );
        assert_eq!(int_at(&["storage", "redis", "port"]), Some(6380));
        assert_eq!(int_at(&["storage", "redis", "db"]), Some(3));
        assert_eq!(
            str_field(&val, &["storage", "d1", "account_id"]),
            Some("acc")
        );
        assert_eq!(
            str_field(&val, &["storage", "d1", "base_url"]),
            Some("https://api.cloudflare.com/client/v4")
        );
    }

    #[test]
    fn merge_default_model_sets_only_that_key() {
        let existing = "storage:\n  type: jsonl\n";
        let val = parse_yaml(&merge_default_model(Some(existing), "claude-x").unwrap());
        assert_eq!(str_field(&val, &["default_model"]), Some("claude-x"));
        assert_eq!(str_field(&val, &["storage", "type"]), Some("jsonl"));
    }

    #[test]
    fn merge_config_replaces_non_mapping_root() {
        let cfg = default_config();
        let val = parse_yaml(&merge_config(Some("just a scalar"), &cfg).unwrap());
        assert_eq!(str_field(&val, &["storage", "type"]), Some("jsonl"));
    }

    fn pb_len(field: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![(field << 3) as u8 | 2];
        let mut n = payload.len();
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn extract_spans_proto_parses_trace_export() {
        let any_str = pb_len(1, b"infer-cli");
        let mut kv = pb_len(1, b"service.name");
        kv.extend(pb_len(2, &any_str));
        let resource_msg = pb_len(1, &kv);
        let resource = pb_len(1, &resource_msg);

        let mut span = pb_len(1, &[0xab, 0xcd]);
        span.extend(pb_len(2, &[0x01]));
        span.extend(pb_len(5, b"chat"));
        span.extend([6 << 3, 2]);
        span.extend([(7 << 3) | 1]);
        span.extend(5u64.to_le_bytes());
        span.extend([(8 << 3) | 1]);
        span.extend(9u64.to_le_bytes());
        let any_int = [3 << 3, 42];
        let mut attr = pb_len(1, b"tokens");
        attr.extend(pb_len(2, &any_int));
        span.extend(pb_len(9, &attr));
        let status = [3 << 3, 2];
        span.extend(pb_len(15, &status));

        let scope_spans = pb_len(2, &span);
        let mut resource_spans = resource;
        resource_spans.extend(pb_len(2, &scope_spans));
        let body = pb_len(1, &resource_spans);

        let spans = extract_spans_proto(&body);
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(s.trace_id, "abcd");
        assert_eq!(s.span_id, "01");
        assert_eq!(s.name, "chat");
        assert_eq!(s.kind, 2);
        assert_eq!(s.start_time_unix_nano, 5);
        assert_eq!(s.end_time_unix_nano, 9);
        assert_eq!(s.attributes, vec![("tokens".to_string(), "42".to_string())]);
        assert_eq!(s.status_code, 2);
        assert_eq!(s.service_name, "infer-cli");
    }

    #[test]
    fn extract_spans_proto_tolerates_garbage() {
        assert!(extract_spans_proto(&[0xff, 0xff, 0xff]).is_empty());
        assert!(extract_spans_proto(&[]).is_empty());
    }

    #[test]
    fn extract_metrics_proto_parses_sum_and_histogram() {
        let mut sum_point = vec![(3 << 3) | 1];
        sum_point.extend(7u64.to_le_bytes());
        sum_point.extend([(6 << 3) | 1]);
        sum_point.extend(42i64.to_le_bytes());
        let mut attr = pb_len(1, b"gen_ai.token.type");
        attr.extend(pb_len(2, &pb_len(1, b"input")));
        sum_point.extend(pb_len(7, &attr));
        let sum = pb_len(1, &sum_point);
        let mut counter = pb_len(1, b"infer.agent.tool.calls");
        counter.extend(pb_len(3, b"1"));
        counter.extend(pb_len(7, &sum));

        let mut histo_point = vec![(4 << 3) | 1];
        histo_point.extend(3u64.to_le_bytes());
        histo_point.extend([(5 << 3) | 1]);
        histo_point.extend(1500f64.to_bits().to_le_bytes());
        let histo = pb_len(1, &histo_point);
        let mut usage = pb_len(1, b"gen_ai.client.token.usage");
        usage.extend(pb_len(9, &histo));

        let mut scope_metrics = pb_len(2, &counter);
        scope_metrics.extend(pb_len(2, &usage));
        let resource_metrics = pb_len(2, &scope_metrics);
        let body = pb_len(1, &resource_metrics);

        let metrics = extract_metrics_proto(&body);
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "infer.agent.tool.calls");
        assert_eq!(metrics[0].unit, "1");
        assert_eq!(metrics[0].value, 42.0);
        assert_eq!(metrics[0].count, 1);
        assert_eq!(metrics[0].time_unix_nano, 7);
        assert_eq!(
            metrics[0].attributes,
            vec![("gen_ai.token.type".to_string(), "input".to_string())]
        );
        assert_eq!(metrics[1].name, "gen_ai.client.token.usage");
        assert_eq!(metrics[1].value, 1500.0);
        assert_eq!(metrics[1].count, 3);
    }
}
