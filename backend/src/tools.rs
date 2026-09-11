use crate::agent::run_infer_in;
use serde_json::Value;

/// `tools.<key>` config entries mapped to the CLI registry's tool names.
/// Non-tool subtrees (safety, sandbox) have no entry here and are never listed.
/// ponytail: a new CLI tool needs an entry here - the dropdown derives from the
/// effective config, while execution still validates enablement inside the CLI.
const TOOLS: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("read", "Read"),
    ("write", "Write"),
    ("edit", "Edit"),
    ("delete", "Delete"),
    ("grep", "Grep"),
    ("tree", "Tree"),
    ("web_fetch", "WebFetch"),
    ("web_search", "WebSearch"),
    ("todo_write", "TodoWrite"),
    ("image_generation", "ImageGeneration"),
    ("image_edit", "ImageEdit"),
    ("image_variation", "ImageVariation"),
    ("text_to_speech", "TextToSpeech"),
    ("computer", "Computer"),
];

/// Enabled tool names parsed from a `infer config get tools --format json`
/// dump (effective config: built-in defaults + user/project yaml + INFERER_*
/// env). Runs from the default agent cwd; a project-level tools override would
/// need the cwd - ponytail: pass one through if that bites.
pub(crate) fn enabled_tools_in(dump: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(dump) else {
        return Vec::new();
    };
    if !value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    TOOLS
        .iter()
        .filter(|(key, _)| {
            value
                .get(*key)
                .and_then(|tool| tool.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .map(|(_, name)| (*name).to_string())
        .collect()
}

#[tauri::command]
pub(crate) async fn list_tools() -> Result<Vec<String>, String> {
    let dump = run_infer_in(None, &["config", "get", "tools", "--format", "json"]).await?;
    Ok(enabled_tools_in(&dump))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[test]
    fn enabled_tools_respects_the_master_switch_and_per_tool_flags() {
        let dump = r#"{
            "enabled": true,
            "safety": {"require_approval": true},
            "bash": {"enabled": true, "mode": {"all": {"allow": ["ls"]}}},
            "read": {"enabled": true},
            "write": {"enabled": false},
            "grep": {"backend": "auto"}
        }"#;
        assert_eq!(enabled_tools_in(dump), vec!["Bash", "Read"]);
    }

    #[test]
    fn enabled_tools_empty_when_tools_disabled_or_dump_unparseable() {
        assert!(enabled_tools_in(r#"{"enabled": false, "read": {"enabled": true}}"#).is_empty());
        assert!(enabled_tools_in("not json").is_empty());
    }
}
