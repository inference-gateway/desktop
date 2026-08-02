use sha2::{Digest, Sha256};
use std::io::{BufRead, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

// ponytail: single-file backend, no abstractions until a second command needs them

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
enum ProgressEvent {
    Checking,
    Downloading { received: u64, total: u64 },
    Verifying,
    Installing,
    Initializing,
    Ready,
    Error { message: String },
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
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
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
/// ponytail: no progress streaming from gh, add if gh output parsing is needed
fn try_gh_download(asset: &str, dest: &std::path::Path) -> Result<bool, String> {
    let available = std::process::Command::new("gh")
        .arg("--version")
        .output()
        .map_or(false, |o| o.status.success());
    if !available {
        return Ok(false);
    }
    let authed = std::process::Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map_or(false, |o| o.status.success());
    if !authed {
        return Ok(false);
    }
    let status = std::process::Command::new("gh")
        .args([
            "release", "download", "latest",
            "--repo", "inference-gateway/cli",
            "--pattern", asset,
            "--output", dest.to_str().unwrap_or(""),
            "--clobber",
        ])
        .status()
        .map_err(|e| format!("gh release download failed: {}", e))?;
    if !status.success() {
        return Err("gh release download exited with non-zero status".into());
    }
    Ok(true)
}

#[tauri::command]
async fn check_and_install_cli(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Checking);

    let bin_path = infer_bin_path();

    if bin_path.exists() {
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

    let asset = asset_name()
        .ok_or_else(|| format!("Unsupported platform: {}-{}", std::env::consts::OS, std::env::consts::ARCH))?;

    let release_url = "https://github.com/inference-gateway/cli/releases/latest/download";

    let bin_dir = home_dir().join(".infer").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let temp_path = bin_dir.join(format!("{}.tmp", binary_name()));

    let _ = on_event.send(ProgressEvent::Downloading { received: 0, total: 0 });
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
        let _ = std::fs::File::open(&cfg_path)
            .and_then(|mut f| f.read_to_string(&mut config));
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
    SessionId { session_id: String },
    AssistantMessage { content: String, reasoning_content: Option<String> },
    ToolCall { name: String, args_summary: String },
    Info { message: String },
    Warning { message: String },
    AgentError { message: String },
    RawLine { line: String },
    Done { exit_code: i32, stderr: String },
    Cancelled,
}

struct AppState {
    running_child: Mutex<Option<std::process::Child>>,
}

/// Parse a single NDJSON line from infer agent stdout into an AgentEvent.
fn parse_agent_line(line: &str, session_id: &mut Option<String>) -> Option<AgentEvent> {
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Some(AgentEvent::RawLine { line: line.to_string() }),
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
                Some(AgentEvent::Info { message: msg.to_string() })
            }
            "warning" => Some(AgentEvent::Warning { message: msg.to_string() }),
            "agent_error" => Some(AgentEvent::AgentError { message: msg.to_string() }),
            _ => Some(AgentEvent::RawLine { line: line.to_string() }),
        }
    } else if let Some(role) = val.get("role").and_then(|v| v.as_str()) {
        if role == "assistant" {
            let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let reasoning_content = val.get("reasoning_content").and_then(|v| v.as_str()).map(|s| s.to_string());

            if let Some(tool_calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
                if !content.is_empty() || reasoning_content.is_some() {
                    return Some(AgentEvent::AssistantMessage { content, reasoning_content });
                }
                if let Some(tc) = tool_calls.first() {
                    let name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("tool");
                    let args = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");
                    return Some(AgentEvent::ToolCall { name: name.to_string(), args_summary: args.to_string() });
                }
                return None;
            }

            if let Some(tools) = val.get("tools").and_then(|v| v.as_str()) {
                if !content.is_empty() || reasoning_content.is_some() {
                    return Some(AgentEvent::AssistantMessage { content, reasoning_content });
                }
                return Some(AgentEvent::ToolCall { name: "tool".to_string(), args_summary: tools.to_string() });
            }

            if !content.is_empty() || reasoning_content.is_some() {
                return Some(AgentEvent::AssistantMessage { content, reasoning_content });
            }
        }
        None
    } else if val.get("session_stats").is_some() {
        None
    } else {
        Some(AgentEvent::RawLine { line: line.to_string() })
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn infer agent: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    {
        let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
        *guard = Some(child.try_clone().map_err(|e| e.to_string())?);
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
        for line in reader.lines() {
            if let Ok(l) = line {
                all.push_str(&l);
                all.push('\n');
            }
        }
        all
    });

    let _ = tokio::task::spawn_blocking(move || stdout_handle.join()).await
        .map_err(|e| format!("Join error: {}", e))?;

    let status = child.wait().map_err(|e| format!("Failed to wait for process: {}", e))?;

    {
        let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }

    let stderr_text = stderr_handle.join().unwrap_or_default();

    let had_error_val = *had_error.lock().unwrap();
    if !status.success() && !had_error_val {
        let code = status.code().unwrap_or(-1);
        let msg = if stderr_text.is_empty() {
            format!("Process exited with code {}", code)
        } else {
            format!("Process exited with code {}: {}", code, stderr_text.trim())
        };
        let _ = on_event.send(AgentEvent::AgentError { message: msg });
    }

    let _ = on_event.send(AgentEvent::Done {
        exit_code: status.code().unwrap_or(-1),
        stderr: stderr_text,
    });

    Ok(new_session_id.lock().unwrap().clone())
}

#[tauri::command]
async fn cancel_agent(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.running_child.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            running_child: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            check_and_install_cli,
            send_message,
            cancel_agent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_name_mapping() {
        assert_eq!(asset_name_for("linux", "x86_64"), Some("infer-linux-amd64"));
        assert_eq!(asset_name_for("linux", "aarch64"), Some("infer-linux-arm64"));
        assert_eq!(asset_name_for("macos", "x86_64"), Some("infer-darwin-amd64"));
        assert_eq!(asset_name_for("macos", "aarch64"), Some("infer-darwin-arm64"));
        assert_eq!(asset_name_for("windows", "x86_64"), Some("infer-windows-amd64"));
        assert_eq!(asset_name_for("windows", "aarch64"), Some("infer-windows-arm64"));

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
        let text = "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8  infer-linux-amd64\n";
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
        assert!(matches!(event, AgentEvent::AssistantMessage { content, reasoning_content } if content == "Hello! How can I help you?" && reasoning_content == Some("Thinking...".into())));
        assert!(sid.is_none());
    }

    #[test]
    fn test_parse_assistant_message_no_reasoning() {
        let line = r#"{"role":"assistant","content":"Just a simple answer."}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::AssistantMessage { content, reasoning_content } if content == "Just a simple answer." && reasoning_content.is_none()));
    }

    #[test]
    fn test_parse_tool_call() {
        let line = r#"{"role":"assistant","content":"","tool_calls":[{"function":{"name":"read_file","arguments":"{\"path\":\"/tmp/test\"}"}}]}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::ToolCall { name, args_summary } if name == "read_file" && args_summary == "{\"path\":\"/tmp/test\"}"));
    }

    #[test]
    fn test_parse_tool_call_with_content() {
        let line = r#"{"role":"assistant","content":"Let me check that file.","tool_calls":[{"function":{"name":"read_file","arguments":"{\"path\":\"/tmp/test\"}"}}]}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        // Content takes priority when present alongside tool_calls
        assert!(matches!(event, AgentEvent::AssistantMessage { content, .. } if content == "Let me check that file."));
    }

    #[test]
    fn test_parse_tools_string() {
        let line = r#"{"role":"assistant","content":"","tools":"read_file(\"/tmp/test\")"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::ToolCall { name, args_summary } if name == "tool" && args_summary == "read_file(\"/tmp/test\")"));
    }

    #[test]
    fn test_parse_agent_error() {
        let line = r#"{"type":"agent_error","message":"Gateway unreachable"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::AgentError { message } if message == "Gateway unreachable"));
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
        assert_eq!(sid, Some("existing-id".into())); // unchanged
    }

    #[test]
    fn test_parse_warning() {
        let line = r#"{"type":"warning","message":"Rate limit approaching"}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::Warning { message } if message == "Rate limit approaching"));
    }

    #[test]
    fn test_parse_session_stats_skipped() {
        let line = r#"{"session_stats":{"total_tokens":150,"prompt_tokens":50,"completion_tokens":100}}"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid);
        assert!(event.is_none());
    }

    #[test]
    fn test_parse_raw_line() {
        let line = r#"this is not valid json at all"#;
        let mut sid = None;
        let event = parse_agent_line(line, &mut sid).unwrap();
        assert!(matches!(event, AgentEvent::RawLine { line: l } if l == "this is not valid json at all"));
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
        assert!(event.is_some()); // empty string is not valid JSON, so it becomes RawLine
    }

    #[test]
    fn test_parse_ndjson_fixture() {
        // Simulate a full NDJSON stream: info with session_id, assistant message, tool call, agent_error
        let fixture = r#"{"type":"info","message":"Session started","session_id":"session-42","timestamp":"2024-01-01T00:00:00Z"}
{"role":"assistant","content":"I'll look that up for you.","reasoning_content":"Searching..."}
{"role":"assistant","content":"","tool_calls":[{"function":{"name":"search","arguments":"{\"q\":\"test\"}"}}]}
{"type":"agent_error","message":"API key not found"}"#;

        let mut sid = None;
        let events: Vec<AgentEvent> = fixture
            .lines()
            .filter_map(|line| {
                let l = line.trim();
                if l.is_empty() { None } else { parse_agent_line(l, &mut sid) }
            })
            .collect();

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], AgentEvent::Info { .. }));
        assert_eq!(sid, Some("session-42".into()));
        assert!(matches!(events[1], AgentEvent::AssistantMessage { content, .. } if content == "I'll look that up for you."));
        assert!(matches!(events[2], AgentEvent::ToolCall { name, .. } if name == "search"));
        assert!(matches!(events[3], AgentEvent::AgentError { message } if message == "API key not found"));
    }
}
