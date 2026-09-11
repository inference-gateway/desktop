use crate::agent::run_infer_in;
use crate::env::agent_cwd;
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

/// True when `name` is safe to pass to `infer tools execute`.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Execute a tool directly through the CLI's own registry and validation path
/// (`infer tools execute`), from the project's directory so relative paths
/// behave like an in-session tool call. No LLM involved.
#[tauri::command]
pub(crate) async fn execute_tool(
    name: String,
    args: String,
    project: Option<String>,
) -> Result<String, String> {
    if !valid_name(&name) {
        return Err(format!("invalid tool name: {name}"));
    }
    let parsed: Value =
        serde_json::from_str(&args).map_err(|e| format!("arguments must be a JSON object: {e}"))?;
    if !parsed.is_object() {
        return Err("arguments must be a JSON object".into());
    }
    let cwd = project
        .as_deref()
        .and_then(crate::projects::project_dir)
        .filter(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or_else(agent_cwd);
    run_infer_in(
        Some(cwd.to_string_lossy().into_owned()),
        &["tools", "execute", &name, &args],
    )
    .await
    .map(|out| strip_ansi(&out))
}

// ponytail: `infer tools execute` ignores NO_COLOR/--no-colors (CLI 0.189),
// so CSI sequences are stripped here; drop this once the CLI honors them.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_sequences_only() {
        assert_eq!(
            strip_ansi("\x1b[38;2;1;2;3m│\x1b[m hi \x1b[1mbold\x1b[0m"),
            "│ hi bold"
        );
        assert_eq!(strip_ansi("plain\x1b"), "plain\x1b");
    }

    #[test]
    fn name_validation() {
        assert!(valid_name("Bash"));
        assert!(valid_name("A2A_QueryAgent"));
        assert!(!valid_name(""));
        assert!(!valid_name("-flag"));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("a b"));
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
