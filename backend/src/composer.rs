use crate::env::mock_mode;
use crate::thread::exchange;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

/// Answering can start a worker in the project first.
const MENU_TIMEOUT: Duration = Duration::from_secs(15);
const MOCK_FILES: &[&str] = &[
    "README.md",
    "backend/src/main.rs",
    "frontend/App.tsx",
    "~/.infer/config.yaml",
];

fn list_files_frame(project_dir: &Path) -> Value {
    json!({ "type": "list_files", "project_dir": project_dir.to_string_lossy() })
}

/// The paths a `files` answer lists. `None` is a frame of another kind.
fn parse_files(frame: &Value) -> Option<Vec<String>> {
    if frame["type"] != "files" {
        return None;
    }
    Some(serde_json::from_value(frame["files"].clone()).unwrap_or_default())
}

/// The files the composer's `@` menu offers, as the project's worker lists
/// them for `@<path>` expansion. A daemon that cannot answer, such as an
/// `infer` from before the frame, offers none.
#[tauri::command]
pub(crate) async fn list_files(app: tauri::AppHandle, project: Option<String>) -> Vec<String> {
    if mock_mode() {
        return MOCK_FILES.iter().map(|file| file.to_string()).collect();
    }
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::ensure_running(&app.state::<crate::AppState>())?;
        let project_dir = crate::agent::thread_dir(project.as_deref());
        exchange(
            &binding,
            list_files_frame(&project_dir),
            MENU_TIMEOUT,
            parse_files,
        )
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_request_names_the_project_dir() {
        assert_eq!(
            list_files_frame(Path::new("/proj")),
            json!({ "type": "list_files", "project_dir": "/proj" })
        );
    }

    #[test]
    fn a_files_answer_lists_the_paths() {
        let listed = json!({ "type": "files", "files": ["README.md", "~/.infer/config.yaml"] });
        assert_eq!(
            parse_files(&listed),
            Some(vec!["README.md".into(), "~/.infer/config.yaml".into()])
        );
        assert_eq!(parse_files(&json!({ "type": "files" })), Some(vec![]));
        assert_eq!(
            parse_files(&json!({ "type": "issues", "issues": [] })),
            None
        );
    }
}
