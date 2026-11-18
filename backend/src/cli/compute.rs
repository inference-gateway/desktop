use crate::cli::config::{filled, listed, named_entries, names, remove_named, set_or_remove, text};
use serde_norway::{Mapping, Value};
use std::path::PathBuf;

const CLUSTERS_KEY: &str = "clusters";
pub(crate) const COMPUTE_FILE: &str = "compute.yaml";
pub(crate) const COMPUTE_LABEL: &str = "inference-gateway.com/compute";
const LABEL_VALUE_MAX: usize = 63;

/// What every compute action answers on an `infer` that predates compute, so
/// the desktop never writes a `compute.yaml` or a `compute` key it cannot read.
pub(crate) const NEEDS_COMPUTE: &str =
    "This needs an infer CLI with compute support. Install updates under Settings > Updates";

/// A cluster of `compute.yaml` as the Compute tab edits it. A key the tab does
/// not know, such as `kubeconfig` or `mode`, stays in the file as it is.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ClusterConfig {
    pub(crate) name: String,
    pub(crate) context: String,
    pub(crate) namespace: String,
    pub(crate) gateway_env: Vec<String>,
}

/// The userspace `compute.yaml`, the only one the CLI reads.
pub(crate) fn compute_path() -> PathBuf {
    crate::env::infer_dir().join(COMPUTE_FILE)
}

fn cluster_of(entry: &Value) -> Option<ClusterConfig> {
    let name = text(entry, "name");
    if name.is_empty() {
        return None;
    }
    Some(ClusterConfig {
        name,
        context: text(entry, "context"),
        namespace: text(entry, "namespace"),
        gateway_env: names(entry, "gateway_env"),
    })
}

/// The clusters `compute.yaml` registers, in file order.
pub(crate) fn parse_clusters(compute_yaml: Option<&str>) -> Vec<ClusterConfig> {
    crate::cli::config::config_mapping(compute_yaml)
        .get(CLUSTERS_KEY)
        .and_then(Value::as_sequence)
        .map(|entries| entries.iter().filter_map(cluster_of).collect())
        .unwrap_or_default()
}

pub(crate) fn read_clusters() -> Vec<ClusterConfig> {
    parse_clusters(std::fs::read_to_string(compute_path()).ok().as_deref())
}

/// Whether `name` is a Kubernetes label value, which a cluster's name has to
/// be because it labels the Gateway and the sessions of its workspaces.
pub(crate) fn label_value(name: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    !name.is_empty()
        && name.len() <= LABEL_VALUE_MAX
        && edge(name.chars().next())
        && edge(name.chars().last())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn clusters_of(root: &mut Mapping) -> Result<&mut Vec<Value>, String> {
    named_entries(root, CLUSTERS_KEY, COMPUTE_FILE)
}

fn apply(entry: &mut Mapping, config: &ClusterConfig) {
    set_or_remove(entry, "name", filled(&config.name));
    set_or_remove(entry, "context", filled(&config.context));
    set_or_remove(entry, "namespace", filled(&config.namespace));
    set_or_remove(entry, "gateway_env", listed(&config.gateway_env));
}

/// `compute.yaml` with `config` added, or replacing the cluster `previous`
/// names. Machines, GPU settings and every other key stay as they are.
pub(crate) fn merge_cluster(
    compute_yaml: Option<&str>,
    config: &ClusterConfig,
    previous: Option<&str>,
) -> Result<String, String> {
    let name = config.name.trim();
    if !label_value(name) {
        return Err(format!(
            "A cluster name is up to {LABEL_VALUE_MAX} letters, digits, '-', '_' or '.', starting and ending with a letter or digit"
        ));
    }
    if config.namespace.trim().is_empty() {
        return Err("Name the namespace the cluster's sessions run in.".into());
    }
    let mut yaml = crate::cli::config::config_mapping(compute_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    let entries = clusters_of(root)?;
    let named = |entry: &Value, wanted: &str| text(entry, "name") == wanted;
    let replaced = previous.and_then(|previous| entries.iter().position(|e| named(e, previous)));
    let taken = entries
        .iter()
        .enumerate()
        .any(|(index, entry)| named(entry, name) && Some(index) != replaced);
    if taken {
        return Err(format!("a cluster named {name} already exists"));
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

fn remove_cluster(compute_yaml: Option<&str>, name: &str) -> Result<String, String> {
    remove_named(compute_yaml, CLUSTERS_KEY, COMPUTE_FILE, name)
}

/// The workspaces of `daemon.yaml` that run on the cluster `name`.
fn workspaces_on(daemon_yaml: Option<&str>, name: &str) -> Vec<String> {
    crate::cli::config::config_mapping(daemon_yaml)
        .get("workspaces")
        .and_then(Value::as_sequence)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| text(entry, "compute") == name)
                .map(|entry| text(entry, "name"))
                .collect()
        })
        .unwrap_or_default()
}

/// `daemon.yaml` with every workspace on the cluster `from` moved to `to`, so
/// renaming a cluster keeps its workspaces on it.
fn rename_workspaces(daemon_yaml: Option<&str>, from: &str, to: &str) -> Result<String, String> {
    let mut yaml = crate::cli::config::config_mapping(daemon_yaml);
    if let Some(entries) = yaml.get_mut("workspaces").and_then(Value::as_sequence_mut) {
        for entry in entries.iter_mut().filter_map(Value::as_mapping_mut) {
            if entry.get("compute").and_then(Value::as_str) == Some(from) {
                entry.insert("compute".into(), to.into());
            }
        }
    }
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// Whether the installed `infer` reads `compute.yaml`, told by whether it has
/// the compute commands. Mock mode always does.
pub(crate) fn compute_supported_blocking() -> bool {
    crate::env::mock_mode()
        || crate::cli::launch::infer_command(&crate::env::home_dir())
            .args(["compute", "clusters", "list"])
            .output()
            .is_ok_and(|output| output.status.success())
}

pub(crate) fn require_compute() -> Result<(), String> {
    if compute_supported_blocking() {
        Ok(())
    } else {
        Err(NEEDS_COMPUTE.into())
    }
}

/// Saves a cluster to `compute.yaml`, or replaces the one `previous_name`
/// names. A rename moves the workspaces on it along. The daemon reads both
/// files on its next start.
#[tauri::command]
pub(crate) async fn save_cluster(
    config: ClusterConfig,
    previous_name: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        require_compute()?;
        crate::cli::config::update_file(&compute_path(), |existing| {
            merge_cluster(existing, &config, previous_name.as_deref())
        })?;
        match previous_name {
            Some(previous) if previous != config.name.trim() => {
                crate::cli::config::update_file(&crate::env::daemon_config_path(), |existing| {
                    rename_workspaces(existing, &previous, config.name.trim())
                })
            }
            _ => Ok(()),
        }
    })
    .await
    .map_err(|e| format!("saving the cluster stopped: {e}"))?
}

/// The clusters of `compute.yaml`.
#[tauri::command]
pub(crate) fn cluster_configs() -> Vec<ClusterConfig> {
    read_clusters()
}

/// Removes a cluster no workspace runs on.
#[tauri::command]
pub(crate) fn delete_cluster(name: String) -> Result<(), String> {
    let daemon_yaml = std::fs::read_to_string(crate::env::daemon_config_path()).ok();
    let users = workspaces_on(daemon_yaml.as_deref(), &name);
    if !users.is_empty() {
        return Err(format!(
            "{name} runs workspace {} - move them to another cluster first",
            users.join(", ")
        ));
    }
    crate::cli::config::update_file(&compute_path(), |existing| remove_cluster(existing, &name))
}

/// Whether the installed `infer` reads `compute.yaml`.
#[tauri::command]
pub(crate) async fn compute_supported() -> bool {
    tokio::task::spawn_blocking(compute_supported_blocking)
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPUTE_YAML: &str = "clusters:\n  - name: k3d\n    context: k3d-agents\n    namespace: agents\n    mode: all-in-one\n    gateway_env:\n      - ANTHROPIC_API_KEY\nmachines:\n  - name: box\n    address: box:1\n    token: ${BOX_TOKEN}\n";

    fn cluster(name: &str) -> ClusterConfig {
        ClusterConfig {
            name: name.into(),
            context: format!("{name}-context"),
            namespace: "agents".into(),
            gateway_env: Vec::new(),
        }
    }

    #[test]
    fn clusters_read_by_name_and_keep_what_the_tab_does_not_know() {
        let clusters = parse_clusters(Some(COMPUTE_YAML));
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].context, "k3d-agents");
        assert_eq!(clusters[0].gateway_env, ["ANTHROPIC_API_KEY"]);

        let edited = ClusterConfig {
            namespace: "other".into(),
            gateway_env: Vec::new(),
            ..clusters[0].clone()
        };
        let merged = merge_cluster(Some(COMPUTE_YAML), &edited, Some("k3d")).unwrap();
        assert!(merged.contains("mode: all-in-one"), "{merged}");
        assert!(
            merged.contains("${BOX_TOKEN}"),
            "a machine's reference stays: {merged}"
        );
        assert!(
            !merged.contains("gateway_env"),
            "an emptied list is dropped: {merged}"
        );
        assert_eq!(parse_clusters(Some(&merged))[0].namespace, "other");
    }

    #[test]
    fn a_cluster_needs_a_label_safe_unique_name_and_a_namespace() {
        let taken = merge_cluster(Some(COMPUTE_YAML), &cluster("k3d"), None).unwrap_err();
        assert!(taken.contains("already exists"), "{taken}");
        for bad in ["has space", "-leading", "trailing-", ""] {
            assert!(merge_cluster(None, &cluster(bad), None).is_err(), "{bad:?}");
        }
        let without_namespace = ClusterConfig {
            namespace: String::new(),
            ..cluster("dev")
        };
        assert!(merge_cluster(None, &without_namespace, None).is_err());
        assert!(merge_cluster(None, &cluster("Prod.EU_1"), None).is_ok());
    }

    #[test]
    fn removing_a_cluster_leaves_the_rest() {
        let removed = remove_cluster(Some(COMPUTE_YAML), "k3d").unwrap();
        assert!(parse_clusters(Some(&removed)).is_empty());
        assert!(removed.contains("box"), "{removed}");
    }

    #[test]
    fn a_rename_moves_the_workspaces_on_the_cluster_along() {
        let daemon = "workspaces:\n  - name: reviewer\n    compute: k3d\n  - name: legacy\n    kind: kubernetes\n    context: k3d\n";
        assert_eq!(workspaces_on(Some(daemon), "k3d"), ["reviewer"]);
        let renamed = rename_workspaces(Some(daemon), "k3d", "agents").unwrap();
        assert_eq!(workspaces_on(Some(&renamed), "agents"), ["reviewer"]);
        assert!(
            renamed.contains("context: k3d"),
            "a workspace with its own connection is untouched: {renamed}"
        );
    }
}
