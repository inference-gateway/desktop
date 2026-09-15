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

/// Counts from `infer mcp status --format json`; per-server rows are ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct McpStatus {
    pub(crate) enabled: bool,
    pub(crate) total_servers: u64,
    pub(crate) connected_servers: u64,
    pub(crate) total_tools: u64,
}

/// Counts from `infer agents status --format json`; per-agent rows are ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct A2aStatus {
    pub(crate) total_agents: u64,
    pub(crate) ready_agents: u64,
}

#[tauri::command]
pub(crate) async fn mcp_status() -> Result<McpStatus, String> {
    let dump = run_infer_in(None, &["mcp", "status", "--format", "json"]).await?;
    serde_json::from_str(&dump).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn a2a_status() -> Result<A2aStatus, String> {
    let dump = run_infer_in(None, &["agents", "status", "--format", "json"]).await?;
    serde_json::from_str(&dump).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_keep_the_counts_and_drop_the_rows() {
        let mcp: McpStatus = serde_json::from_str(
            r#"{"enabled":true,"total_servers":2,"connected_servers":1,"total_tools":14,
                "servers":[{"name":"fs","connected":true,"tools":14}]}"#,
        )
        .unwrap();
        assert_eq!(
            mcp,
            McpStatus {
                enabled: true,
                total_servers: 2,
                connected_servers: 1,
                total_tools: 14
            }
        );

        let a2a: A2aStatus =
            serde_json::from_str(r#"{"total_agents":1,"ready_agents":0,"agents":[]}"#).unwrap();
        assert_eq!(
            a2a,
            A2aStatus {
                total_agents: 1,
                ready_agents: 0
            }
        );
    }

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
