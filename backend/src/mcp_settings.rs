use crate::config;
use crate::env::infer_dir;
use crate::secrets::valid_variable;
use crate::workspace_settings::{
    self, KUBERNETES, WorkspaceConfig, filled, named_entries, remove_named, set_or_remove, text,
};
use serde::{Deserialize, Serialize};
use serde_norway::{Mapping, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const MCP_FILE: &str = "mcp.yaml";
const SERVERS_KEY: &str = "servers";
const WORKSPACE_KEY: &str = "mcp_servers";
const NAME_MAX_LEN: usize = 64;

/// One header of a hosted server, written as `<prefix>${<variable>}` so the
/// value is a reference the computer expands from its secrets, never a secret.
/// A header written otherwise in mcp.yaml reads back as `literal`, its value
/// left out so it never reaches the tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct McpHeader {
    name: String,
    prefix: String,
    variable: String,
    literal: bool,
}

/// An MCP server of mcp.yaml as the MCPs tab edits it: a hosted one has a URL
/// and headers, a container one (`run`) an image and a port. `enabled` is
/// whether this computer connects to it, and `workspaces` the Kubernetes
/// workspaces whose pods get it. A key the tab does not know stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct McpServerConfig {
    pub(crate) name: String,
    enabled: bool,
    run: bool,
    url: String,
    headers: Vec<McpHeader>,
    image: String,
    port: u16,
    workspaces: Vec<String>,
}

/// The global MCP switch and every server of mcp.yaml.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct McpSettings {
    enabled: bool,
    servers: Vec<McpServerConfig>,
}

fn mcp_path() -> PathBuf {
    infer_dir().join(MCP_FILE)
}

fn header_of(name: String, value: &str) -> McpHeader {
    let reference = value
        .strip_suffix('}')
        .and_then(|rest| rest.rsplit_once("${"))
        .filter(|(_, variable)| valid_variable(variable));
    match reference {
        Some((prefix, variable)) => McpHeader {
            name,
            prefix: prefix.to_owned(),
            variable: variable.to_owned(),
            literal: false,
        },
        None => McpHeader {
            name,
            literal: true,
            ..McpHeader::default()
        },
    }
}

fn headers_of(entry: &Value) -> Vec<McpHeader> {
    entry
        .get("headers")
        .and_then(Value::as_mapping)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    Some(header_of(name.as_str()?.to_owned(), value.as_str()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn server_of(entry: &Value, workspaces: &[WorkspaceConfig]) -> Option<McpServerConfig> {
    let name = text(entry, "name");
    if name.is_empty() {
        return None;
    }
    Some(McpServerConfig {
        enabled: entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        run: entry.get("run").and_then(Value::as_bool).unwrap_or(false),
        url: text(entry, "url"),
        headers: headers_of(entry),
        image: text(entry, "oci"),
        port: entry
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or(0),
        workspaces: workspaces
            .iter()
            .filter(|w| w.mcp_servers.contains(&name))
            .map(|w| w.name.clone())
            .collect(),
        name,
    })
}

fn parse_settings(mcp_yaml: Option<&str>, workspaces: &[WorkspaceConfig]) -> McpSettings {
    let root = config::config_mapping(mcp_yaml);
    McpSettings {
        enabled: root
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        servers: root
            .get(SERVERS_KEY)
            .and_then(Value::as_sequence)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| server_of(entry, workspaces))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The CLI names a server's tools after it, so a name is lowercase letters,
/// digits, `-` and `_`.
fn valid_server_name(name: &str) -> bool {
    (1..=NAME_MAX_LEN).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Why `server` cannot be saved beside the `existing` servers, or None when
/// it can. A header must expand a variable some secret or API key provides,
/// and a container server runs on this computer alone.
fn problem(
    server: &McpServerConfig,
    is_new: bool,
    existing: &[McpServerConfig],
    workspaces: &[WorkspaceConfig],
    variables: &BTreeSet<String>,
) -> Option<String> {
    let name = server.name.as_str();
    if !valid_server_name(name) {
        return Some("A server's name is lowercase letters, digits, '-' and '_'.".into());
    }
    let found = existing.iter().find(|other| other.name == name);
    match (is_new, found) {
        (true, Some(_)) => return Some(format!("A server named {name} already exists.")),
        (false, None) => return Some(format!("There is no server named {name}.")),
        (false, Some(stored)) if stored.run != server.run => {
            return Some(format!(
                "{name} cannot change between hosted and container."
            ));
        }
        _ => {}
    }
    if server.run {
        if server.image.trim().is_empty() || server.port == 0 {
            return Some("A container server needs an image and the port it serves on.".into());
        }
        if !server.workspaces.is_empty() {
            return Some("A container server runs on this computer only.".into());
        }
    } else if !server.url.starts_with("https://") && !server.url.starts_with("http://") {
        return Some("A hosted server needs an http:// or https:// URL.".into());
    }
    let mut seen = BTreeSet::new();
    for header in &server.headers {
        let header_name = header.name.trim();
        if header_name.is_empty() || !seen.insert(header_name.to_ascii_lowercase()) {
            return Some("Every header needs a name of its own.".into());
        }
        if !variables.contains(&header.variable) {
            return Some(format!(
                "Header {header_name} needs a secret. Add one under Secrets, then pick it here."
            ));
        }
    }
    server
        .workspaces
        .iter()
        .find(|name| {
            !workspaces
                .iter()
                .any(|w| &w.name == *name && w.kind == KUBERNETES)
        })
        .map(|name| format!("{name} is not a Kubernetes workspace."))
}

fn headers_value(headers: &[McpHeader]) -> Option<Value> {
    let mapping: Mapping = headers
        .iter()
        .map(|header| {
            (
                header.name.trim().into(),
                format!("{}${{{}}}", header.prefix, header.variable).into(),
            )
        })
        .collect();
    (!mapping.is_empty()).then_some(Value::Mapping(mapping))
}

fn apply(entry: &mut Mapping, server: &McpServerConfig) {
    entry.insert("name".into(), server.name.as_str().into());
    entry.insert("enabled".into(), server.enabled.into());
    entry.insert("run".into(), server.run.into());
    if server.run {
        set_or_remove(entry, "oci", filled(&server.image));
        entry.insert("port".into(), u64::from(server.port).into());
    } else {
        set_or_remove(entry, "url", filled(&server.url));
        set_or_remove(entry, "headers", headers_value(&server.headers));
    }
}

fn servers_of(root: &mut Mapping) -> Result<&mut Vec<Value>, String> {
    named_entries(root, SERVERS_KEY, MCP_FILE)
}

/// `mcp.yaml` with `server` added or replacing its entry. Every other entry
/// and key stays as written, so a `${VAR}` reference is never expanded here.
fn merge_server(mcp_yaml: Option<&str>, server: &McpServerConfig) -> Result<String, String> {
    let mut yaml = config::config_mapping(mcp_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    let entries = servers_of(root)?;
    let at = entries
        .iter()
        .position(|entry| text(entry, "name") == server.name);
    let mut entry = at
        .and_then(|index| entries[index].as_mapping().cloned())
        .unwrap_or_default();
    apply(&mut entry, server);
    match at {
        Some(index) => entries[index] = Value::Mapping(entry),
        None => entries.push(Value::Mapping(entry)),
    }
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn remove_server(mcp_yaml: Option<&str>, name: &str) -> Result<String, String> {
    remove_named(mcp_yaml, SERVERS_KEY, MCP_FILE, name)
}

fn merge_enabled(mcp_yaml: Option<&str>, enabled: bool) -> Result<String, String> {
    let mut yaml = config::config_mapping(mcp_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    root.insert("enabled".into(), enabled.into());
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// The variables a header may expand: those of the secrets and the API keys.
fn known_variables() -> BTreeSet<String> {
    crate::secrets::list_secrets()
        .into_iter()
        .map(|secret| secret.variable)
        .chain(config::auth_env().into_iter().map(|(key, _)| key))
        .collect()
}

/// Lists `server` on exactly the workspaces it goes to. Answers whether
/// daemon.yaml changed, which the daemon reads when it starts.
fn list_on_workspaces(name: &str, on: &[String], before: &[String]) -> Result<bool, String> {
    let (on_set, before_set): (BTreeSet<&String>, BTreeSet<&String>) =
        (on.iter().collect(), before.iter().collect());
    if on_set == before_set {
        return Ok(false);
    }
    config::update_file(&crate::daemon::config_path(), |existing| {
        workspace_settings::list_on(existing, WORKSPACE_KEY, name, on)
    })?;
    Ok(true)
}

fn read_settings() -> McpSettings {
    parse_settings(
        std::fs::read_to_string(mcp_path()).ok().as_deref(),
        &workspace_settings::workspace_configs(),
    )
}

/// Starts or stops a container server's shared container to match `enabled`,
/// like the app does for every one when it starts. Answers what failed.
async fn follow_container(name: &str, enabled: bool) -> Vec<String> {
    let action = if enabled { "start" } else { "stop" };
    crate::agent::run_infer_in(None, &["mcp", action, name])
        .await
        .err()
        .into_iter()
        .collect()
}

#[tauri::command]
pub(crate) fn mcp_settings() -> McpSettings {
    read_settings()
}

/// Saves `server` to mcp.yaml as written, and to the `mcp_servers` of the
/// workspaces it goes to. Answers whether daemon.yaml changed, beside the
/// container start or stop that failed.
#[tauri::command]
pub(crate) async fn save_mcp_server(
    server: McpServerConfig,
    is_new: bool,
) -> Result<(bool, Vec<String>), String> {
    let settings = read_settings();
    let workspaces = workspace_settings::workspace_configs();
    if let Some(problem) = problem(
        &server,
        is_new,
        &settings.servers,
        &workspaces,
        &known_variables(),
    ) {
        return Err(problem);
    }
    config::update_file(&mcp_path(), |existing| merge_server(existing, &server))?;
    let before = settings
        .servers
        .iter()
        .find(|other| other.name == server.name)
        .map(|other| other.workspaces.clone())
        .unwrap_or_default();
    let listed = list_on_workspaces(&server.name, &server.workspaces, &before)?;
    let failures = if server.run {
        follow_container(&server.name, server.enabled).await
    } else {
        Vec::new()
    };
    Ok((listed, failures))
}

/// Deletes the server `name` from mcp.yaml and from every workspace, stopping
/// its container first. Answers whether daemon.yaml changed, beside the
/// container stop that failed.
#[tauri::command]
pub(crate) async fn delete_mcp_server(name: String) -> Result<(bool, Vec<String>), String> {
    let stored = read_settings()
        .servers
        .into_iter()
        .find(|server| server.name == name)
        .ok_or_else(|| format!("There is no server named {name}."))?;
    let failures = if stored.run {
        follow_container(&name, false).await
    } else {
        Vec::new()
    };
    config::update_file(&mcp_path(), |existing| remove_server(existing, &name))?;
    let listed = list_on_workspaces(&name, &[], &stored.workspaces)?;
    Ok((listed, failures))
}

/// The global MCP switch: off, no chat on this computer connects to a server.
#[tauri::command]
pub(crate) fn set_mcp_enabled(enabled: bool) -> Result<(), String> {
    config::update_file(&mcp_path(), |existing| merge_enabled(existing, enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MCP_YAML: &str = "enabled: true\nconnection_timeout: 30\nservers:\n- name: agentmail\n  enabled: true\n  run: false\n  url: https://mcp.agentmail.to/mcp\n  include_tools:\n  - send_message\n  headers:\n    x-api-key: ${AGENTMAIL_API_KEY}\n    Authorization: Bearer ${GITHUB_TOKEN}\n    x-legacy: am_us_literal\n- name: filesystem\n  enabled: false\n  run: true\n  oci: mcp/filesystem:latest\n  port: 3000\n  volumes:\n  - /tmp:/data\n";

    fn kubernetes(name: &str, mcp_servers: &[&str]) -> WorkspaceConfig {
        let mut workspace = WorkspaceConfig::default();
        workspace.name = name.into();
        workspace.kind = KUBERNETES.into();
        workspace.mcp_servers = mcp_servers.iter().map(|s| s.to_string()).collect();
        workspace
    }

    fn header(name: &str, prefix: &str, variable: &str) -> McpHeader {
        McpHeader {
            name: name.into(),
            prefix: prefix.into(),
            variable: variable.into(),
            literal: false,
        }
    }

    fn hosted(name: &str, headers: Vec<McpHeader>, workspaces: &[&str]) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            enabled: true,
            url: "https://mcp.example.com/mcp".into(),
            headers,
            workspaces: workspaces.iter().map(|s| s.to_string()).collect(),
            ..McpServerConfig::default()
        }
    }

    #[test]
    fn servers_read_their_headers_as_references_and_their_workspaces_from_daemon_yaml() {
        let settings = parse_settings(Some(MCP_YAML), &[kubernetes("reviewer", &["agentmail"])]);
        assert!(settings.enabled);
        let agentmail = &settings.servers[0];
        assert_eq!(agentmail.workspaces, vec!["reviewer"]);
        assert_eq!(
            agentmail.headers,
            vec![
                header("x-api-key", "", "AGENTMAIL_API_KEY"),
                header("Authorization", "Bearer ", "GITHUB_TOKEN"),
                McpHeader {
                    name: "x-legacy".into(),
                    literal: true,
                    ..McpHeader::default()
                },
            ]
        );
        let filesystem = &settings.servers[1];
        assert!(filesystem.run && !filesystem.enabled);
        assert_eq!(
            (filesystem.image.as_str(), filesystem.port),
            ("mcp/filesystem:latest", 3000)
        );
        assert_eq!(parse_settings(None, &[]), McpSettings::default());
    }

    #[test]
    fn a_save_writes_references_and_keeps_every_key_the_tab_does_not_know() {
        let mut agentmail = parse_settings(Some(MCP_YAML), &[]).servers[0].clone();
        agentmail.headers.truncate(2);
        agentmail.enabled = false;
        let saved = merge_server(Some(MCP_YAML), &agentmail).unwrap();
        assert!(saved.contains("x-api-key: ${AGENTMAIL_API_KEY}"), "{saved}");
        assert!(
            saved.contains("Authorization: Bearer ${GITHUB_TOKEN}"),
            "{saved}"
        );
        assert!(!saved.contains("am_us_literal"), "{saved}");
        assert!(
            saved.contains("include_tools:\n  - send_message")
                || saved.contains("include_tools:\n- send_message"),
            "{saved}"
        );
        assert!(saved.contains("connection_timeout: 30"), "{saved}");
        let reread = parse_settings(Some(&saved), &[]);
        assert!(!reread.servers[0].enabled);
        assert_eq!(
            reread.servers[1],
            parse_settings(Some(MCP_YAML), &[]).servers[1]
        );

        let added = merge_server(
            None,
            &hosted(
                "github",
                vec![header("Authorization", "Bearer ", "GITHUB_TOKEN")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(
            parse_settings(Some(&added), &[]).servers[0].headers[0].variable,
            "GITHUB_TOKEN"
        );
        let removed = remove_server(Some(MCP_YAML), "agentmail").unwrap();
        assert_eq!(parse_settings(Some(&removed), &[]).servers.len(), 1);
        assert!(!parse_settings(Some(&merge_enabled(Some(MCP_YAML), false).unwrap()), &[]).enabled);
    }

    #[test]
    fn a_header_expands_a_secret_and_a_container_stays_on_this_computer() {
        let existing = parse_settings(Some(MCP_YAML), &[]).servers;
        let workspaces = [kubernetes("reviewer", &[]), {
            let mut remote = kubernetes("remote", &[]);
            remote.kind = "daemon".into();
            remote
        }];
        let variables = BTreeSet::from(["AGENTMAIL_API_KEY".to_string()]);
        let problem_of = |server: &McpServerConfig, is_new: bool| {
            problem(server, is_new, &existing, &workspaces, &variables).unwrap_or_default()
        };
        let mail = hosted(
            "mail",
            vec![header("x-api-key", "", "AGENTMAIL_API_KEY")],
            &["reviewer"],
        );
        assert_eq!(problem_of(&mail, true), "");
        let mut container = existing[1].clone();
        container.port = 0;
        let mut on_pods = existing[1].clone();
        on_pods.workspaces = vec!["reviewer".into()];
        let mut as_container = hosted("agentmail", Vec::new(), &[]);
        as_container.run = true;
        let mut no_url = hosted("plain", Vec::new(), &[]);
        no_url.url = "mcp.example.com".into();
        for (server, is_new, problem) in [
            (
                hosted("Mail", Vec::new(), &[]),
                true,
                "A server's name is lowercase letters, digits, '-' and '_'.",
            ),
            (
                hosted("agentmail", Vec::new(), &[]),
                true,
                "A server named agentmail already exists.",
            ),
            (
                hosted("ghost", Vec::new(), &[]),
                false,
                "There is no server named ghost.",
            ),
            (
                as_container,
                false,
                "agentmail cannot change between hosted and container.",
            ),
            (
                container,
                false,
                "A container server needs an image and the port it serves on.",
            ),
            (
                on_pods,
                false,
                "A container server runs on this computer only.",
            ),
            (
                no_url,
                true,
                "A hosted server needs an http:// or https:// URL.",
            ),
            (
                hosted(
                    "mail",
                    vec![
                        header("x-api-key", "", "AGENTMAIL_API_KEY"),
                        header("X-API-KEY", "", "AGENTMAIL_API_KEY"),
                    ],
                    &[],
                ),
                true,
                "Every header needs a name of its own.",
            ),
            (
                hosted("mail", vec![header("x-legacy", "am_us_literal", "")], &[]),
                true,
                "Header x-legacy needs a secret. Add one under Secrets, then pick it here.",
            ),
            (
                hosted("mail", Vec::new(), &["remote"]),
                true,
                "remote is not a Kubernetes workspace.",
            ),
        ] {
            assert_eq!(problem_of(&server, is_new), problem);
        }
    }
}
