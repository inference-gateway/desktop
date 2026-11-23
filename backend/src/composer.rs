use crate::env::infer_dir;
use notify::{RecursiveMode, Watcher};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

/// Answering can start a worker in the project first.
const MENU_TIMEOUT: Duration = Duration::from_secs(15);

/// The resolved ~/.infer/composer.yaml, as `infer config get composer` prints
/// it: each context's trigger table plus the status bar toggles the CLI moved
/// into this file. The desktop reads the CLI's resolved table as is.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ComposerSettings {
    #[serde(default)]
    pub(crate) triggers: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub(crate) status_bar: ComposerStatusBar,
}

/// The status bar toggles of composer.yaml: the bar's own switch and which
/// indicators it shows. A key the CLI's answer leaves out keeps the CLI's
/// default, and the indicators map only holds the non-default ones.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ComposerStatusBar {
    pub(crate) enabled: bool,
    pub(crate) indicators: BTreeMap<String, bool>,
}

impl Default for ComposerStatusBar {
    fn default() -> Self {
        Self {
            enabled: true,
            indicators: BTreeMap::new(),
        }
    }
}

/// The composer triggers and status bar toggles, resolved the way the CLI runs
/// with them. An error carries the CLI's own wording, so a file it rejects
/// shows the reason and the composer keeps the built-in defaults.
#[tauri::command(async)]
pub(crate) async fn get_composer() -> Result<ComposerSettings, String> {
    let text =
        crate::cli::infer::run_infer(&["config", "get", "composer", "--format", "json"]).await?;
    serde_json::from_str(&text).map_err(|e| format!("composer config is not valid JSON: {e}"))
}

/// The one live watcher over ~/.infer/composer.yaml.
pub(crate) struct ComposerWatcher(pub(crate) std::sync::Mutex<Option<notify::RecommendedWatcher>>);

/// The watcher fires on any event under ~/.infer, because the watched dir holds
/// composer.yaml only after its first write.
const COMPOSER_YAML: &str = "composer.yaml";

/// Whether an event touches composer.yaml, so the composer reloads its table
/// when the file appears, changes or is removed.
fn touches_composer_yaml(event: &notify::Event) -> bool {
    event
        .paths
        .iter()
        .any(|p| p.file_name().is_some_and(|f| f == COMPOSER_YAML))
}

/// Builds the composer.yaml watcher, emitting [`crate::events::COMPOSER_CHANGED`]
/// on each change. The managed state keeps it alive once installed. Watching
/// ~/.infer instead of the file sees its first write too; a dir the watch
/// cannot follow (gone mid-watching) only delays the reload to the next change.
pub(crate) fn watch_composer_file(
    app: tauri::AppHandle,
) -> notify::Result<notify::RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.as_ref().is_ok_and(touches_composer_yaml) {
            let _ = app.emit(crate::events::COMPOSER_CHANGED, ());
        }
    })?;
    let _ = watcher.watch(infer_dir(), RecursiveMode::NonRecursive);
    Ok(watcher)
}

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
        crate::cli::daemon::ask(
            || {
                let state = app.state::<crate::AppState>();
                crate::cli::daemon::ensure_running(&state.processes, &state.scheduler_log)
            },
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

/// The prompts the composer's arrow-up recalls, oldest first: the project's
/// input history the CLI keeps for every client, fed by each `run_agent_input`.
#[tauri::command]
pub(crate) async fn list_history(app: tauri::AppHandle, project: Option<String>) -> Vec<String> {
    ask_project(app, project, "list_history", "history").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_daemon_lists_files_and_open_issues() {
        let files = crate::cli::daemon_mock::answer(&project_frame("list_files", Path::new("/p")))
            .and_then(|frame| parse_list::<String>(&frame, "files"))
            .unwrap_or_default();
        assert_eq!(files.first().map(String::as_str), Some("README.md"));

        let issues =
            crate::cli::daemon_mock::answer(&project_frame("list_issues", Path::new("/p")))
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

    /// The composer config `infer config get composer` prints, with every
    /// trigger's sources in menu order and the status bar toggles beside them.
    const CLI_ANSWER: &str = r##"{
        "triggers": {
            "chat": { "@": ["files", "agents"], "#": ["issues"] },
            "room": { "@": ["agents", "files"], "#": ["issues"] }
        },
        "status_bar": {
            "enabled": true,
            "subagent_linger_seconds": 5,
            "indicators": { "cost": false }
        }
    }"##;

    #[test]
    fn the_cli_answer_holds_the_trigger_table_and_status_bar() {
        let composer: ComposerSettings = serde_json::from_str(CLI_ANSWER).unwrap();
        assert_eq!(
            composer.triggers["chat"]["@"],
            vec!["files".to_owned(), "agents".to_owned()]
        );
        assert_eq!(
            composer.triggers["room"]["@"],
            vec!["agents".to_owned(), "files".to_owned()]
        );
        assert_eq!(composer.triggers["chat"]["#"], vec!["issues".to_owned()]);
        assert_eq!(composer.status_bar.indicators["cost"], false);
    }

    /// Keys the CLI's answer leaves out keep the defaults, and a switch the
    /// field carries is read where it is.
    #[test]
    fn keys_the_answer_leaves_out_keep_the_defaults() {
        let composer: ComposerSettings =
            serde_json::from_str("{\"triggers\": {\"chat\": {}}}").unwrap();
        assert!(composer.triggers["chat"].is_empty());
        assert_eq!(composer.triggers.get("room"), None);
        assert!(composer.status_bar.enabled);
        assert!(composer.status_bar.indicators.is_empty());

        let composer: ComposerSettings =
            serde_json::from_str("{\"status_bar\": {\"enabled\": false}}").unwrap();
        assert!(composer.triggers.is_empty());
        assert!(!composer.status_bar.enabled);
        assert!(composer.status_bar.indicators.is_empty());
    }

    #[test]
    fn the_watcher_reacts_only_to_composer_yaml_events() {
        let event = |paths: &[&str]| {
            paths
                .iter()
                .fold(notify::Event::new(notify::EventKind::Any), |event, path| {
                    event.add_path(std::path::PathBuf::from(path))
                })
        };
        assert!(touches_composer_yaml(&event([
            "/home/x/.infer/composer.yaml"
        ])));
        assert!(touches_composer_yaml(&event([
            "/home/x/.infer/config.yaml",
            "/home/x/.infer/composer.yaml"
        ])));
        assert!(!touches_composer_yaml(&event([
            "/home/x/.infer/config.yaml",
            "/home/x/.infer/composer.yaml.tmp"
        ])));
        assert!(!touches_composer_yaml(&event([])));
    }
}
