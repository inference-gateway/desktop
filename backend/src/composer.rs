use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

/// Answering can start a worker in the project first.
const MENU_TIMEOUT: Duration = Duration::from_secs(15);

/// An open GitHub issue of the project, as the worker lists it for `#N` expansion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Issue {
    number: u64,
    title: String,
    #[serde(default)]
    state: String,
}

fn project_frame(kind: &str, project_dir: &Path) -> Value {
    json!({ "type": kind, "project_dir": project_dir.to_string_lossy() })
}

/// The entries a `kind` answer lists under its own name, as the `files` and
/// `issues` answers do. `None` is a frame of another kind.
fn parse_list<T: DeserializeOwned>(frame: &Value, kind: &str) -> Option<Vec<T>> {
    if frame["type"] != kind {
        return None;
    }
    Some(serde_json::from_value(frame[kind].clone()).unwrap_or_default())
}

/// Sends a project-scoped `request` for the chat's project dir and answers the
/// list its `reply` carries. A daemon that cannot answer, such as an `infer`
/// from before the frame, answers an empty list.
async fn ask_project<T: DeserializeOwned + Send + 'static>(
    app: tauri::AppHandle,
    project: Option<String>,
    request: &'static str,
    reply: &'static str,
) -> Vec<T> {
    tokio::task::spawn_blocking(move || {
        let project_dir = crate::agent::thread_dir(project.as_deref());
        crate::daemon::ask(
            || crate::daemon::ensure_running(&app.state::<crate::AppState>()),
            project_frame(request, &project_dir),
            MENU_TIMEOUT,
            |frame| parse_list(frame, reply),
        )
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}

/// The files the composer's `@` menu offers, as the project's worker lists
/// them for `@<path>` expansion.
#[tauri::command]
pub(crate) async fn list_files(app: tauri::AppHandle, project: Option<String>) -> Vec<String> {
    ask_project(app, project, "list_files", "files").await
}

/// The open issues the composer's `#` menu offers. Empty wherever the worker
/// has no git repo, no `gh` or no auth.
#[tauri::command]
pub(crate) async fn list_issues(app: tauri::AppHandle, project: Option<String>) -> Vec<Issue> {
    ask_project(app, project, "list_issues", "issues").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_daemon_lists_files_and_open_issues() {
        let files = crate::daemon_mock::answer(&project_frame("list_files", Path::new("/p")))
            .and_then(|frame| parse_list::<String>(&frame, "files"))
            .unwrap_or_default();
        assert_eq!(files.first().map(String::as_str), Some("README.md"));

        let issues = crate::daemon_mock::answer(&project_frame("list_issues", Path::new("/p")))
            .and_then(|frame| parse_list::<Issue>(&frame, "issues"))
            .unwrap_or_default();
        assert_eq!(
            issues.first(),
            Some(&Issue {
                number: 57,
                title: "Fix the flaky test".into(),
                state: "OPEN".into(),
            })
        );
    }

    #[test]
    fn a_request_names_the_project_dir() {
        assert_eq!(
            project_frame("list_files", Path::new("/proj")),
            json!({ "type": "list_files", "project_dir": "/proj" })
        );
    }

    #[test]
    fn a_files_answer_lists_the_paths() {
        let listed = json!({ "type": "files", "files": ["README.md", "~/.infer/config.yaml"] });
        assert_eq!(
            parse_list::<String>(&listed, "files"),
            Some(vec!["README.md".into(), "~/.infer/config.yaml".into()])
        );
        assert_eq!(
            parse_list::<String>(&json!({ "type": "files" }), "files"),
            Some(vec![])
        );
        assert_eq!(
            parse_list::<String>(&json!({ "type": "issues", "issues": [] }), "files"),
            None
        );
    }

    #[test]
    fn an_issues_answer_lists_number_title_and_state() {
        let listed = json!({ "type": "issues", "issues": [
            { "number": 57, "title": "Fix the flaky test", "state": "OPEN" }
        ]});
        assert_eq!(
            parse_list::<Issue>(&listed, "issues"),
            Some(vec![Issue {
                number: 57,
                title: "Fix the flaky test".into(),
                state: "OPEN".into(),
            }])
        );
        assert_eq!(
            parse_list::<Issue>(&json!({ "type": "issues", "issues": [] }), "issues"),
            Some(vec![])
        );
        assert_eq!(
            parse_list::<Issue>(&json!({ "type": "files", "files": [] }), "issues"),
            None
        );
    }
}
