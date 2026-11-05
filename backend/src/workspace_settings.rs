use serde_norway::{Mapping, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const WORKSPACES_KEY: &str = "workspaces";

/// A workspace entry of `daemon.yaml` as the Settings tab edits it. A key the
/// tab does not know, such as `mode` or `address`, stays in the file as it is.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceConfig {
    name: String,
    kind: String,
    context: String,
    namespace: String,
    image: String,
    storage: String,
    idle_ttl: String,
    retention: String,
    gateway_env: Vec<String>,
    forward_env: Vec<String>,
    env: BTreeMap<String, String>,
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn text(entry: &Value, key: &str) -> String {
    entry.get(key).and_then(scalar).unwrap_or_default()
}

fn names(entry: &Value, key: &str) -> Vec<String> {
    entry
        .get(key)
        .and_then(Value::as_sequence)
        .map(|items| items.iter().filter_map(scalar).collect())
        .unwrap_or_default()
}

fn environment(entry: &Value) -> BTreeMap<String, String> {
    entry
        .get("env")
        .and_then(Value::as_mapping)
        .map(|vars| {
            vars.iter()
                .filter_map(|(name, value)| Some((scalar(name)?, scalar(value)?)))
                .collect()
        })
        .unwrap_or_default()
}

fn config_of(entry: &Value) -> Option<WorkspaceConfig> {
    let name = text(entry, "name");
    if name.is_empty() {
        return None;
    }
    Some(WorkspaceConfig {
        name,
        kind: text(entry, "kind"),
        context: text(entry, "context"),
        namespace: text(entry, "namespace"),
        image: text(entry, "image"),
        storage: text(entry, "storage"),
        idle_ttl: text(entry, "idle_ttl"),
        retention: text(entry, "retention"),
        gateway_env: names(entry, "gateway_env"),
        forward_env: names(entry, "forward_env"),
        env: environment(entry),
    })
}

fn parse_configs(daemon_yaml: Option<&str>) -> Vec<WorkspaceConfig> {
    crate::config::config_mapping(daemon_yaml)
        .get(WORKSPACES_KEY)
        .and_then(Value::as_sequence)
        .map(|entries| entries.iter().filter_map(config_of).collect())
        .unwrap_or_default()
}

fn set_or_remove(entry: &mut Mapping, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            entry.insert(key.into(), value);
        }
        None => {
            entry.remove(key);
        }
    }
}

fn filled(text: &str) -> Option<Value> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.into())
}

fn listed(names: &[String]) -> Option<Value> {
    let names: Vec<Value> = names.iter().filter_map(|name| filled(name)).collect();
    (!names.is_empty()).then_some(Value::Sequence(names))
}

fn mapped(vars: &BTreeMap<String, String>) -> Option<Value> {
    let vars: Mapping = vars
        .iter()
        .filter(|(name, _)| !name.trim().is_empty())
        .map(|(name, value)| (name.trim().into(), value.as_str().into()))
        .collect();
    (!vars.is_empty()).then_some(Value::Mapping(vars))
}

fn apply(entry: &mut Mapping, config: &WorkspaceConfig) {
    set_or_remove(entry, "name", filled(&config.name));
    set_or_remove(entry, "kind", filled(&config.kind));
    set_or_remove(entry, "context", filled(&config.context));
    set_or_remove(entry, "namespace", filled(&config.namespace));
    set_or_remove(entry, "image", filled(&config.image));
    set_or_remove(entry, "storage", filled(&config.storage));
    set_or_remove(entry, "idle_ttl", filled(&config.idle_ttl));
    set_or_remove(entry, "retention", filled(&config.retention));
    set_or_remove(entry, "gateway_env", listed(&config.gateway_env));
    set_or_remove(entry, "forward_env", listed(&config.forward_env));
    set_or_remove(entry, "env", mapped(&config.env));
}

fn named(entry: &Value, name: &str) -> bool {
    text(entry, "name") == name
}

fn workspaces_of(root: &mut Mapping) -> Result<&mut Vec<Value>, String> {
    root.entry(WORKSPACES_KEY.into())
        .or_insert_with(|| Value::Sequence(Vec::new()))
        .as_sequence_mut()
        .ok_or_else(|| "workspaces in daemon.yaml is not a list".to_string())
}

/// `daemon.yaml` with `config` added, or replacing the entry `previous` names
/// when it was renamed. Every other entry and key stays as it is.
fn merge_workspace(
    daemon_yaml: Option<&str>,
    config: &WorkspaceConfig,
    previous: Option<&str>,
) -> Result<String, String> {
    let name = config.name.trim();
    if name.is_empty() {
        return Err("a workspace needs a name".into());
    }
    let mut yaml = crate::config::config_mapping(daemon_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    let entries = workspaces_of(root)?;
    let replaced =
        previous.and_then(|previous| entries.iter().position(|entry| named(entry, previous)));
    let taken = entries
        .iter()
        .enumerate()
        .any(|(index, entry)| named(entry, name) && Some(index) != replaced);
    if taken {
        return Err(format!("a workspace named {name} already exists"));
    }
    let mut entry = replaced
        .and_then(|index| entries[index].as_mapping().cloned())
        .unwrap_or_default();
    apply(&mut entry, config);
    match replaced {
        Some(index) => entries[index] = Value::Mapping(entry),
        None => entries.push(Value::Mapping(entry)),
    }
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn remove_workspace(daemon_yaml: Option<&str>, name: &str) -> Result<String, String> {
    let mut yaml = crate::config::config_mapping(daemon_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    workspaces_of(root)?.retain(|entry| !named(entry, name));
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn kubeconfig_path(home: &Path, kubeconfig_env: Option<&str>) -> PathBuf {
    kubeconfig_env
        .and_then(|paths| std::env::split_paths(paths).next())
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".kube").join("config"))
}

fn parse_contexts(kubeconfig: &str) -> Vec<String> {
    serde_norway::from_str::<Value>(kubeconfig)
        .ok()
        .as_ref()
        .and_then(|config| config.get("contexts"))
        .and_then(Value::as_sequence)
        .map(|contexts| {
            contexts
                .iter()
                .map(|context| text(context, "name"))
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The workspaces `daemon.yaml` configures, in file order.
#[tauri::command]
pub(crate) fn workspace_configs() -> Vec<WorkspaceConfig> {
    parse_configs(
        std::fs::read_to_string(crate::daemon::config_path())
            .ok()
            .as_deref(),
    )
}

/// Adds a workspace to `daemon.yaml`, or replaces the one `previous_name`
/// names. The daemon reads it on its next start.
#[tauri::command]
pub(crate) fn save_workspace(
    config: WorkspaceConfig,
    previous_name: Option<String>,
) -> Result<(), String> {
    crate::config::update_file(&crate::daemon::config_path(), |existing| {
        merge_workspace(existing, &config, previous_name.as_deref())
    })
}

#[tauri::command]
pub(crate) fn delete_workspace(name: String) -> Result<(), String> {
    crate::config::update_file(&crate::daemon::config_path(), |existing| {
        remove_workspace(existing, &name)
    })
}

/// The contexts of the user's kubeconfig, for the cluster a Kubernetes
/// workspace runs in.
#[tauri::command]
pub(crate) fn kube_contexts() -> Vec<String> {
    let kubeconfig_env = std::env::var("KUBECONFIG").ok();
    let path = kubeconfig_path(&crate::env::home_dir(), kubeconfig_env.as_deref());
    std::fs::read_to_string(path)
        .map(|kubeconfig| parse_contexts(&kubeconfig))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAEMON_YAML: &str = "binding:\n  enabled: true\n  token: secret\nworkspaces:\n  - name: laptop\n    kind: local\n  - name: build\n    kind: kubernetes\n    context: build-cluster\n    namespace: infer\n    mode: all-in-one\n    gateway_env:\n      - OPENAI_API_KEY\n    env:\n      INFER_AGENT_MODEL: openai/gpt-4o\n      INFER_COMPUTER_USE_ENABLED: true\n";

    fn kubernetes(name: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            name: name.into(),
            kind: "kubernetes".into(),
            context: "k3d-dev".into(),
            namespace: "agents".into(),
            ..WorkspaceConfig::default()
        }
    }

    #[test]
    fn configs_read_every_named_entry_with_env_values_as_text() {
        let configs = parse_configs(Some(DAEMON_YAML));
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].kind, "local");
        assert_eq!(configs[1].context, "build-cluster");
        assert_eq!(configs[1].gateway_env, vec!["OPENAI_API_KEY"]);
        assert_eq!(configs[1].env["INFER_COMPUTER_USE_ENABLED"], "true");
        assert!(parse_configs(None).is_empty());
        assert!(parse_configs(Some("workspaces: nope")).is_empty());
    }

    #[test]
    fn a_new_workspace_is_appended_and_the_rest_of_the_file_stays() {
        let merged = merge_workspace(Some(DAEMON_YAML), &kubernetes("dev"), None).unwrap();
        let configs = parse_configs(Some(&merged));
        assert_eq!(configs.len(), 3);
        assert_eq!(configs[2], kubernetes("dev"));
        assert!(merged.contains("token: secret"));
        let first = merge_workspace(None, &kubernetes("dev"), None).unwrap();
        assert_eq!(parse_configs(Some(&first)), vec![kubernetes("dev")]);
    }

    #[test]
    fn an_edit_replaces_its_entry_keeps_unknown_keys_and_drops_emptied_ones() {
        let merged =
            merge_workspace(Some(DAEMON_YAML), &kubernetes("build-2"), Some("build")).unwrap();
        let configs = parse_configs(Some(&merged));
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[1], kubernetes("build-2"));
        assert!(merged.contains("mode: all-in-one"));
        assert!(!merged.contains("gateway_env"));
        assert!(!merged.contains("INFER_AGENT_MODEL"));
    }

    #[test]
    fn a_name_is_required_and_unique() {
        assert!(merge_workspace(Some(DAEMON_YAML), &kubernetes(" "), None).is_err());
        assert!(merge_workspace(Some(DAEMON_YAML), &kubernetes("laptop"), None).is_err());
        assert!(merge_workspace(Some(DAEMON_YAML), &kubernetes("laptop"), Some("build")).is_err());
        assert!(merge_workspace(Some(DAEMON_YAML), &kubernetes("build"), Some("build")).is_ok());
    }

    #[test]
    fn removing_a_workspace_leaves_the_others() {
        let removed = remove_workspace(Some(DAEMON_YAML), "build").unwrap();
        let configs = parse_configs(Some(&removed));
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "laptop");
        assert!(removed.contains("token: secret"));
    }

    #[test]
    fn contexts_come_from_the_first_kubeconfig() {
        let kubeconfig = "contexts:\n  - name: k3d-dev\n    context: {}\n  - name: prod\n";
        assert_eq!(parse_contexts(kubeconfig), vec!["k3d-dev", "prod"]);
        assert!(parse_contexts("not: a kubeconfig").is_empty());
        let home = Path::new("/home/me");
        assert_eq!(
            kubeconfig_path(home, None),
            PathBuf::from("/home/me/.kube/config")
        );
        let listed = std::env::join_paths(["/a/config", "/b/config"]).unwrap();
        assert_eq!(
            kubeconfig_path(home, listed.to_str()),
            PathBuf::from("/a/config")
        );
    }
}
