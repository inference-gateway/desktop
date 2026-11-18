use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const WORKSPACE: &str = "mock-computer";
const ARTIFACT: &str = "demo.gif";
const FILES: &[&str] = &[
    "README.md",
    "backend/src/main.rs",
    "frontend/App.tsx",
    "~/.infer/config.yaml",
];
const ISSUES: &[(u64, &str)] = &[(57, "Fix the flaky test"), (42, "Add a dark mode toggle")];

fn placeholder_image() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(crate::screen_records::PLACEHOLDER_JPEG)
}

/// Where mock sessions keep their own artifacts, so a scenario's tool call can
/// produce one. It lives in the mock HOME, which every e2e test starts empty.
fn own_artifacts_dir() -> PathBuf {
    crate::env::infer_dir().join("mock-artifacts")
}

/// The sessions' own artifacts, newest first, then the demo GIF every session has.
fn listed_artifacts(dir: &Path) -> Vec<Value> {
    let mut own: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok().filter(|m| m.is_file())?;
            Some((
                meta.modified().ok()?,
                entry.file_name().into_string().ok()?,
                meta.len(),
            ))
        })
        .collect();
    own.sort_by_key(|a| std::cmp::Reverse(a.0));
    own.into_iter()
        .map(|(_, name, size)| json!({ "name": name, "size": size, "modified_at": "" }))
        .chain([json!({ "name": ARTIFACT, "size": 43, "modified_at": "" })])
        .collect()
}

/// One of the sessions' own artifacts, or the placeholder picture for any other name.
fn read_artifact(dir: &Path, name: &str) -> String {
    use base64::Engine as _;
    std::fs::read(dir.join(name))
        .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
        .unwrap_or_else(|_| placeholder_image())
}

/// The frame a daemon with one workspace would answer `request` with, in the
/// daemon's own wire format, so mock mode runs the real parsers. `None` is a
/// request the mock daemon itself can answer, such as `list_tasks`.
pub(crate) fn answer(request: &Value) -> Option<Value> {
    let id = &request["id"];
    Some(match request["type"].as_str()? {
        "list_workspaces" => json!({ "type": "workspaces", "workspaces": [
            { "name": WORKSPACE, "kind": "kubernetes", "reachable": true }
        ]}),
        "list_artifacts" => {
            json!({ "type": "artifacts", "artifacts": listed_artifacts(&own_artifacts_dir()) })
        }
        "read_artifact" => {
            json!({ "type": "artifact", "name": request["name"], "data": read_artifact(&own_artifacts_dir(), request["name"].as_str().unwrap_or("")) })
        }
        "stop_thread" => json!({ "type": "thread_stopped", "id": id, "workspace": WORKSPACE }),
        "read_frame" => json!({ "type": "frame", "frame": {
            "id": "", "data": placeholder_image(), "format": "jpeg", "width": 0, "height": 0
        }}),
        "screen_control" => {
            json!({ "type": "screen_result", "held": request["action"] == "take_over" })
        }
        "screen_input" => json!({ "type": "screen_result", "held": true }),
        "list_files" => json!({ "type": "files", "files": FILES }),
        "list_issues" => json!({ "type": "issues", "issues": ISSUES
            .iter()
            .map(|(number, title)| json!({ "number": number, "title": title, "state": "OPEN" }))
            .collect::<Vec<_>>()
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_s_own_artifacts_come_first_and_read_back() {
        let dir = std::env::temp_dir().join(format!("igd-mock-artifacts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("draft-v1.wav"), b"RIFF").unwrap();

        let names: Vec<_> = listed_artifacts(&dir)
            .iter()
            .map(|a| a["name"].clone())
            .collect();
        assert_eq!(names, [json!("draft-v1.wav"), json!(ARTIFACT)]);
        assert_eq!(read_artifact(&dir, "draft-v1.wav"), "UklGRg==");
        assert_eq!(read_artifact(&dir, ARTIFACT), placeholder_image());

        std::fs::remove_dir_all(dir).unwrap();
    }
}
