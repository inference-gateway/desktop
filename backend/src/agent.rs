pub(crate) mod a2a;
pub(crate) mod conversations;
pub(crate) mod events;
pub(crate) mod files;
pub(crate) mod history;
pub(crate) mod infer;
pub(crate) mod models;

use crate::AppState;
use crate::env::{agent_cwd, compose_extras};
use crate::thread::{Sink, Thread, ThreadOptions};
use events::AgentRecording;
pub(crate) use events::{AgentEvent, BackgroundJob};
use files::attached_image_paths;
pub(crate) use infer::{gateway_url, infer_command, run_infer, run_infer_in};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

/// A `!cmd` prompt runs straight through the CLI's direct-exec path and never
/// touches agents, so it need not wait for shared services to start.
fn is_bash_command(prompt: &str) -> bool {
    prompt.starts_with('!') && !prompt.starts_with("!!")
}

/// A one-sentence conventional-commit subject for an agent run's checkpoint,
/// built from the prompt's first non-empty line (capped) so a content project's
/// git log reads as what was asked rather than a bare "agent run".
fn agent_commit_message(prompt: &str) -> String {
    let summary = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if summary.is_empty() {
        return "chore(timeline): agent run".to_string();
    }
    let summary = if summary.chars().count() > 68 {
        format!("{}...", summary.chars().take(65).collect::<String>())
    } else {
        summary.to_string()
    };
    format!("chore(timeline): {summary}")
}

/// The daemon thread a chat runs in: its worker's launch options come from
/// the first open, since the daemon applies them once.
fn thread_options(
    model: &str,
    auto_mode: bool,
    system_prompt: Option<&str>,
    extra_instructions: Option<&str>,
    cwd: &Path,
    agent: Option<&str>,
) -> ThreadOptions {
    ThreadOptions {
        model: model.to_string(),
        mode: if auto_mode { "auto" } else { "standard" }.to_string(),
        system_prompt: system_prompt
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned),
        custom_instructions: Some(compose_extras(extra_instructions, cwd)),
        sandbox_directories: crate::projects::sandbox_allowed_dirs()
            .map(|dirs| dirs.split(',').map(str::to_owned).collect())
            .unwrap_or_default(),
        max_turns: 500,
        agent: agent.filter(|a| !a.trim().is_empty()).map(str::to_owned),
    }
}

/// The directory a chat's thread runs in: its project dir, else the app's
/// agent cwd.
pub(crate) fn thread_dir(project: Option<&str>) -> PathBuf {
    project
        .and_then(crate::projects::project_dir)
        .filter(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or_else(agent_cwd)
}

/// Fan-out for one thread's events: the chat's IPC channel streams the
/// ordered transcript to the main webview, a global "agent-event" broadcast
/// reaches every window, and a recording the run starts captures input
/// until it stops.
fn session_sink(app: tauri::AppHandle, session_id: String) -> Sink {
    let recording: Arc<Mutex<Option<AgentRecording>>> = Arc::new(Mutex::new(None));
    let sink: Arc<Mutex<Option<Sink>>> = Arc::new(Mutex::new(None));
    let inner = Arc::clone(&sink);
    let emit: Sink = Arc::new(move |event: AgentEvent| {
        let remote = app
            .state::<AppState>()
            .remote_sessions
            .lock()
            .is_ok_and(|sessions| sessions.contains(&session_id));
        let _ = app.emit(
            "agent-event",
            serde_json::json!({ "sessionId": session_id, "name": "", "remote": remote, "event": event }),
        );
        if let Some(channel) = app
            .state::<AppState>()
            .channels
            .lock()
            .ok()
            .and_then(|c| c.get(&session_id).cloned())
        {
            let _ = channel.send(event.clone());
        }
        let started = match &event {
            AgentEvent::RecordingStarted { path, .. } if !remote => Some(path.clone()),
            _ => None,
        };
        let ends = started.is_some()
            || matches!(
                event,
                AgentEvent::RecordingStopped | AgentEvent::AgentError { .. }
            );
        if ends && let Some(r) = recording.lock().ok().and_then(|mut r| r.take()) {
            r.stop();
        }
        if let (Some(path), Some(sink)) = (started, inner.lock().ok().and_then(|s| s.clone()))
            && let Ok(mut slot) = recording.lock()
        {
            *slot = Some(AgentRecording::start(path, sink));
        }
    });
    *sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&emit));
    emit
}

/// The open thread for a chat, opened on the daemon when the app has none.
async fn ensure_thread(
    app: &tauri::AppHandle,
    state: &AppState,
    session_id: &str,
    project_dir: &Path,
    options: ThreadOptions,
) -> Result<Arc<Thread>, String> {
    if let Some(thread) = state.threads.get(session_id) {
        return Ok(thread);
    }
    let app = app.clone();
    let session_id = session_id.to_string();
    let project_dir = project_dir.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || {
        crate::agent_mail::resync_allowlist(&options);
        let state = app.state::<AppState>();
        let binding = crate::daemon::ensure_running(&state)?;
        let status_app = app.clone();
        let thread = Thread::open(
            &binding,
            &session_id,
            &project_dir,
            &options,
            session_sink(app.clone(), session_id.clone()),
            Arc::new(move |status| crate::daemon::extension_status(&status_app, status)),
        )?;
        state.threads.insert(&session_id, Arc::clone(&thread));
        Ok(thread)
    })
    .await
    .map_err(|e| format!("thread open task failed: {e}"))?
}

/// Stops following a chat the user left, so its worker idles out in the daemon.
#[tauri::command]
pub(crate) fn close_thread(session_id: String, state: tauri::State<'_, AppState>) {
    state.threads.close_idle(&session_id);
}

fn thread_for(state: &AppState, session_id: &str) -> Result<Arc<Thread>, String> {
    state
        .threads
        .get(session_id)
        .ok_or_else(|| "No running agent".to_string())
}

/// Opens (or reuses) a chat's thread and returns its history as the AG-UI
/// messages of the worker's MESSAGES_SNAPSHOT. `on_event` streams the runs
/// the app did not send, unless the chat already streams on a channel.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn open_thread(
    session_id: String,
    model: String,
    on_event: Channel<AgentEvent>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    system_prompt: Option<String>,
    extra_instructions: Option<String>,
    auto_mode: bool,
    project: Option<String>,
    agent: Option<String>,
) -> Result<String, String> {
    let cwd = thread_dir(project.as_deref());
    let options = thread_options(
        &model,
        auto_mode,
        system_prompt.as_deref(),
        extra_instructions.as_deref(),
        &cwd,
        agent.as_deref(),
    );
    let thread = ensure_thread(&app, &state, &session_id, &cwd, options).await?;
    if let Ok(mut channels) = state.channels.lock() {
        channels.entry(session_id).or_insert(on_event);
    }
    serde_json::to_string(thread.history()).map_err(|e| e.to_string())
}

/// The run_agent_input user message for a prompt: its text, plus one image
/// part per decodable `[Attached image: <path>]` the composer prepended.
fn message_content(prompt: &str) -> serde_json::Value {
    let images: Vec<serde_json::Value> = attached_image_paths(prompt)
        .into_iter()
        .filter_map(|path| {
            use base64::Engine as _;
            let bytes = std::fs::read(&path).ok()?;
            let ext = Path::new(&path).extension()?.to_str()?.to_ascii_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => "image/png",
            };
            Some(serde_json::json!({
                "type": "image",
                "mimeType": mime,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                "filename": Path::new(&path).file_name()?.to_str()?,
            }))
        })
        .collect();
    if images.is_empty() {
        return prompt.into();
    }
    let mut parts = vec![serde_json::json!({ "type": "text", "text": prompt })];
    parts.extend(images);
    parts.into()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_message(
    prompt: String,
    model: String,
    session_id: String,
    on_event: Channel<AgentEvent>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    system_prompt: Option<String>,
    extra_instructions: Option<String>,
    auto_mode: bool,
    project: Option<String>,
    workspace: Option<String>,
    agent: Option<String>,
) -> Result<(), String> {
    if !is_bash_command(&prompt) {
        crate::tools::await_services().await;
    }
    let cwd = thread_dir(project.as_deref());
    // Content projects are local git repos so agent edits are revertable. Take a
    // checkpoint of the pre-run state (best-effort - never block a run on git).
    let content_dir = project
        .as_deref()
        .filter(|name| crate::projects::project_is_content(name))
        .and_then(crate::projects::project_dir);
    if let Some(dir) = &content_dir {
        let _ = crate::projects::ensure_content_repo(dir);
        let _ = crate::projects::checkpoint(dir, "chore(timeline): checkpoint before agent run");
    }
    let options = thread_options(
        &model,
        auto_mode,
        system_prompt.as_deref(),
        extra_instructions.as_deref(),
        &cwd,
        agent.as_deref(),
    );
    let mode = options.mode.clone();
    let is_new_to_the_app = state.threads.get(&session_id).is_none();
    let mut thread = ensure_thread(&app, &state, &session_id, &cwd, options.clone()).await?;
    let target = workspace.filter(|name| !name.trim().is_empty());
    if target.is_some()
        && let Ok(mut sessions) = state.remote_sessions.lock()
    {
        sessions.insert(session_id.clone());
    }
    if let Some(target) = target.filter(|_| is_new_to_the_app && thread.history().is_empty()) {
        teleport_thread(&session_id, &cwd, &target, &on_event).await?;
        state.threads.remove(&session_id);
        thread = ensure_thread(&app, &state, &session_id, &cwd, options).await?;
    }
    if let Ok(mut channels) = state.channels.lock() {
        channels.insert(session_id.clone(), on_event);
    }
    let commit = agent_commit_message(&prompt);
    *thread.on_done.lock().unwrap_or_else(|e| e.into_inner()) = content_dir.map(|dir| {
        Box::new(move || {
            let _ = crate::projects::checkpoint(&dir, &commit);
        }) as Box<dyn Fn() + Send + Sync>
    });
    thread.select(&model, &mode)?;
    thread.run(Some(message_content(&prompt)))
}

/// Places a new chat on its agent's computer before the first run, showing
/// each stage as startup progress. A failure is returned, never worked
/// around, so the agent cannot fall back to driving this machine.
async fn teleport_thread(
    session_id: &str,
    project_dir: &Path,
    target: &str,
    on_event: &Channel<AgentEvent>,
) -> Result<(), String> {
    let report = |state: &str, message: String| {
        let _ = on_event.send(AgentEvent::AgentStatus {
            name: target.to_string(),
            state: state.to_string(),
            message,
            done: 0,
            total: 0,
        });
    };
    report("Teleporting", format!("Starting the computer {target}"));
    let progress = on_event.clone();
    let (session_id, project_dir, name) = (
        session_id.to_string(),
        project_dir.to_path_buf(),
        target.to_string(),
    );
    let outcome = tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding()?;
        crate::workspaces::teleport(&binding, &session_id, &project_dir, &name, |stage| {
            let _ = progress.send(AgentEvent::AgentStatus {
                name: name.clone(),
                state: "Teleporting".into(),
                message: format!("Moving to {name}: {stage}"),
                done: 0,
                total: 0,
            });
        })
    })
    .await
    .map_err(|e| format!("teleport task failed: {e}"))
    .and_then(|outcome| outcome);
    report(
        if outcome.is_ok() { "Ready" } else { "Failed" },
        String::new(),
    );
    outcome
}

/// Moves a chat to another workspace of the daemon, or back to this computer,
/// reporting each stage. Its thread is dropped, so the next prompt opens
/// through the daemon's relay to wherever the session lives now.
#[tauri::command]
pub(crate) async fn move_session(
    session_id: String,
    project: Option<String>,
    target: String,
    remote: bool,
    on_stage: Channel<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let project_dir = thread_dir(project.as_deref());
    let (id, name) = (session_id.clone(), target);
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding()?;
        crate::workspaces::teleport(&binding, &id, &project_dir, &name, |stage| {
            let _ = on_stage.send(stage.to_owned());
        })
    })
    .await
    .map_err(|e| format!("teleport task failed: {e}"))??;
    state.threads.remove(&session_id);
    if let Ok(mut sessions) = state.remote_sessions.lock() {
        if remote {
            sessions.insert(session_id);
        } else {
            sessions.remove(&session_id);
        }
    }
    Ok(())
}

/// Answers a tool-call interrupt: a resume entry on the thread's next run.
/// `resolved` approves the call and `cancelled` declines it.
#[tauri::command]
pub(crate) async fn send_approval(
    session_id: String,
    tool_call_id: String,
    approved: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    thread_for(&state, &session_id)?.resume(serde_json::json!({
        "interruptId": tool_call_id,
        "status": if approved { "resolved" } else { "cancelled" },
    }))
}

/// Pushes a follow-up prompt into a running thread; the worker queues it and
/// drains it as the next turn (used while the run waits on tasks).
#[tauri::command]
pub(crate) async fn send_user_message(
    session_id: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    thread_for(&state, &session_id)?.run(Some(content.into()))
}

/// Answers an AskUserQuestion form: `answers` is the frontend's
/// `UserQuestionAnswer[]`, `None` dismisses the form.
#[tauri::command]
pub(crate) async fn send_question_answers(
    session_id: String,
    tool_call_id: String,
    answers: Option<serde_json::Value>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let entry = match answers {
        Some(answers) => serde_json::json!({
            "interruptId": tool_call_id,
            "status": "resolved",
            "payload": { "answers": answers },
        }),
        None => serde_json::json!({ "interruptId": tool_call_id, "status": "cancelled" }),
    };
    thread_for(&state, &session_id)?.resume(entry)
}

/// Continues a thread after a stopped run with a new, empty run: how a
/// paused computer-use session resumes.
#[tauri::command]
pub(crate) async fn resume_agent(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    thread_for(&state, &session_id)?.run(None)
}

/// Stops the thread's running turn, which ends with outcome `cancelled`.
#[tauri::command]
pub(crate) async fn cancel_agent(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    match state.threads.get(&session_id) {
        Some(thread) => thread.interrupt(),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_single_bang_prompts_are_bash_commands() {
        assert!(is_bash_command("!ls -la"));
        assert!(!is_bash_command("!!Read(file_path=\"x\")"));
        assert!(!is_bash_command("list files"));
    }

    #[test]
    fn agent_commit_message_uses_the_prompt_first_line_capped() {
        assert_eq!(
            agent_commit_message("add captions to this recording"),
            "chore(timeline): add captions to this recording"
        );
        assert_eq!(
            agent_commit_message("\n  write the captions track  \nand nothing else"),
            "chore(timeline): write the captions track"
        );
        assert_eq!(agent_commit_message("   "), "chore(timeline): agent run");
        let long = "x".repeat(200);
        let msg = agent_commit_message(&long);
        assert!(msg.ends_with("..."));
        assert!(msg.len() < 90);
    }
}
