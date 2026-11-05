use crate::daemon::Binding;
use crate::env::mock_mode;
use crate::thread::{exchange, uuid};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tauri::Manager;

/// The daemon probes every workspace for up to five seconds before it answers.
const LIST_TIMEOUT: Duration = Duration::from_secs(8);
/// A Kubernetes workspace pulls an image and boots a pod on its first teleport.
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(600);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const MOCK_WORKSPACE: &str = "mock-computer";

/// A computer a session can run on: an entry of the daemon's `workspaces`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Workspace {
    name: String,
    kind: String,
    reachable: bool,
    error: Option<String>,
}

/// Where a teleport stands, as the daemon reports it stage by stage.
#[derive(Debug, PartialEq)]
enum Stage {
    Progress(String),
    Ready,
    Failed(String),
}

fn teleport_frame(session_id: &str, project_dir: &str, target: &str, teleport_id: &str) -> Value {
    json!({
        "type": "teleport_session",
        "teleport_id": teleport_id,
        "project_dir": project_dir,
        "id": session_id,
        "target": target,
    })
}

fn read_frame_frame(session_id: &str, project_dir: &str) -> Value {
    json!({ "type": "read_frame", "project_dir": project_dir, "id": session_id })
}

fn parse_workspaces(frame: &Value) -> Option<Vec<Workspace>> {
    if frame["type"] != "workspaces" {
        return None;
    }
    let text = |entry: &Value, key: &str| entry[key].as_str().map(str::to_owned);
    let entries = frame["workspaces"].as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                Some(Workspace {
                    name: text(entry, "name")?,
                    kind: text(entry, "kind").unwrap_or_default(),
                    reachable: entry["reachable"].as_bool().unwrap_or(false),
                    error: text(entry, "error"),
                })
            })
            .collect(),
    )
}

fn parse_stage(frame: &Value) -> Option<Stage> {
    if frame["type"] != "teleport_progress" {
        return None;
    }
    Some(match frame["stage"].as_str()? {
        "ready" => Stage::Ready,
        "failed" => Stage::Failed(frame["message"].as_str().unwrap_or("").to_owned()),
        stage => Stage::Progress(stage.to_owned()),
    })
}

/// The screen a `frame` answer carries as a data URL. The outer `None` is a
/// frame of another kind, the inner one a session that streams no screen yet.
fn frame_data_url(frame: &Value) -> Option<Option<String>> {
    if frame["type"] != "frame" {
        return None;
    }
    let screen = &frame["frame"];
    Some(screen["data"].as_str().map(|data| {
        let format = screen["format"].as_str().unwrap_or("jpeg");
        format!("data:image/{format};base64,{data}")
    }))
}

/// A teleport ships a git bundle of `HEAD`, so a directory without a commit
/// gets a repository and an empty first commit.
fn ensure_commit(dir: &Path) -> Result<(), String> {
    use crate::projects::git_output;
    if git_output(dir, &["rev-parse", "--verify", "HEAD"]).is_ok() {
        return Ok(());
    }
    if git_output(dir, &["rev-parse", "--git-dir"]).is_err() {
        git_output(dir, &["init", "-b", "main"])?;
    }
    git_output(
        dir,
        &[
            "-c",
            "user.email=desktop@localhost",
            "-c",
            "user.name=Inference Gateway Desktop",
            "commit",
            "--allow-empty",
            "-m",
            "chore: start the workspace",
            "--no-verify",
        ],
    )
    .map(|_| ())
}

/// Places a session in the `target` workspace, reporting each stage on the
/// way. A failed teleport leaves the session where it was.
pub(crate) fn teleport(
    binding: &Binding,
    session_id: &str,
    project_dir: &Path,
    target: &str,
    mut on_stage: impl FnMut(&str),
) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }
    ensure_commit(project_dir)?;
    let request = teleport_frame(session_id, &project_dir.to_string_lossy(), target, &uuid());
    exchange(
        binding,
        request,
        TELEPORT_TIMEOUT,
        |frame| match parse_stage(frame)? {
            Stage::Progress(stage) => {
                on_stage(&stage);
                None
            }
            Stage::Ready => Some(Ok(())),
            Stage::Failed(message) => Some(Err(message)),
        },
    )?
    .map_err(|message| format!("teleport to {target} failed: {message}"))
}

fn mock_workspaces() -> Vec<Workspace> {
    vec![Workspace {
        name: MOCK_WORKSPACE.into(),
        kind: "kubernetes".into(),
        reachable: true,
        error: None,
    }]
}

fn mock_screen() -> String {
    use base64::Engine as _;
    let jpeg =
        base64::engine::general_purpose::STANDARD.encode(crate::screen_records::PLACEHOLDER_JPEG);
    format!("data:image/jpeg;base64,{jpeg}")
}

/// The daemon's workspaces. A daemon that cannot answer, such as a CLI from
/// before workspaces, has none.
#[tauri::command]
pub(crate) async fn list_workspaces(app: tauri::AppHandle) -> Vec<Workspace> {
    if mock_mode() {
        return mock_workspaces();
    }
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::ensure_running(&app.state::<crate::AppState>())?;
        exchange(
            &binding,
            json!({ "type": "list_workspaces" }),
            LIST_TIMEOUT,
            parse_workspaces,
        )
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}

/// The latest screen of the computer a session runs on, as a data URL.
/// ponytail: one connection per poll, keep one open if tiles outgrow a dozen.
#[tauri::command]
pub(crate) async fn screen_frame(session_id: String, project: Option<String>) -> Option<String> {
    if mock_mode() {
        return Some(mock_screen());
    }
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding().ok()?;
        let project_dir = crate::agent::thread_dir(project.as_deref());
        let request = read_frame_frame(&session_id, &project_dir.to_string_lossy());
        exchange(&binding, request, FRAME_TIMEOUT, frame_data_url).ok()?
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teleport_frame_names_the_session_and_its_target() {
        assert_eq!(
            teleport_frame("s1", "/proj", "ubuntu-desktop", "t1"),
            json!({
                "type": "teleport_session",
                "teleport_id": "t1",
                "project_dir": "/proj",
                "id": "s1",
                "target": "ubuntu-desktop",
            })
        );
        assert_eq!(
            read_frame_frame("s1", "/proj"),
            json!({ "type": "read_frame", "project_dir": "/proj", "id": "s1" })
        );
    }

    #[test]
    fn workspaces_keep_their_reachability_and_error() {
        let frame = json!({"type": "workspaces", "workspaces": [
            {"name": "laptop", "kind": "local", "reachable": true},
            {"name": "k8s", "kind": "kubernetes", "reachable": false, "error": "no operator"},
            {"kind": "daemon"},
        ]});
        let workspaces = parse_workspaces(&frame).unwrap();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].name, "laptop");
        assert!(workspaces[0].reachable);
        assert_eq!(workspaces[1].error.as_deref(), Some("no operator"));
        assert_eq!(parse_workspaces(&json!({"type": "frame"})), None);
    }

    #[test]
    fn stages_end_on_ready_or_failed() {
        let stage = |stage: &str| json!({"type": "teleport_progress", "stage": stage});
        assert_eq!(
            parse_stage(&stage("provisioning")),
            Some(Stage::Progress("provisioning".into()))
        );
        assert_eq!(parse_stage(&stage("ready")), Some(Stage::Ready));
        assert_eq!(
            parse_stage(
                &json!({"type": "teleport_progress", "stage": "failed", "message": "no pod"})
            ),
            Some(Stage::Failed("no pod".into()))
        );
        assert_eq!(parse_stage(&json!({"type": "session_teleported"})), None);
    }

    #[test]
    fn a_frame_becomes_a_data_url_and_an_error_is_no_screen() {
        let frame = json!({"type": "frame", "frame": {"data": "QUJD", "format": "png"}});
        assert_eq!(
            frame_data_url(&frame),
            Some(Some("data:image/png;base64,QUJD".into()))
        );
        assert_eq!(
            frame_data_url(&json!({"type": "frame", "error": "no screen"})),
            Some(None)
        );
        assert_eq!(frame_data_url(&json!({"type": "thread_activity"})), None);
    }

    #[test]
    fn ensure_commit_gives_an_empty_directory_a_first_commit_once() {
        let dir = std::env::temp_dir().join(format!("igd-workspace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        ensure_commit(&dir).unwrap();
        let head = crate::projects::git_output(&dir, &["rev-parse", "HEAD"]).unwrap();
        ensure_commit(&dir).unwrap();
        assert_eq!(
            crate::projects::git_output(&dir, &["rev-parse", "HEAD"]).unwrap(),
            head
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
