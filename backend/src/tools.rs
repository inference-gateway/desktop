use crate::agent::infer::{run_infer_blocking, run_infer_in};
use serde_json::Value;

/// The tools the agent can call, from an `infer tools list --format json`
/// dump. The CLI's registry owns the list, so the desktop keeps no copy of it.
/// Anything unparseable yields nothing.
pub(crate) fn tool_names_in(dump: &str) -> Vec<String> {
    serde_json::from_str::<Value>(dump)
        .ok()
        .and_then(|v| serde_json::from_value(v.get("tools")?.clone()).ok())
        .unwrap_or_default()
}

/// Slash commands the composer offers, in display order. Everything else the
/// CLI registers only opens a chat-TUI panel and has nothing to do here.
/// ponytail: add a name here to surface a new CLI shortcut in the composer.
const COMPOSER_SHORTCUTS: &[&str] = &["init", "install-opentask"];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Shortcut {
    pub(crate) name: String,
    pub(crate) description: String,
}

/// The allowlisted shortcuts present in an `infer shortcuts list --format json`
/// dump, ordered as `COMPOSER_SHORTCUTS`. Anything unparseable yields nothing,
/// so an older CLI without the command just leaves the dropdown to skills.
pub(crate) fn composer_shortcuts_in(dump: &str) -> Vec<Shortcut> {
    let listed: Vec<Shortcut> = serde_json::from_str::<Value>(dump)
        .ok()
        .and_then(|v| serde_json::from_value(v.get("shortcuts")?.clone()).ok())
        .unwrap_or_default();
    COMPOSER_SHORTCUTS
        .iter()
        .filter_map(|name| listed.iter().find(|s| s.name == *name).cloned())
        .collect()
}

#[tauri::command]
pub(crate) async fn list_shortcuts() -> Result<Vec<Shortcut>, String> {
    let dump = run_infer_in(None, &["shortcuts", "list", "--format", "json"]).await?;
    Ok(composer_shortcuts_in(&dump))
}

#[tauri::command]
pub(crate) async fn list_tools() -> Result<Vec<String>, String> {
    let dump = run_infer_in(None, &["tools", "list", "--format", "json"]).await?;
    Ok(tool_names_in(&dump))
}

/// `infer mcp status --format json`: the counts the status bar shows and the
/// per-server rows the MCPs tab shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct McpStatus {
    pub(crate) enabled: bool,
    pub(crate) total_servers: u64,
    pub(crate) connected_servers: u64,
    pub(crate) total_tools: u64,
    #[serde(default)]
    pub(crate) servers: Vec<McpServerStatus>,
}

/// One server's row of `infer mcp status`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct McpServerStatus {
    pub(crate) name: String,
    pub(crate) connected: bool,
    #[serde(default)]
    pub(crate) tools: u64,
    #[serde(default)]
    pub(crate) error: String,
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

/// Serialises shared container startup against session spawns. A headless
/// session that starts while `infer agents start` is still pulling an image
/// cannot see the shared container in `docker ps` yet, so it starts its own
/// and the two race for the port; the shared one loses and the session's
/// container dies with the session.
static SERVICES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Waits for an in-flight `start_services` to finish. Call before spawning a
/// session that starts local agents on its own.
pub(crate) async fn await_services() {
    drop(SERVICES.lock().await);
}

/// Starts `run: true` MCP servers and A2A agents as detached shared containers
/// so per-message `infer headless` sessions reuse them. Failures are ignored:
/// an older CLI without the commands, or no Docker, just leaves the counts at 0.
#[tauri::command]
pub(crate) async fn start_services() {
    let _guard = SERVICES.lock().await;
    let _ = run_infer_in(None, &["mcp", "start"]).await;
    let _ = run_infer_in(None, &["agents", "start"]).await;
}

/// Stops the detached shared containers `start_services` started, so quitting
/// the desktop leaves nothing running and the next launch recreates them from
/// the installed CLI (an upgraded CLI may inject different env). Failures are
/// ignored for the same reasons as start.
pub(crate) fn stop_services() {
    for args in [["mcp", "stop"], ["agents", "stop"]] {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let _ = run_infer_blocking(None, &args);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_shortcuts_keep_allowlist_order_and_drop_the_rest() {
        let dump = r#"{"shortcuts":[
            {"name":"exit","description":"Exit","usage":"/exit"},
            {"name":"install-opentask","description":"Set up opentask","usage":"/install-opentask"},
            {"name":"init","description":"Create AGENTS.md","usage":"/init"}
        ],"total":3}"#;
        let found = composer_shortcuts_in(dump);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["init", "install-opentask"]);
        assert_eq!(found[0].description, "Create AGENTS.md");
        assert!(composer_shortcuts_in("not json").is_empty());
        assert!(composer_shortcuts_in(r#"{"total":0}"#).is_empty());
    }

    #[test]
    fn status_reports_keep_the_counts_and_the_server_rows() {
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
                total_tools: 14,
                servers: vec![McpServerStatus {
                    name: "fs".into(),
                    connected: true,
                    tools: 14,
                    error: String::new(),
                }],
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
    fn tool_names_read_the_cli_tool_list() {
        let dump = r#"{"tools": ["Agent", "Bash", "Read"], "total": 3}"#;
        assert_eq!(tool_names_in(dump), vec!["Agent", "Bash", "Read"]);
    }

    #[test]
    fn tool_names_empty_when_dump_unparseable() {
        assert!(tool_names_in("not json").is_empty());
        assert!(tool_names_in(r#"{"total": 0}"#).is_empty());
    }
}
