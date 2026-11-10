use serde_json::{Value, json};

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

/// The frame a daemon with one workspace would answer `request` with, in the
/// daemon's own wire format, so mock mode runs the real parsers. `None` is a
/// request the mock daemon itself can answer, such as `list_tasks`.
pub(crate) fn answer(request: &Value) -> Option<Value> {
    let id = &request["id"];
    Some(match request["type"].as_str()? {
        "list_workspaces" => json!({ "type": "workspaces", "workspaces": [
            { "name": WORKSPACE, "kind": "kubernetes", "reachable": true }
        ]}),
        "list_artifacts" => json!({ "type": "artifacts", "artifacts": [
            { "name": ARTIFACT, "size": 43, "modified_at": "" }
        ]}),
        "read_artifact" => {
            json!({ "type": "artifact", "name": request["name"], "data": placeholder_image() })
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
