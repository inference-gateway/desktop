use sha2::{Digest, Sha256};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

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

fn infer_bin_path() -> PathBuf {
    home_dir().join(".infer").join("bin").join(binary_name())
}

fn config_path() -> PathBuf {
    home_dir().join(".infer").join("config.yaml")
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
    Warning {
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
    #[allow(dead_code)]
    Cancelled,
}

#[derive(Clone, serde::Serialize)]
struct ToolCallInfo {
    id: String,
    name: String,
    args: String,
}

struct AppState {
    running_child: Mutex<Option<std::process::Child>>,
    child_stdin: Mutex<Option<std::process::ChildStdin>>,
    gateway_child: Mutex<Option<std::process::Child>>,
}

/// Parse a single NDJSON line from infer agent stdout into an AgentEvent.
fn parse_agent_line(line: &str, session_id: &mut Option<String>) -> Option<AgentEvent> {
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            return Some(AgentEvent::RawLine {
                line: line.to_string(),
            })
        }
    };

    if let Some(typ) = val.get("type").and_then(|v| v.as_str()) {
        let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
        match typ {
            "info" => {
                if let Some(sid) = val.get("session_id").and_then(|v| v.as_str()) {
                    if !sid.is_empty() && session_id.is_none() {
                        *session_id = Some(sid.to_string());
                    }
                }
                Some(AgentEvent::Info {
                    message: msg.to_string(),
                })
            }
            "warning" => Some(AgentEvent::Warning {
                message: msg.to_string(),
            }),
            "approval_request" => {
                let tool_name = val
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_args = val
                    .get("tool_args")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = val
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(AgentEvent::ApprovalRequest {
                    tool_name,
                    tool_args,
                    tool_call_id,
                })
            }
            "agent_error" => Some(AgentEvent::AgentError {
                message: msg.to_string(),
            }),
            _ => Some(AgentEvent::RawLine {
                line: line.to_string(),
            }),
        }
    } else if let Some(role) = val.get("role").and_then(|v| v.as_str()) {
        if role == "assistant" {
            let content = val
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let reasoning_content = val
                .get("reasoning_content")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let tool_calls: Vec<ToolCallInfo> = val
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .map(|tc| ToolCallInfo {
                            id: tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            name: tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string(),
                            args: tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}")
                                .to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();

            if content.is_empty() && reasoning_content.is_none() && tool_calls.is_empty() {
                return None;
            }
            return Some(AgentEvent::AssistantMessage {
                content,
                reasoning_content,
                tool_calls,
            });
        }
        if role == "tool" {
            let content = val
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if content.is_empty() {
                return None;
            }
            let tool_call_id = val
                .get("tool_call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            return Some(AgentEvent::ToolResult {
                content,
                tool_call_id,
            });
        }
        None
    } else if val.get("session_stats").is_some() {
        None
    } else {
        Some(AgentEvent::RawLine {
            line: line.to_string(),
        })
    }
}

#[tauri::command]
async fn send_message(
    prompt: String,
    model: String,
    session_id: Option<String>,
    on_event: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    let bin_path = infer_bin_path();

    let mut child = std::process::Command::new(&bin_path)
        .arg("agent")
        .arg("--session-id")
        .arg(session_id.as_deref().unwrap_or(""))
        .arg("-m")
        .arg(&model)
        .arg(&prompt)
        .envs(auth_env())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn infer agent: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let child_stdin = child.stdin.take().unwrap();

    {
        let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
        *guard = Some(child);
    }
    {
        let mut guard = state.child_stdin.lock().map_err(|e| e.to_string())?;
        *guard = Some(child_stdin);
    }

    let on_event_clone = on_event.clone();
    let new_session_id = Arc::new(Mutex::new(session_id.clone()));
    let had_error = Arc::new(Mutex::new(false));

    let new_session_id_clone = Arc::clone(&new_session_id);
    let had_error_clone = Arc::clone(&had_error);

    let stdout_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let mut sid = new_session_id_clone.lock().unwrap();
            if let Some(event) = parse_agent_line(&line, &mut sid) {
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

    let status = {
        let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
        match guard.take() {
            Some(mut child) => Some(
                child
                    .wait()
                    .map_err(|e| format!("Failed to wait for process: {}", e))?,
            ),
            None => None,
        }
    };
    {
        let mut guard = state.child_stdin.lock().map_err(|e| e.to_string())?;
        *guard = None;
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

    let new_session = new_session_id.lock().unwrap().clone();
    Ok(new_session)
}

#[tauri::command]
async fn send_approval(
    tool_call_id: String,
    approved: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut guard = state.child_stdin.lock().map_err(|e| e.to_string())?;
    let stdin = guard.as_mut().ok_or("No running agent")?;
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
async fn cancel_agent(state: tauri::State<'_, AppState>) -> Result<(), String> {
    // Drop stdin first to unblock the child
    {
        let mut guard = state.child_stdin.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }
    let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

/// Resolve the local gateway URL from infer's config, defaulting to localhost:8080.
fn gateway_url() -> String {
    let default = "http://localhost:8080".to_string();
    let cfg = match std::fs::read_to_string(config_path()) {
        Ok(c) => c,
        Err(_) => return default,
    };
    let mut in_gateway = false;
    for line in cfg.lines() {
        if !line.starts_with([' ', '\t']) {
            in_gateway = line.trim_start().starts_with("gateway:");
            continue;
        }
        if in_gateway {
            if let Some(rest) = line.trim().strip_prefix("url:") {
                let v = rest.trim().trim_matches(['"', '\'']);
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }
    default
}

/// Run `infer <args>` to completion and return stdout, erroring on non-zero exit.
fn run_infer(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(infer_bin_path())
        .args(args)
        .env("HOME", home_dir().to_str().unwrap_or(""))
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
}

#[tauri::command]
async fn list_conversations() -> Result<String, String> {
    run_infer(&["conversations", "list", "--format", "json"])
}

#[tauri::command]
async fn get_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "show", &session_id, "--format", "json"])
}

#[tauri::command]
async fn delete_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "delete", &session_id])
}

#[tauri::command]
async fn list_models() -> Result<Vec<String>, String> {
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
/// update lands on the next spawn.
#[tauri::command]
async fn start_gateway(state: tauri::State<'_, AppState>, force: bool) -> Result<(), String> {
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
    if let Ok(out) = gh {
        if out.status.success() {
            if let Some(tag) = parse_version(&String::from_utf8_lossy(&out.stdout)) {
                return Some(tag);
            }
        }
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
async fn check_updates() -> Result<Vec<UpdateInfo>, String> {
    tokio::task::spawn_blocking(|| {
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
    .map_err(|e| e.to_string())
}

pub fn run() {
    use tauri::Manager;

    tauri::Builder::default()
        .manage(AppState {
            running_child: Mutex::new(None),
            child_stdin: Mutex::new(None),
            gateway_child: Mutex::new(None),
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
            start_gateway,
            check_updates,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Ok(mut guard) = app_handle.state::<AppState>().gateway_child.lock() {
                    if let Some(mut child) = guard.take() {
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

    // --- NDJSON parser tests ---

    #[test]
    fn test_parse_assistant_message() {
        let line = r#"{"role":"assistant","content":"Hello! How can I help you?","reasoning_content":"Thinking..."}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::AssistantMessage { content, reasoning_content, .. } if content == "Hello! How can I help you?" && reasoning_content == Some("Thinking...".into()))
        );
        assert!(sid.is_none());
    }

    #[test]
    fn test_parse_assistant_message_no_reasoning() {
        let line = r#"{"role":"assistant","content":"Just a simple answer."}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::AssistantMessage { content, reasoning_content, .. } if content == "Just a simple answer." && reasoning_content.is_none())
        );
    }

    #[test]
    fn test_parse_tool_call() {
        let line = r#"{"role":"assistant","content":"","tool_calls":[{"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":\"/tmp/test\"}"}}]}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        match event {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-1");
                assert_eq!(tool_calls[0].name, "read_file");
                assert_eq!(tool_calls[0].args, "{\"path\":\"/tmp/test\"}");
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_parse_tool_call_with_content() {
        let line = r#"{"role":"assistant","content":"Let me check that file.","reasoning_content":"","tool_calls":[{"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":\"/tmp/test\"}"}},{"id":"call-2","function":{"name":"list_dir","arguments":"{\"path\":\"/tmp\"}"}}]}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        match event {
            AgentEvent::AssistantMessage {
                content,
                reasoning_content,
                tool_calls,
            } => {
                assert_eq!(content, "Let me check that file.");
                assert!(reasoning_content.is_none());
                assert_eq!(tool_calls.len(), 2);
                assert_eq!(tool_calls[0].name, "read_file");
                assert_eq!(tool_calls[1].id, "call-2");
                assert_eq!(tool_calls[1].name, "list_dir");
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_parse_tool_result() {
        let line = r#"{"role":"tool","content":"Result of tool call: {\"tool_name\":\"read_file\",\"success\":true}","tool_call_id":"call-1"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::ToolResult { content, tool_call_id } if content.starts_with("Result of tool call:") && tool_call_id == "call-1")
        );
    }

    #[test]
    fn test_parse_agent_error() {
        let line = r#"{"type":"agent_error","message":"Gateway unreachable"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::AgentError { message } if message == "Gateway unreachable")
        );
    }

    #[test]
    fn test_parse_info_with_session_id() {
        let line = r#"{"type":"info","message":"Session started","session_id":"abc-123","timestamp":"2024-01-01T00:00:00Z"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::Info { message } if message == "Session started"));
        assert_eq!(sid, Some("abc-123".into()));
    }

    #[test]
    fn test_parse_info_without_session_id() {
        let line = r#"{"type":"info","message":"Model loaded"}"#;
        let mut sid = Some("existing-id".into());
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::Info { message } if message == "Model loaded"));
        assert_eq!(sid, Some("existing-id".into()));
    }

    #[test]
    fn test_parse_warning() {
        let line = r#"{"type":"warning","message":"Rate limit approaching"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::Warning { message } if message == "Rate limit approaching")
        );
    }

    #[test]
    fn test_parse_session_stats_skipped() {
        let line =
            r#"{"session_stats":{"total_tokens":150,"prompt_tokens":50,"completion_tokens":100}}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid);
        assert!(event.is_none());
    }

    #[test]
    fn test_parse_raw_line() {
        let line = r#"this is not valid json at all"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::RawLine { line: l } if l == "this is not valid json at all")
        );
    }

    #[test]
    fn test_parse_unknown_type_is_raw() {
        let line = r#"{"type":"unknown","data":"something"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_empty_line_returns_none() {
        let line = "";
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid);
        assert!(event.is_some());
    }

    #[test]
    fn test_parse_ndjson_fixture() {
        let fixture = r#"{"type":"info","message":"Session started","session_id":"session-42","timestamp":"2024-01-01T00:00:00Z"}
{"role":"assistant","content":"I'll look that up for you.","reasoning_content":"Searching...","tool_calls":[{"id":"call-1","function":{"name":"search","arguments":"{\"q\":\"test\"}"}}]}
{"role":"tool","content":"Result of tool call: {\"tool_name\":\"search\",\"success\":true}","tool_call_id":"call-1"}
{"type":"agent_error","message":"API key not found"}"#;

        let mut sid = None;
        let events: Vec<AgentEvent> = fixture
            .lines()
            .filter_map(|line| {
                let l = line.trim();
                if l.is_empty() {
                    None
                } else {
                    parse_agent_line(l, &mut sid)
                }
            })
            .collect();

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], AgentEvent::Info { .. }));
        assert_eq!(sid, Some("session-42".into()));
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, tool_calls, .. } if content == "I'll look that up for you." && tool_calls.len() == 1 && tool_calls[0].name == "search")
        );
        assert!(
            matches!(&events[2], AgentEvent::ToolResult { tool_call_id, .. } if tool_call_id == "call-1")
        );
        assert!(
            matches!(&events[3], AgentEvent::AgentError { message } if message == "API key not found")
        );
    }

    #[test]
    fn test_parse_approval_request() {
        let line = r#"{"type":"approval_request","tool_name":"read_file","tool_args":"{\"path\":\"/tmp/test\"}","tool_call_id":"call-1"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(
            matches!(event, AgentEvent::ApprovalRequest { tool_name, tool_args, tool_call_id }
            if tool_name == "read_file" && tool_args == "{\"path\":\"/tmp/test\"}" && tool_call_id == "call-1")
        );
    }

    #[test]
    fn test_approval_round_trip() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("printf '%s\\n' '{\"type\":\"approval_request\",\"tool_name\":\"test\",\"tool_args\":\"{\\\"key\\\":\\\"val\\\"}\",\"tool_call_id\":\"call-1\"}'; read line; printf '%s\\n' \"$line\"")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let mut child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();

        let reader = std::io::BufReader::new(child_stdout);
        let mut lines = reader.lines();
        let request_line = lines.next().unwrap().unwrap();
        let mut sid = None;
        let event = parse_agent_line(&request_line, &mut sid).unwrap();
        assert!(
            matches!(&event, AgentEvent::ApprovalRequest { tool_name, tool_args, tool_call_id }
            if tool_name == "test" && tool_args == "{\"key\":\"val\"}" && tool_call_id == "call-1")
        );

        let response = serde_json::json!({
            "type": "approval_response",
            "tool_call_id": "call-1",
            "approved": false,
        });
        let response_line = format!("{}\n", serde_json::to_string(&response).unwrap());
        child_stdin.write_all(response_line.as_bytes()).unwrap();
        child_stdin.flush().unwrap();
        drop(child_stdin);

        let echoed = lines.next().unwrap().unwrap();
        let echoed_val: serde_json::Value = serde_json::from_str(&echoed).unwrap();
        assert_eq!(echoed_val["type"], "approval_response");
        assert_eq!(echoed_val["tool_call_id"], "call-1");
        assert_eq!(echoed_val["approved"], false);

        let status = child.wait().unwrap();
        assert!(status.success());
    }
}
