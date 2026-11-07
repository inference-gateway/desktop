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
/// Taking a screen over can relaunch the session's worker first.
const SCREEN_TIMEOUT: Duration = Duration::from_secs(15);
/// Listing wakes a suspended workspace first.
const ARTIFACTS_TIMEOUT: Duration = Duration::from_secs(120);
const ARTIFACT_TIMEOUT: Duration = Duration::from_secs(300);
const MOCK_ARTIFACT: &str = "demo.gif";
/// The daemon gives a workspace a minute to delete a session's machine.
const STOP_TIMEOUT: Duration = Duration::from_secs(70);
const MOCK_WORKSPACE: &str = "mock-computer";

/// A computer a session can run on: an entry of the daemon's `workspaces`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Workspace {
    name: String,
    kind: String,
    reachable: bool,
    error: Option<String>,
}

/// The latest screen of a session's computer and the frame space a person's
/// pointer maps onto while they drive it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Screen {
    src: String,
    width: u64,
    height: u64,
}

/// A file a session produced, as the worker that owns the session lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Artifact {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    modified_at: String,
}

/// A chat whose machine is to be removed, with the project it runs in.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct MachineSession {
    id: String,
    project: Option<String>,
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

fn screen_control_frame(session_id: &str, project_dir: &str, take_over: bool) -> Value {
    let action = if take_over { "take_over" } else { "hand_back" };
    json!({ "type": "screen_control", "project_dir": project_dir, "id": session_id, "action": action })
}

fn screen_input_frame(session_id: &str, project_dir: &str, input: &Value) -> Value {
    json!({ "type": "screen_input", "project_dir": project_dir, "id": session_id, "input": input })
}

/// Whether the person holds the screen after a `screen_result`, or why the
/// worker refused. `None` is a frame of another kind.
fn parse_screen_result(frame: &Value) -> Option<Result<bool, String>> {
    if frame["type"] != "screen_result" {
        return None;
    }
    Some(match frame["error"].as_str().filter(|e| !e.is_empty()) {
        Some(error) => Err(error.to_owned()),
        None => Ok(frame["held"].as_bool().unwrap_or(false)),
    })
}

fn stop_thread_frame(session_id: &str, project_dir: &str) -> Value {
    json!({ "type": "stop_thread", "project_dir": project_dir, "id": session_id })
}

/// Whether a `thread_stopped` answer for `session_id` removed a machine, or why
/// the workspace kept it. `None` is a frame of another kind or session.
fn parse_thread_stopped(frame: &Value, session_id: &str) -> Option<Result<bool, String>> {
    if frame["type"] != "thread_stopped" || frame["id"] != session_id {
        return None;
    }
    Some(match frame["error"].as_str().filter(|e| !e.is_empty()) {
        Some(error) => Err(error.to_owned()),
        None => Ok(frame["workspace"].as_str().is_some_and(|w| !w.is_empty())),
    })
}

fn artifact_frame(kind: &str, session_id: &str, project_dir: &str) -> Value {
    json!({ "type": kind, "project_dir": project_dir, "id": session_id })
}

/// The files an `artifacts` answer lists, newest first as the worker sends them.
fn parse_artifacts(frame: &Value) -> Option<Vec<Artifact>> {
    if frame["type"] != "artifacts" {
        return None;
    }
    Some(serde_json::from_value(frame["artifacts"].clone()).unwrap_or_default())
}

/// The bytes of the file an `artifact` answer carries, or why the worker could
/// not send it. `None` is a frame of another kind.
fn parse_artifact(frame: &Value) -> Option<Result<Vec<u8>, String>> {
    use base64::Engine as _;
    if frame["type"] != "artifact" {
        return None;
    }
    if let Some(error) = frame["error"].as_str().filter(|e| !e.is_empty()) {
        return Some(Err(error.to_owned()));
    }
    let data = frame["data"].as_str().unwrap_or("");
    Some(
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("the artifact did not arrive intact: {e}")),
    )
}

/// An artifact is a bare file name, so a saved one cannot land outside Downloads.
fn artifact_file_name(name: &str) -> Result<&str, String> {
    let bare = !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']);
    bare.then_some(name)
        .ok_or_else(|| format!("{name:?} is not an artifact file name"))
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

/// The screen a `frame` answer carries, as a data URL. The outer `None` is a
/// frame of another kind, the inner one a session that streams no screen yet.
fn parse_screen(frame: &Value) -> Option<Option<Screen>> {
    if frame["type"] != "frame" {
        return None;
    }
    let screen = &frame["frame"];
    Some(screen["data"].as_str().map(|data| {
        let format = screen["format"].as_str().unwrap_or("jpeg");
        Screen {
            src: format!("data:image/{format};base64,{data}"),
            width: screen["width"].as_u64().unwrap_or(0),
            height: screen["height"].as_u64().unwrap_or(0),
        }
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

fn mock_screen() -> Screen {
    use base64::Engine as _;
    let jpeg =
        base64::engine::general_purpose::STANDARD.encode(crate::screen_records::PLACEHOLDER_JPEG);
    Screen {
        src: format!("data:image/jpeg;base64,{jpeg}"),
        width: 0,
        height: 0,
    }
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

/// The files a session produced, read from wherever the session runs.
#[tauri::command]
pub(crate) async fn list_artifacts(
    session_id: String,
    project: Option<String>,
) -> Result<Vec<Artifact>, String> {
    if mock_mode() {
        return Ok(vec![Artifact {
            name: MOCK_ARTIFACT.into(),
            size: 43,
            modified_at: String::new(),
        }]);
    }
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding()?;
        let project_dir = crate::agent::thread_dir(project.as_deref());
        let request = artifact_frame(
            "list_artifacts",
            &session_id,
            &project_dir.to_string_lossy(),
        );
        exchange(&binding, request, ARTIFACTS_TIMEOUT, parse_artifacts)
    })
    .await
    .map_err(|e| format!("artifacts task failed: {e}"))?
}

fn fetch_artifact(session_id: &str, project: Option<&str>, name: &str) -> Result<Vec<u8>, String> {
    if mock_mode() {
        return Ok(crate::screen_records::PLACEHOLDER_JPEG.to_vec());
    }
    let binding = crate::daemon::binding()?;
    let project_dir = crate::agent::thread_dir(project);
    let mut request = artifact_frame("read_artifact", session_id, &project_dir.to_string_lossy());
    request["name"] = json!(name);
    exchange(&binding, request, ARTIFACT_TIMEOUT, parse_artifact)?
}

/// Saves one of a session's artifacts to the Downloads folder, fetching it from
/// the session's workspace, and shows it in the file manager. Answers its path.
#[tauri::command]
pub(crate) async fn save_artifact(
    session_id: String,
    project: Option<String>,
    name: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let file_name = artifact_file_name(&name)?;
        let bytes = fetch_artifact(&session_id, project.as_deref(), file_name)?;
        let downloads = crate::env::home_dir().join("Downloads");
        std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
        let saved = downloads.join(file_name);
        std::fs::write(&saved, bytes).map_err(|e| format!("saving {}: {e}", saved.display()))?;
        if !mock_mode() {
            let _ = crate::timeline::reveal(&saved);
        }
        Ok(saved.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("artifact task failed: {e}"))?
}

/// Has the daemon delete the machine of one chat and release its lease, so the
/// chat starts on this computer next time. Answers whether a machine went.
fn remove_machine(binding: &Binding, session: &MachineSession) -> Result<bool, String> {
    let project_dir = crate::agent::thread_dir(session.project.as_deref());
    let request = stop_thread_frame(&session.id, &project_dir.to_string_lossy());
    exchange(binding, request, STOP_TIMEOUT, |frame| {
        parse_thread_stopped(frame, &session.id)
    })?
}

/// ponytail: one chat after another, each waiting on its workspace. Stop them
/// side by side with `std::thread::scope` once a clean-up of many chats is slow.
fn remove_machines(sessions: &[MachineSession]) -> Result<usize, String> {
    let binding = crate::daemon::binding()?;
    let of_chats = sessions.iter().map(|session| {
        remove_machine(&binding, session)
            .map(usize::from)
            .map_err(|error| format!("{}: {error}", session.id))
    });
    let configs = crate::workspace_settings::workspace_configs();
    let left_behind = configs
        .iter()
        .map(crate::workspace_settings::delete_machines);
    let (removed, failures): (Vec<_>, Vec<_>) =
        of_chats.chain(left_behind).partition(Result::is_ok);
    if failures.is_empty() {
        Ok(removed.into_iter().flatten().sum())
    } else {
        let reasons: Vec<String> = failures.into_iter().filter_map(Result::err).collect();
        Err(reasons.join("\n"))
    }
}

/// Removes every machine of the workspaces: those of the given chats through
/// the daemon, which also releases their leases, then any a cluster still has.
/// Answers how many machines went.
#[tauri::command]
pub(crate) async fn clean_machines(sessions: Vec<MachineSession>) -> Result<usize, String> {
    if mock_mode() {
        return Ok(sessions.len());
    }
    tokio::task::spawn_blocking(move || remove_machines(&sessions))
        .await
        .map_err(|e| format!("clean-up task failed: {e}"))?
}

/// Has the daemon delete the machine of one chat, with the transcript on it.
/// The chat's next message starts it over. Answers whether a machine went.
#[tauri::command]
pub(crate) async fn delete_machine(session: MachineSession) -> Result<bool, String> {
    if mock_mode() {
        return Ok(true);
    }
    tokio::task::spawn_blocking(move || remove_machine(&crate::daemon::binding()?, &session))
        .await
        .map_err(|e| format!("delete task failed: {e}"))?
}

/// The latest screen of the computer a session runs on.
/// ponytail: one connection per poll, keep one open if tiles outgrow a dozen.
#[tauri::command]
pub(crate) async fn screen_frame(session_id: String, project: Option<String>) -> Option<Screen> {
    if mock_mode() {
        return Some(mock_screen());
    }
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding().ok()?;
        let project_dir = crate::agent::thread_dir(project.as_deref());
        let request = read_frame_frame(&session_id, &project_dir.to_string_lossy());
        exchange(&binding, request, FRAME_TIMEOUT, parse_screen).ok()?
    })
    .await
    .ok()
    .flatten()
}

/// Sends one screen request for a session and answers whether the person
/// holds its screen afterwards.
async fn screen_request(
    project: Option<String>,
    request: impl FnOnce(&str) -> Value + Send + 'static,
) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let binding = crate::daemon::binding()?;
        let project_dir = crate::agent::thread_dir(project.as_deref());
        let request = request(&project_dir.to_string_lossy());
        exchange(&binding, request, SCREEN_TIMEOUT, parse_screen_result)?
    })
    .await
    .map_err(|e| format!("screen task failed: {e}"))?
}

/// Takes the screen of a session's computer over for the person, or hands it
/// back to the agent. Answers whether the person holds it afterwards.
#[tauri::command]
pub(crate) async fn screen_control(
    session_id: String,
    project: Option<String>,
    take_over: bool,
) -> Result<bool, String> {
    if mock_mode() {
        return Ok(take_over);
    }
    screen_request(project, move |dir| {
        screen_control_frame(&session_id, dir, take_over)
    })
    .await
}

/// Drives a held screen with one input of the person, in frame space.
/// ponytail: one connection per input, keep one open if typing feels slow.
#[tauri::command]
pub(crate) async fn screen_input(
    session_id: String,
    project: Option<String>,
    input: Value,
) -> Result<bool, String> {
    if mock_mode() {
        return Ok(true);
    }
    screen_request(project, move |dir| {
        screen_input_frame(&session_id, dir, &input)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_artifacts_answer_lists_the_files_and_an_artifact_answer_carries_one() {
        let listed = json!({ "type": "artifacts", "artifacts": [
            { "name": "x.mp4", "size": 60416, "modified_at": "2026-10-28T20:25:18Z" }
        ]});
        assert_eq!(
            parse_artifacts(&listed),
            Some(vec![Artifact {
                name: "x.mp4".into(),
                size: 60416,
                modified_at: "2026-10-28T20:25:18Z".into(),
            }])
        );
        assert_eq!(
            parse_artifacts(&json!({ "type": "artifacts" })),
            Some(vec![])
        );
        assert_eq!(parse_artifacts(&json!({ "type": "frame" })), None);

        let file = json!({ "type": "artifact", "name": "x.txt", "data": "aGk=" });
        assert_eq!(parse_artifact(&file), Some(Ok(b"hi".to_vec())));
        let refused = json!({ "type": "artifact", "error": "too large" });
        assert_eq!(parse_artifact(&refused), Some(Err("too large".into())));
        assert_eq!(parse_artifact(&json!({ "type": "artifacts" })), None);
    }

    #[test]
    fn only_a_bare_file_name_is_an_artifact() {
        assert_eq!(artifact_file_name("demo.gif"), Ok("demo.gif"));
        for name in ["", ".", "..", "../x", "a/b", "a\\b", "/etc/passwd"] {
            assert!(artifact_file_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn a_stopped_thread_tells_whether_its_machine_went() {
        let stopped = |extra: Value| {
            let mut frame = json!({ "type": "thread_stopped", "id": "s1" });
            frame
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            parse_thread_stopped(&frame, "s1")
        };
        assert_eq!(stopped(json!({ "workspace": "k8s" })), Some(Ok(true)));
        assert_eq!(stopped(json!({ "workspace": "" })), Some(Ok(false)));
        assert_eq!(
            stopped(json!({ "error": "cluster unreachable" })),
            Some(Err("cluster unreachable".into()))
        );
        assert_eq!(stopped(json!({ "id": "s2", "workspace": "k8s" })), None);
        assert_eq!(
            parse_thread_stopped(&json!({ "type": "frame" }), "s1"),
            None
        );
    }

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
        let frame = json!({"type": "frame", "frame": {"data": "QUJD", "format": "png", "width": 1280, "height": 800}});
        assert_eq!(
            parse_screen(&frame),
            Some(Some(Screen {
                src: "data:image/png;base64,QUJD".into(),
                width: 1280,
                height: 800,
            }))
        );
        assert_eq!(
            parse_screen(&json!({"type": "frame", "error": "no screen"})),
            Some(None)
        );
        assert_eq!(parse_screen(&json!({"type": "thread_activity"})), None);
    }

    #[test]
    fn screen_frames_name_the_session_and_what_the_person_does() {
        assert_eq!(
            screen_control_frame("s1", "/proj", true),
            json!({"type": "screen_control", "project_dir": "/proj", "id": "s1", "action": "take_over"})
        );
        assert_eq!(
            screen_control_frame("s1", "/proj", false)["action"],
            "hand_back"
        );
        let click = json!({"kind": "click", "x": 64, "y": 32});
        assert_eq!(
            screen_input_frame("s1", "/proj", &click),
            json!({"type": "screen_input", "project_dir": "/proj", "id": "s1", "input": click})
        );
    }

    #[test]
    fn a_screen_result_says_who_holds_the_screen_or_why_not() {
        let result = |held: bool| json!({"type": "screen_result", "held": held});
        assert_eq!(parse_screen_result(&result(true)), Some(Ok(true)));
        assert_eq!(parse_screen_result(&result(false)), Some(Ok(false)));
        assert_eq!(
            parse_screen_result(
                &json!({"type": "screen_result", "held": true, "error": "already held"})
            ),
            Some(Err("already held".into()))
        );
        assert_eq!(parse_screen_result(&json!({"type": "frame"})), None);
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
