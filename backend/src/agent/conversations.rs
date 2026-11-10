use super::infer::{run_infer, run_infer_in};
use crate::AppState;
use crate::config::read_config;
use crate::env::{agent_cwd, infer_dir};
use crate::thread::Thread;
use std::path::{Path, PathBuf};

/// Keeps only the conversations stored under one of `owned` directories (the
/// desktop's agent cwd and its project dirs). `infer conversations list
/// --all-projects` also returns chats other CLI sessions created elsewhere,
/// which the desktop can neither open nor delete from its own directories.
/// Entries without a `project` field (older CLIs) are kept.
pub(crate) fn owned_conversations(json: &str, owned: &[PathBuf]) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    let owned: Vec<PathBuf> = owned
        .iter()
        .map(|d| d.canonicalize().unwrap_or_else(|_| d.clone()))
        .collect();
    if let Some(list) = value
        .get_mut("conversations")
        .and_then(|v| v.as_array_mut())
    {
        list.retain(|c| match c.get("project").and_then(|p| p.as_str()) {
            None | Some("") => true,
            Some(p) => {
                let p = Path::new(p);
                let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                owned.contains(&p)
            }
        });
    }
    value.to_string()
}

#[tauri::command]
pub(crate) async fn list_conversations() -> Result<String, String> {
    let json = run_infer(&[
        "conversations",
        "list",
        "--all-projects",
        "--format",
        "json",
    ])
    .await?;
    let mut owned = crate::projects::project_dirs();
    owned.push(agent_cwd());
    Ok(owned_conversations(&json, &owned))
}

#[tauri::command]
pub(crate) async fn delete_conversation(
    session_id: String,
    cwd: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    state.threads.remove(&session_id);
    if let Ok(mut channels) = state.channels.lock() {
        channels.remove(&session_id);
    }
    stop_worker(&session_id, cwd.as_deref()).await;
    match run_infer_in(cwd, &["conversations", "delete", &session_id]).await {
        Err(e) if e.contains("conversation not found") => Ok(String::new()),
        other => other,
    }
}

/// Stops a deleted chat's worker on a running daemon. Best effort: no daemon
/// means no worker to stop, and a refused frame is not worth failing the delete.
async fn stop_worker(session_id: &str, cwd: Option<&str>) {
    let project_dir = cwd
        .map(std::path::PathBuf::from)
        .unwrap_or_else(agent_cwd)
        .to_string_lossy()
        .into_owned();
    let session_id = session_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding()?;
        if !crate::daemon::reachable(binding.port) {
            return Ok(());
        }
        Thread::stop(&binding, &session_id, &project_dir)
    })
    .await
    .map(|result: Result<(), String>| {
        if let Err(e) = result {
            eprintln!("stopping a deleted chat's worker: {e}");
        }
    });
}

/// The CLI's per-cwd conversation store: ~/.infer/projects/<slug>/conversations,
/// where the slug is the absolute cwd with path separators replaced by '-'.
pub(crate) fn conversation_store_dir(cwd: &Path) -> PathBuf {
    let slug = cwd
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "-");
    infer_dir()
        .join("projects")
        .join(slug)
        .join("conversations")
}

/// Moves a conversation's JSONL file between per-cwd stores so a chat dragged
/// to another project resumes with its history instead of forking. A no-op
/// when an explicit flat storage directory is configured, when the source
/// file was never persisted, or when source and destination match.
#[tauri::command]
pub(crate) async fn move_conversation(
    session_id: String,
    from_cwd: Option<String>,
    to_project: Option<String>,
) -> Result<(), String> {
    if session_id.contains(['/', '\\']) || session_id.contains("..") {
        return Err("invalid session id".into());
    }
    if !read_config().storage_directory.is_empty() {
        return Ok(());
    }
    let from = from_cwd.map(PathBuf::from).unwrap_or_else(agent_cwd);
    let to = to_project
        .as_deref()
        .and_then(crate::projects::project_dir)
        .unwrap_or_else(agent_cwd);
    if from == to {
        return Ok(());
    }
    let file = format!("{session_id}.jsonl");
    let src = conversation_store_dir(&from).join(&file);
    if !src.exists() {
        return Ok(());
    }
    let dst_dir = conversation_store_dir(&to);
    std::fs::create_dir_all(&dst_dir)
        .map_err(|e| format!("creating {}: {e}", dst_dir.display()))?;
    std::fs::rename(&src, dst_dir.join(&file)).map_err(|e| format!("moving conversation: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_conversations_drops_chats_from_foreign_directories() {
        let tmp = std::env::temp_dir().join("desktop-owned-conv-test");
        std::fs::create_dir_all(&tmp).unwrap();
        let json = serde_json::json!({
            "conversations": [
                {"id": "a", "project": tmp.display().to_string()},
                {"id": "b", "project": "/nowhere/else"},
                {"id": "c"},
                {"id": "d", "project": ""},
            ],
            "total": 4,
        })
        .to_string();
        let out = owned_conversations(&json, std::slice::from_ref(&tmp));
        let ids: Vec<String> =
            serde_json::from_str::<serde_json::Value>(&out).unwrap()["conversations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["id"].as_str().unwrap().to_string())
                .collect();
        assert_eq!(ids, vec!["a", "c", "d"]);
        assert_eq!(owned_conversations("not json", &[]), "not json");
    }

    #[cfg(unix)]
    #[test]
    fn conversation_store_dir_matches_the_cli_slug_scheme() {
        let dir = conversation_store_dir(Path::new("/home/alice/repo"));
        assert!(
            dir.ends_with("projects/-home-alice-repo/conversations"),
            "{}",
            dir.display()
        );
    }
}
