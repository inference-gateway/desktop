use crate::config;
use crate::env::{home_dir, infer_dir_in, mock_mode};
use crate::workspace_settings::{self, KUBERNETES, WorkspaceConfig};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_norway::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const SECRETS_FILE: &str = "secrets.yaml";
const MCP_FILE: &str = "mcp.yaml";

/// The name prefix of the Kubernetes Secret that carries one secret into the
/// namespace of every workspace it is assigned to.
const KUBE_SECRET_PREFIX: &str = "infer-secret-";
const FIELD_MANAGER: &str = "--field-manager=inference-gateway-desktop";
const MANAGED_BY: &str = "app.kubernetes.io/managed-by";
const DESKTOP: &str = "inference-gateway-desktop";
const LABEL_MAX_LEN: usize = 63;

/// Serializes every save and delete, each a read-modify-write of auth.yaml,
/// secrets.yaml and daemon.yaml, so two at once never drop each other's change.
static EDITS: Mutex<()> = Mutex::new(());

/// A secret as secrets.yaml keeps it. Its value lives in auth.yaml, and the
/// workspaces it is assigned to list its Kubernetes Secret in daemon.yaml.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct StoredSecret {
    label: String,
    variable: String,
    computer: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct SecretsFile {
    secrets: Vec<StoredSecret>,
}

/// A secret as Settings > Secrets shows and saves it: a label naming it, the
/// environment variable it becomes, and where it goes - this computer and the
/// Kubernetes workspaces it is assigned to. `mcp_servers` names the servers of
/// mcp.yaml whose headers expand the variable, and is only ever shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Secret {
    pub(crate) label: String,
    pub(crate) variable: String,
    pub(crate) computer: bool,
    pub(crate) workspaces: Vec<String>,
    pub(crate) mcp_servers: Vec<String>,
}

/// Where a Kubernetes Secret lives: one namespace of one cluster.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    context: String,
    namespace: String,
}

fn secrets_path(home: &Path) -> PathBuf {
    infer_dir_in(home).join(SECRETS_FILE)
}

fn parse_stored(text: Option<&str>) -> Vec<StoredSecret> {
    text.and_then(|text| serde_norway::from_str::<SecretsFile>(text).ok())
        .map(|file| file.secrets)
        .unwrap_or_default()
}

fn read_stored_in(home: &Path) -> Vec<StoredSecret> {
    parse_stored(std::fs::read_to_string(secrets_path(home)).ok().as_deref())
}

fn write_stored_in(home: &Path, secrets: Vec<StoredSecret>) -> Result<(), String> {
    let text = serde_norway::to_string(&SecretsFile { secrets }).map_err(|e| e.to_string())?;
    config::update_file(&secrets_path(home), |_| Ok(text))
}

/// The Kubernetes Secret of the secret `label`.
pub(crate) fn kube_secret_name(label: &str) -> String {
    format!("{KUBE_SECRET_PREFIX}{label}")
}

/// A label is a DNS label, so it can name the secret's Kubernetes Secret.
fn valid_label(label: &str) -> bool {
    (1..=LABEL_MAX_LEN).contains(&label.len())
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

fn valid_variable(variable: &str) -> bool {
    variable
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && variable
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Why `secret` cannot be saved beside the `stored` ones, or None when it can.
/// No target may get two secrets for one variable, and an API key's variable
/// stays the API key's.
fn problem(
    secret: &Secret,
    is_new: bool,
    stored: &[StoredSecret],
    workspaces: &[WorkspaceConfig],
    api_keys: &[String],
) -> Option<String> {
    let (label, variable) = (secret.label.as_str(), secret.variable.as_str());
    if !valid_label(label) {
        return Some("A secret's label is lowercase letters, digits and '-', starting and ending with a letter or digit.".into());
    }
    if !valid_variable(variable) {
        return Some(format!(
            "{variable:?} is not an environment variable name: letters, digits and '_', not starting with a digit."
        ));
    }
    let exists = stored.iter().any(|other| other.label == label);
    if is_new && exists {
        return Some(format!("A secret labelled {label} already exists."));
    }
    if !is_new && !exists {
        return Some(format!("There is no secret labelled {label}."));
    }
    if api_keys.iter().any(|key| key == variable) {
        return Some(format!("{variable} is an API key. Set it under API Keys."));
    }
    if let Some(name) = secret.workspaces.iter().find(|name| {
        !workspaces
            .iter()
            .any(|w| &w.name == *name && w.kind == KUBERNETES)
    }) {
        return Some(format!("{name} is not a Kubernetes workspace."));
    }
    for other in stored
        .iter()
        .filter(|other| other.label != label && other.variable == variable)
    {
        if secret.computer && other.computer {
            return Some(format!(
                "This computer already gets {variable} from the secret {}.",
                other.label
            ));
        }
        let theirs = kube_secret_name(&other.label);
        if let Some(workspace) = workspaces
            .iter()
            .find(|w| secret.workspaces.contains(&w.name) && w.secrets.contains(&theirs))
        {
            return Some(format!(
                "Workspace {} already gets {variable} from the secret {}.",
                workspace.name, other.label
            ));
        }
    }
    None
}

/// Whether `text` expands `variable` the way the CLI's os.ExpandEnv does,
/// as `${NAME}` or as `$NAME` ending where the name does.
fn expands(text: &str, variable: &str) -> bool {
    let bare = format!("${variable}");
    text.contains(&format!("${{{variable}}}"))
        || text.match_indices(&bare).any(|(at, _)| {
            !text[at + bare.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// The servers of `mcp_yaml` with a header that expands `variable`.
fn mcp_servers_using(mcp_yaml: Option<&str>, variable: &str) -> Vec<String> {
    let uses = |server: &&Value| {
        server
            .get("headers")
            .and_then(Value::as_mapping)
            .is_some_and(|headers| {
                headers
                    .values()
                    .filter_map(Value::as_str)
                    .any(|header| expands(header, variable))
            })
    };
    config::config_mapping(mcp_yaml)
        .get("servers")
        .and_then(Value::as_sequence)
        .map(|servers| {
            servers
                .iter()
                .filter(uses)
                .filter_map(|server| server.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn view(stored: StoredSecret, workspaces: &[WorkspaceConfig], mcp_yaml: Option<&str>) -> Secret {
    let theirs = kube_secret_name(&stored.label);
    Secret {
        workspaces: workspaces
            .iter()
            .filter(|w| w.secrets.contains(&theirs))
            .map(|w| w.name.clone())
            .collect(),
        mcp_servers: mcp_servers_using(mcp_yaml, &stored.variable),
        label: stored.label,
        variable: stored.variable,
        computer: stored.computer,
    }
}

/// The secrets assigned to this computer, as (variable, value) pairs for every
/// infer process the desktop starts. One without a value is left out.
pub(crate) fn computer_env() -> Vec<(String, String)> {
    computer_env_in(&home_dir())
}

fn computer_env_in(home: &Path) -> Vec<(String, String)> {
    read_stored_in(home)
        .into_iter()
        .filter(|secret| secret.computer)
        .map(|secret| {
            let value = config::secret_value_in(home, &secret.label);
            (secret.variable, value)
        })
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

fn place_of(workspace: &WorkspaceConfig) -> Place {
    Place {
        context: workspace.context.clone(),
        namespace: workspace.namespace.clone(),
    }
}

fn places(workspaces: &[WorkspaceConfig], names: &BTreeSet<String>) -> BTreeSet<Place> {
    workspaces
        .iter()
        .filter(|w| names.contains(&w.name))
        .map(place_of)
        .collect()
}

fn describe(place: &Place) -> String {
    match place.namespace.as_str() {
        "" => format!("cluster {}", place.context),
        namespace => format!("cluster {}, namespace {namespace}", place.context),
    }
}

/// Writes the secret into the namespace of every workspace `wanted` names and
/// answers the workspaces to list it on - each one reached, and each one that
/// listed it before, whose namespace keeps its previous value - beside what failed.
fn reach(
    workspaces: &[WorkspaceConfig],
    before: &BTreeSet<String>,
    wanted: &BTreeSet<String>,
    mut write: impl FnMut(&Place) -> Result<(), String>,
) -> (Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
    let reached: BTreeSet<Place> = places(workspaces, wanted)
        .into_iter()
        .filter(|place| match write(place) {
            Ok(()) => true,
            Err(e) => {
                failures.push(format!("{}: {e}", describe(place)));
                false
            }
        })
        .collect();
    let listed = workspaces
        .iter()
        .filter(|w| wanted.contains(&w.name))
        .filter(|w| reached.contains(&place_of(w)) || before.contains(&w.name))
        .map(|w| w.name.clone())
        .collect();
    (listed, failures)
}

/// The namespaces the secret leaves: those of the workspaces that listed it
/// before and no longer do, unless a workspace still listing it shares one.
fn stale(
    workspaces: &[WorkspaceConfig],
    before: &BTreeSet<String>,
    listed: &BTreeSet<String>,
) -> BTreeSet<Place> {
    let dropped = before.difference(listed).cloned().collect();
    places(workspaces, &dropped)
        .difference(&places(workspaces, listed))
        .cloned()
        .collect()
}

/// The secret's Kubernetes Secret, its one key the variable. The value is
/// base64 in `data`, since server-side apply drops a key only it set there.
fn manifest(label: &str, variable: &str, value: &str) -> String {
    let data = serde_json::Map::from_iter([(
        variable.to_owned(),
        base64::engine::general_purpose::STANDARD
            .encode(value)
            .into(),
    )]);
    serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "type": "Opaque",
        "metadata": {
            "name": kube_secret_name(label),
            "labels": { MANAGED_BY: DESKTOP },
        },
        "data": data,
    })
    .to_string()
}

fn in_namespace<'a>(place: &'a Place, args: &[&'a str]) -> Vec<&'a str> {
    let mut all = args.to_vec();
    if !place.namespace.is_empty() {
        all.extend(["-n", place.namespace.as_str()]);
    }
    all
}

fn write_kube_secret(place: &Place, manifest: &str) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }
    let args = [
        "apply",
        "--server-side",
        "--force-conflicts",
        FIELD_MANAGER,
        "-f",
        "-",
    ];
    workspace_settings::kubectl_with_input(&place.context, &in_namespace(place, &args), manifest)
        .map(drop)
}

fn delete_kube_secret(place: &Place, name: &str) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }
    let args = ["delete", "secret", name, "--ignore-not-found"];
    workspace_settings::kubectl(&place.context, &in_namespace(place, &args)).map(drop)
}

/// Carries the secret `label` into the namespaces of the Kubernetes workspaces
/// `wanted` names, lists it in daemon.yaml on the ones it reached, then takes
/// it out of the namespaces it left. Answers what failed - the rest still applies.
fn sync(
    label: &str,
    variable: &str,
    value: &str,
    wanted: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    let workspaces: Vec<WorkspaceConfig> = workspace_settings::workspace_configs()
        .into_iter()
        .filter(|w| w.kind == KUBERNETES)
        .collect();
    let name = kube_secret_name(label);
    let before: BTreeSet<String> = workspaces
        .iter()
        .filter(|w| w.secrets.contains(&name))
        .map(|w| w.name.clone())
        .collect();
    let manifest = manifest(label, variable, value);
    let (listed, mut failures) = reach(&workspaces, &before, wanted, |place| {
        write_kube_secret(place, &manifest)
    });
    let listed_set: BTreeSet<String> = listed.iter().cloned().collect();
    if listed_set != before {
        config::update_file(&crate::daemon::config_path(), |existing| {
            workspace_settings::list_on(existing, "secrets", &name, &listed)
        })?;
    }
    for place in stale(&workspaces, &before, &listed_set) {
        if let Err(e) = delete_kube_secret(&place, &name) {
            failures.push(format!("{}: {e}", describe(&place)));
        }
    }
    Ok(failures)
}

/// Why `secret` cannot be saved now, read against what is stored. The agent
/// editor asks before it mints a key it would otherwise have nowhere to keep.
pub(crate) fn check(secret: &Secret, is_new: bool) -> Result<(), String> {
    let api_keys: Vec<String> = config::auth_env().into_iter().map(|(key, _)| key).collect();
    match problem(
        secret,
        is_new,
        &read_stored_in(&home_dir()),
        &workspace_settings::workspace_configs(),
        &api_keys,
    ) {
        Some(problem) => Err(problem),
        None => Ok(()),
    }
}

/// Saves `secret` with `value`, a blank one keeping the stored value, and
/// carries it to where it is assigned. Answers the clusters it could not reach.
pub(crate) fn save_blocking(
    secret: Secret,
    value: String,
    is_new: bool,
) -> Result<Vec<String>, String> {
    let _edits = EDITS.lock().unwrap_or_else(PoisonError::into_inner);
    check(&secret, is_new)?;
    let home = home_dir();
    let value = match value.trim() {
        "" => config::secret_value_in(&home, &secret.label),
        typed => typed.to_owned(),
    };
    if value.is_empty() {
        return Err("Type the secret's value.".into());
    }
    config::set_secret_value_in(&home, &secret.label, &value)?;
    let entry = StoredSecret {
        label: secret.label.clone(),
        variable: secret.variable.clone(),
        computer: secret.computer,
    };
    let mut stored = read_stored_in(&home);
    match stored.iter_mut().find(|other| other.label == entry.label) {
        Some(other) => *other = entry,
        None => stored.push(entry),
    }
    write_stored_in(&home, stored)?;
    sync(
        &secret.label,
        &secret.variable,
        &value,
        &secret.workspaces.iter().cloned().collect(),
    )
}

fn delete_blocking(label: &str) -> Result<Vec<String>, String> {
    let _edits = EDITS.lock().unwrap_or_else(PoisonError::into_inner);
    let home = home_dir();
    let mut stored = read_stored_in(&home);
    let at = stored
        .iter()
        .position(|secret| secret.label == label)
        .ok_or_else(|| format!("There is no secret labelled {label}."))?;
    let failures = sync(label, &stored[at].variable, "", &BTreeSet::new())?;
    stored.remove(at);
    write_stored_in(&home, stored)?;
    config::set_secret_value_in(&home, label, "")?;
    Ok(failures)
}

/// Every secret with where it is assigned. Values never leave the backend.
#[tauri::command]
pub(crate) fn list_secrets() -> Vec<Secret> {
    let home = home_dir();
    let workspaces = workspace_settings::workspace_configs();
    let mcp_yaml = std::fs::read_to_string(infer_dir_in(&home).join(MCP_FILE)).ok();
    read_stored_in(&home)
        .into_iter()
        .map(|secret| view(secret, &workspaces, mcp_yaml.as_deref()))
        .collect()
}

#[tauri::command]
pub(crate) async fn save_secret(
    secret: Secret,
    value: String,
    is_new: bool,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || save_blocking(secret, value, is_new))
        .await
        .map_err(|e| format!("secret task failed: {e}"))?
}

/// Deletes the secret `label` from auth.yaml and from every workspace and
/// cluster it was assigned to. Answers the clusters it could not reach.
#[tauri::command]
pub(crate) async fn delete_secret(label: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || delete_blocking(&label))
        .await
        .map_err(|e| format!("secret task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn workspace(name: &str, kind: &str, namespace: &str, secrets: &[&str]) -> WorkspaceConfig {
        let mut workspace = WorkspaceConfig::default();
        workspace.name = name.into();
        workspace.kind = kind.into();
        workspace.context = "k3d-agents".into();
        workspace.namespace = namespace.into();
        workspace.secrets = secrets.iter().map(|s| s.to_string()).collect();
        workspace
    }

    fn stored(label: &str, variable: &str, computer: bool) -> StoredSecret {
        StoredSecret {
            label: label.into(),
            variable: variable.into(),
            computer,
        }
    }

    fn secret(label: &str, variable: &str, computer: bool, workspaces: &[&str]) -> Secret {
        Secret {
            label: label.into(),
            variable: variable.into(),
            computer,
            workspaces: workspaces.iter().map(|s| s.to_string()).collect(),
            mcp_servers: Vec::new(),
        }
    }

    fn names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_label_names_a_kubernetes_secret_and_a_variable_an_environment_variable() {
        let long = "a".repeat(LABEL_MAX_LEN + 1);
        for (label, valid) in [
            ("code-reviewer-inbox", true),
            ("a1", true),
            ("Upper", false),
            ("-lead", false),
            ("trail-", false),
            ("has space", false),
            ("snake_case", false),
            ("", false),
            (long.as_str(), false),
        ] {
            assert_eq!(valid_label(label), valid, "{label}");
        }
        for (variable, valid) in [
            ("AGENTMAIL_API_KEY", true),
            ("_private", true),
            ("9LIVES", false),
            ("KEBAB-CASE", false),
            ("", false),
        ] {
            assert_eq!(valid_variable(variable), valid, "{variable}");
        }
    }

    #[test]
    fn no_target_gets_two_secrets_for_one_variable() {
        let stored = [stored("reviewer-inbox", "AGENTMAIL_API_KEY", true)];
        let workspaces = [
            workspace(
                "reviewer",
                KUBERNETES,
                "agents",
                &["infer-secret-reviewer-inbox"],
            ),
            workspace("writer", KUBERNETES, "agents", &[]),
            workspace("remote", "daemon", "", &[]),
        ];
        let api_keys = ["OPENAI_API_KEY".to_string()];
        let problem_of = |secret: &Secret, is_new: bool| {
            problem(secret, is_new, &stored, &workspaces, &api_keys).unwrap_or_default()
        };
        let writer = secret("writer-inbox", "AGENTMAIL_API_KEY", false, &["writer"]);
        assert_eq!(problem_of(&writer, true), "");
        let reviewer = secret("reviewer-inbox", "AGENTMAIL_API_KEY", true, &["reviewer"]);
        assert_eq!(problem_of(&reviewer, false), "");
        for (secret, is_new, problem) in [
            (
                secret("writer-inbox", "AGENTMAIL_API_KEY", true, &[]),
                true,
                "This computer already gets AGENTMAIL_API_KEY from the secret reviewer-inbox.",
            ),
            (
                secret("writer-inbox", "AGENTMAIL_API_KEY", false, &["reviewer"]),
                true,
                "Workspace reviewer already gets AGENTMAIL_API_KEY from the secret reviewer-inbox.",
            ),
            (
                secret("reviewer-inbox", "GITHUB_TOKEN", false, &[]),
                true,
                "A secret labelled reviewer-inbox already exists.",
            ),
            (
                secret("ghost", "GITHUB_TOKEN", false, &[]),
                false,
                "There is no secret labelled ghost.",
            ),
            (
                secret("openai", "OPENAI_API_KEY", false, &[]),
                true,
                "OPENAI_API_KEY is an API key. Set it under API Keys.",
            ),
            (
                secret("token", "GITHUB_TOKEN", false, &["remote"]),
                true,
                "remote is not a Kubernetes workspace.",
            ),
        ] {
            assert_eq!(problem_of(&secret, is_new), problem);
        }
    }

    #[test]
    fn an_mcp_server_uses_the_variables_its_headers_expand() {
        let mcp_yaml = "servers:\n- name: agentmail\n  url: https://mcp.agentmail.to/mcp\n  headers:\n    x-api-key: ${AGENTMAIL_API_KEY}\n- name: github\n  headers:\n    Authorization: Bearer $GITHUB_TOKEN\n- name: other\n  headers:\n    x-api-key: $AGENTMAIL_API_KEY_OTHER\n- name: local\n  run: true\n";
        assert_eq!(
            mcp_servers_using(Some(mcp_yaml), "AGENTMAIL_API_KEY"),
            vec!["agentmail"]
        );
        assert_eq!(
            mcp_servers_using(Some(mcp_yaml), "GITHUB_TOKEN"),
            vec!["github"]
        );
        assert!(mcp_servers_using(Some(mcp_yaml), "GITHUB").is_empty());
        assert!(mcp_servers_using(None, "GITHUB_TOKEN").is_empty());
    }

    #[test]
    fn a_secret_is_listed_where_it_reached_and_where_it_already_was() {
        let workspaces = [
            workspace("listed", KUBERNETES, "down", &["infer-secret-token"]),
            workspace("new-down", KUBERNETES, "also-down", &[]),
            workspace("new-up", KUBERNETES, "up", &[]),
        ];
        let (listed, failures) = reach(
            &workspaces,
            &names(&["listed"]),
            &names(&["listed", "new-down", "new-up"]),
            |place| match place.namespace.as_str() {
                "up" => Ok(()),
                _ => Err("unreachable".into()),
            },
        );
        assert_eq!(listed, vec!["listed", "new-up"]);
        assert_eq!(
            failures,
            vec![
                "cluster k3d-agents, namespace also-down: unreachable",
                "cluster k3d-agents, namespace down: unreachable",
            ]
        );
    }

    #[test]
    fn a_namespace_keeps_the_secret_while_a_workspace_in_it_lists_it() {
        let workspaces = [
            workspace("a", KUBERNETES, "shared", &[]),
            workspace("b", KUBERNETES, "shared", &[]),
            workspace("c", KUBERNETES, "own", &[]),
        ];
        assert_eq!(
            stale(&workspaces, &names(&["a", "b", "c"]), &names(&["b"])),
            BTreeSet::from([Place {
                context: "k3d-agents".into(),
                namespace: "own".into(),
            }])
        );
    }

    #[test]
    fn the_manifest_holds_the_value_only_in_base64() {
        let manifest = manifest("reviewer-inbox", "AGENTMAIL_API_KEY", "am_secret");
        let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(parsed["metadata"]["name"], "infer-secret-reviewer-inbox");
        assert_eq!(parsed["metadata"]["labels"][MANAGED_BY], DESKTOP);
        assert_eq!(parsed["data"]["AGENTMAIL_API_KEY"], "YW1fc2VjcmV0");
        assert!(!manifest.contains("am_secret"));
    }

    #[test]
    fn only_secrets_assigned_to_this_computer_reach_its_processes() {
        let home = TempDir::new("secrets-env");
        let secrets = vec![
            stored("github", "GITHUB_TOKEN", true),
            stored("reviewer-inbox", "AGENTMAIL_API_KEY", false),
            stored("blank", "BLANK", true),
        ];
        write_stored_in(&home, secrets.clone()).unwrap();
        assert_eq!(read_stored_in(&home), secrets);
        config::set_secret_value_in(&home, "github", "ghp_token").unwrap();
        config::set_secret_value_in(&home, "reviewer-inbox", "am_inbox").unwrap();
        assert_eq!(
            computer_env_in(&home),
            vec![("GITHUB_TOKEN".to_string(), "ghp_token".to_string())]
        );
        let file = std::fs::read_to_string(secrets_path(&home)).unwrap();
        assert!(!file.contains("ghp_token") && !file.contains("am_inbox"));
    }
}
