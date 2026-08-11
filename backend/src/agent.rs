use crate::AppState;
use crate::config::read_config;
use crate::env::{
    agent_cwd, compose_extras, home_dir, infer_bin_path, infer_env, mock_mode, prompt_env,
    uploads_dir,
};
use crate::observability::json_val_i64;
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub(crate) enum AgentEvent {
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
pub(crate) struct ToolCallInfo {
    id: String,
    name: String,
    args: String,
}
/// Folds AG-UI event-stream lines into complete AgentEvents, accumulating
/// message/tool-call triads internally.
pub(crate) struct AgentParser {
    session_id: Option<String>,
    msg_from_user: bool,
    tc_id: String,
    tc_name: String,
    tc_args: String,
}

/// Reads the text delta of a streaming message event, tolerating producers that
/// use `content` instead of `delta`.
pub(crate) fn delta_of(val: &serde_json::Value) -> Option<&str> {
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
pub(crate) async fn send_message(
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
pub(crate) async fn send_approval(
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
pub(crate) async fn cancel_agent(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
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
pub(crate) fn gateway_url() -> String {
    read_config().gateway_url
}

/// Run `infer <args>` to completion and return stdout, erroring on non-zero exit.
/// The subprocess runs on the blocking pool so the async runtime keeps serving
/// other commands while `infer` works.
pub(crate) async fn run_infer(args: &[&str]) -> Result<String, String> {
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
pub(crate) async fn list_conversations() -> Result<String, String> {
    run_infer(&["conversations", "list", "--format", "json"]).await
}

#[tauri::command]
pub(crate) async fn get_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "show", &session_id, "--format", "json"]).await
}

#[tauri::command]
pub(crate) async fn delete_conversation(session_id: String) -> Result<String, String> {
    run_infer(&["conversations", "delete", &session_id]).await
}

#[tauri::command]
pub(crate) async fn list_models() -> Result<Vec<String>, String> {
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
/// Read ~/.infer/history (line-delimited prompt history shared with the CLI).
/// Returns an empty vec when the file does not exist yet.
#[tauri::command]
pub(crate) fn read_history() -> Result<Vec<String>, String> {
    let path = home_dir().join(".infer").join("history");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(content.lines().map(|l| l.to_string()).collect())
}

/// Append a line to ~/.infer/history (creates the file if missing).
#[tauri::command]
pub(crate) fn append_history(line: String) -> Result<(), String> {
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
pub(crate) fn safe_image_source(path: &str, home: &Path) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let ok = p.starts_with(home)
        && !p.components().any(|c| c == Component::ParentDir)
        && path.contains("/.infer/tmp/");
    if !ok {
        return Err(format!("Refusing to save image outside .infer/tmp: {path}"));
    }
    Ok(p.to_path_buf())
}

/// Read ~/.infer/projects.json - mapping of session IDs to project names.
/// Returns an empty object when the file does not exist yet.
#[tauri::command]
pub(crate) fn read_projects() -> Result<String, String> {
    let path = home_dir().join(".infer").join("projects.json");
    if !path.exists() {
        return Ok("{}".into());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Write ~/.infer/projects.json (entire mapping, atomically replaced).
#[tauri::command]
pub(crate) fn write_projects(data: String) -> Result<(), String> {
    let path = home_dir().join(".infer").join("projects.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, &data).map_err(|e| e.to_string())
}

/// Copy a generated image (the absolute path `infer` reported) to the user's
/// Downloads folder. Returns the destination path on success.
#[tauri::command]
pub(crate) fn save_image(path: String) -> Result<String, String> {
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

#[tauri::command]
pub(crate) fn save_upload(data: String, mime: String) -> Result<String, String> {
    // Map MIME type to file extension.
    let ext = match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        _ => return Err(format!("Unsupported file type: {mime}")),
    };

    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid upload data: {e}"))?;

    // Enforce 10 MB max.
    const MAX_BYTES: usize = 10 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return Err(format!("File too large: {} bytes (max 10 MB)", bytes.len()));
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("{nanos:x}.{ext}");
    let dest = uploads_dir().join(&name);
    std::fs::write(&dest, &bytes).map_err(|e| format!("Failed to save upload: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

// --- A2A Agent configuration ---
// A2A agents live in the `infer` CLI's own config (~/.infer/agents.yaml). Rather
// than reimplement the CLI's agent knowledge - it auto-fills oci/run/model/env
// for known agents (browser-agent, documentation-agent, ...) and distinguishes
// local containers from external URL agents - the desktop shells out to
// `infer agents add|remove|list` so the two stay perfectly in sync. Keyed by
// name (the CLI's primary key).

#[derive(Clone, serde::Serialize)]
pub(crate) struct A2aAgent {
    name: String,
    url: String,
    run: bool,
    model: String,
}

// `infer agents list --format json` fields are PascalCase (Go struct names),
// split into `local` (container) and `external` (URL-only) groups.
#[derive(serde::Deserialize)]
pub(crate) struct CliAgent {
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
pub(crate) struct CliAgentList {
    local: Option<Vec<CliAgent>>,
    external: Option<Vec<CliAgent>>,
}

pub(crate) fn parse_agent_list(json: &[u8]) -> Result<Vec<A2aAgent>, String> {
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

pub(crate) async fn run_infer_agents(args: &[&str]) -> Result<std::process::Output, String> {
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

pub(crate) fn agents_command_ok(out: std::process::Output) -> Result<(), String> {
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command]
pub(crate) async fn list_a2a_agents() -> Result<Vec<A2aAgent>, String> {
    let out = run_infer_agents(&["list", "--format", "json"]).await?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_agent_list(&out.stdout)
}

#[tauri::command]
pub(crate) async fn add_a2a_agent(name: String, url: String) -> Result<(), String> {
    let mut args = vec!["add", name.as_str()];
    let url = url.trim();
    if !url.is_empty() {
        args.push(url);
    }
    agents_command_ok(run_infer_agents(&args).await?)
}

#[tauri::command]
pub(crate) async fn remove_a2a_agent(name: String) -> Result<(), String> {
    agents_command_ok(run_infer_agents(&["remove", name.as_str()]).await?)
}

#[tauri::command]
pub(crate) async fn set_a2a_agent_model(name: String, model: String) -> Result<(), String> {
    agents_command_ok(
        run_infer_agents(&["update", name.as_str(), "--model", model.as_str()]).await?,
    )
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
}
