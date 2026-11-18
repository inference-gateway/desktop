use crate::cli::compute::ClusterConfig;
use crate::cli::workspace_volumes::{WorkspaceVolume, listed_volumes, volumes_of};
use serde_norway::{Mapping, Value};
use std::collections::BTreeMap;

const WORKSPACES_KEY: &str = "workspaces";
pub(crate) const KUBERNETES: &str = "kubernetes";

/// A workspace entry of `daemon.yaml` as the Settings tab edits it. A key the
/// tab does not know, such as `mode` or `address`, stays in the file as it is.
/// `compute` names the `compute.yaml` cluster that gives the workspace its
/// kind, context and namespace (empty when it carries its own connection),
/// and the secrets and MCP servers are read-only views the tab never edits.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceConfig {
    pub(crate) name: String,
    pub(crate) compute: String,
    pub(crate) kind: String,
    pub(crate) context: String,
    pub(crate) namespace: String,
    pub(crate) image: String,
    pub(crate) storage: String,
    pub(crate) idle_ttl: String,
    pub(crate) retention: String,
    pub(crate) gateway_env: Vec<String>,
    pub(crate) forward_env: Vec<String>,
    pub(crate) forward_voice_samples: bool,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) user_data: String,
    pub(crate) volumes: Vec<WorkspaceVolume>,
    pub(crate) resources: WorkspaceResources,
    pub(crate) no_gpu: bool,
    #[serde(skip)]
    pub(crate) secrets: Vec<String>,
    #[serde(skip)]
    pub(crate) mcp_servers: Vec<String>,
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(crate) fn text(entry: &Value, key: &str) -> String {
    entry.get(key).and_then(scalar).unwrap_or_default()
}

pub(crate) fn names(entry: &Value, key: &str) -> Vec<String> {
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

const RESOURCES_KEY: &str = "resources";
const REQUESTS_KEY: &str = "requests";
const LIMITS_KEY: &str = "limits";
const CPU: &str = "cpu";
const MEMORY: &str = "memory";
const NO_GPU: &str = "0";

/// The CPU and memory each of a workspace's computers asks for and may use,
/// as Kubernetes quantities. An empty one leaves that amount to the cluster.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceResources {
    pub(crate) cpu_request: String,
    pub(crate) cpu_limit: String,
    pub(crate) memory_request: String,
    pub(crate) memory_limit: String,
}

impl WorkspaceResources {
    /// Each amount with the block of `resources` and the resource it is for.
    fn amounts(&self) -> [(&'static str, &'static str, &str); 4] {
        [
            (REQUESTS_KEY, CPU, &self.cpu_request),
            (LIMITS_KEY, CPU, &self.cpu_limit),
            (REQUESTS_KEY, MEMORY, &self.memory_request),
            (LIMITS_KEY, MEMORY, &self.memory_limit),
        ]
    }
}

fn amount(entry: &Value, block: &str, resource: &str) -> String {
    entry
        .get(RESOURCES_KEY)
        .and_then(|resources| resources.get(block))
        .map(|amounts| text(amounts, resource))
        .unwrap_or_default()
}

fn resources_of(entry: &Value) -> WorkspaceResources {
    WorkspaceResources {
        cpu_request: amount(entry, REQUESTS_KEY, CPU),
        cpu_limit: amount(entry, LIMITS_KEY, CPU),
        memory_request: amount(entry, REQUESTS_KEY, MEMORY),
        memory_limit: amount(entry, LIMITS_KEY, MEMORY),
    }
}

/// Whether the entry asks for none of the GPU, which is how it opts out of
/// the one its cluster gives by default.
fn opts_out_of_gpu(entry: &Value) -> bool {
    amount(entry, LIMITS_KEY, crate::cluster_gpu::RESOURCE) == NO_GPU
}

fn child<'a>(parent: &'a mut Mapping, key: &str) -> Option<&'a mut Mapping> {
    parent
        .entry(key.into())
        .or_insert_with(|| Value::Mapping(Mapping::new()))
        .as_mapping_mut()
}

/// Whether the workspace runs on the local cluster, the only one that gives
/// its computers a GPU to opt out of.
fn on_gpu_cluster(config: &WorkspaceConfig) -> bool {
    config.compute == crate::local_cluster::CONTEXT
}

/// Writes or clears one amount of the entry's `resources`. The amounts the
/// tab does not edit stay, and a block left empty goes.
fn set_amount(entry: &mut Mapping, block: &str, resource: &str, amount: Option<Value>) {
    let Some(resources) = child(entry, RESOURCES_KEY) else {
        return;
    };
    if let Some(amounts) = child(resources, block) {
        set_or_remove(amounts, resource, amount);
        if amounts.is_empty() {
            resources.remove(block);
        }
    }
    if resources.is_empty() {
        entry.remove(RESOURCES_KEY);
    }
}

fn set_resources(entry: &mut Mapping, config: &WorkspaceConfig) {
    for (block, resource, amount) in config.resources.amounts() {
        set_amount(entry, block, resource, filled(amount));
    }
    let no_gpu = (config.no_gpu && on_gpu_cluster(config)).then(|| NO_GPU.into());
    set_amount(entry, LIMITS_KEY, crate::cluster_gpu::RESOURCE, no_gpu);
}

/// A workspace entry as the tab shows it. One on a cluster reads that cluster's
/// kind, context, namespace and provider keys, so every check and clean-up
/// reaches the cluster the CLI resolves it to.
fn config_of(entry: &Value, clusters: &[ClusterConfig]) -> Option<WorkspaceConfig> {
    let name = text(entry, "name");
    if name.is_empty() {
        return None;
    }
    let config = WorkspaceConfig {
        name,
        compute: text(entry, "compute"),
        kind: text(entry, "kind"),
        context: text(entry, "context"),
        namespace: text(entry, "namespace"),
        image: text(entry, "image"),
        storage: text(entry, "storage"),
        idle_ttl: text(entry, "idle_ttl"),
        retention: text(entry, "retention"),
        gateway_env: names(entry, "gateway_env"),
        forward_env: names(entry, "forward_env"),
        forward_voice_samples: entry.get("forward_voice_samples").and_then(Value::as_bool)
            == Some(true),
        env: environment(entry),
        user_data: text(entry, "user_data"),
        volumes: volumes_of(entry),
        resources: resources_of(entry),
        no_gpu: opts_out_of_gpu(entry),
        secrets: names(entry, "secrets"),
        mcp_servers: names(entry, "mcp_servers"),
    };
    Some(on_cluster(config, clusters))
}

pub(crate) fn on_cluster(config: WorkspaceConfig, clusters: &[ClusterConfig]) -> WorkspaceConfig {
    match clusters
        .iter()
        .find(|cluster| !config.compute.is_empty() && cluster.name == config.compute)
    {
        Some(cluster) => WorkspaceConfig {
            kind: KUBERNETES.into(),
            context: cluster.context.clone(),
            namespace: cluster.namespace.clone(),
            gateway_env: cluster.gateway_env.clone(),
            ..config
        },
        None => config,
    }
}

pub(crate) fn parse_configs(
    daemon_yaml: Option<&str>,
    clusters: &[ClusterConfig],
) -> Vec<WorkspaceConfig> {
    crate::cli::config::config_mapping(daemon_yaml)
        .get(WORKSPACES_KEY)
        .and_then(Value::as_sequence)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| config_of(entry, clusters))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn set_or_remove(entry: &mut Mapping, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            entry.insert(key.into(), value);
        }
        None => {
            entry.remove(key);
        }
    }
}

pub(crate) fn filled(text: &str) -> Option<Value> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.into())
}

pub(crate) fn listed(names: &[String]) -> Option<Value> {
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

/// The keys a cluster gives a workspace on it, which the CLI refuses to see
/// repeated on the workspace.
const CLUSTER_KEYS: [&str; 6] = [
    "kind",
    "kubeconfig",
    "context",
    "namespace",
    "mode",
    "gateway_env",
];

fn apply(entry: &mut Mapping, config: &WorkspaceConfig) {
    set_or_remove(entry, "name", filled(&config.name));
    set_or_remove(entry, "compute", filled(&config.compute));
    set_or_remove(entry, "kind", filled(&config.kind));
    set_or_remove(entry, "context", filled(&config.context));
    set_or_remove(entry, "namespace", filled(&config.namespace));
    set_or_remove(entry, "image", filled(&config.image));
    set_or_remove(entry, "storage", filled(&config.storage));
    set_or_remove(entry, "idle_ttl", filled(&config.idle_ttl));
    set_or_remove(entry, "retention", filled(&config.retention));
    set_or_remove(entry, "gateway_env", listed(&config.gateway_env));
    set_or_remove(entry, "forward_env", listed(&config.forward_env));
    set_or_remove(
        entry,
        "forward_voice_samples",
        config.forward_voice_samples.then(|| true.into()),
    );
    set_or_remove(entry, "env", mapped(&config.env));
    set_or_remove(entry, "user_data", filled(&config.user_data));
    set_or_remove(entry, "volumes", listed_volumes(&config.volumes));
    set_resources(entry, config);
    if !config.compute.trim().is_empty() {
        for key in CLUSTER_KEYS {
            entry.remove(key);
        }
    }
}

fn named(entry: &Value, name: &str) -> bool {
    text(entry, "name") == name
}

/// The list of named entries under `key`, created empty when the file has none.
pub(crate) fn named_entries<'a>(
    root: &'a mut Mapping,
    key: &str,
    file: &str,
) -> Result<&'a mut Vec<Value>, String> {
    root.entry(key.into())
        .or_insert_with(|| Value::Sequence(Vec::new()))
        .as_sequence_mut()
        .ok_or_else(|| format!("{key} in {file} is not a list"))
}

/// `yaml` without the entry called `name` in the list under `key`. Every
/// other entry and key stays as written.
pub(crate) fn remove_named(
    yaml: Option<&str>,
    key: &str,
    file: &str,
    name: &str,
) -> Result<String, String> {
    let mut yaml = crate::cli::config::config_mapping(yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    named_entries(root, key, file)?.retain(|entry| !named(entry, name));
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn workspaces_of(root: &mut Mapping) -> Result<&mut Vec<Value>, String> {
    named_entries(root, WORKSPACES_KEY, "daemon.yaml")
}

/// `daemon.yaml` with `config` added, or replacing the entry `previous` names
/// when it was renamed. Every other entry and key stays as it is.
pub(crate) fn merge_workspace(
    daemon_yaml: Option<&str>,
    config: &WorkspaceConfig,
    previous: Option<&str>,
) -> Result<String, String> {
    let name = config.name.trim();
    if name.is_empty() {
        return Err("a workspace needs a name".into());
    }
    let mut yaml = crate::cli::config::config_mapping(daemon_yaml);
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

/// `daemon.yaml` with `item` listed under `key` by exactly the workspaces `on`
/// names, such as a Secret under `secrets`. A workspace whose list ends up
/// empty loses the key.
pub(crate) fn list_on(
    daemon_yaml: Option<&str>,
    key: &str,
    item: &str,
    on: &[String],
) -> Result<String, String> {
    let mut yaml = crate::cli::config::config_mapping(daemon_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    for entry in workspaces_of(root)?.iter_mut() {
        let wanted = on.contains(&text(entry, "name"));
        let mut items: Vec<String> = names(entry, key)
            .into_iter()
            .filter(|name| name != item)
            .collect();
        if wanted {
            items.push(item.to_owned());
        }
        if let Some(entry) = entry.as_mapping_mut() {
            set_or_remove(entry, key, listed(&items));
        }
    }
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

pub(crate) fn remove_workspace(daemon_yaml: Option<&str>, name: &str) -> Result<String, String> {
    remove_named(daemon_yaml, WORKSPACES_KEY, "daemon.yaml", name)
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
    fn a_secret_is_listed_on_exactly_the_workspaces_named() {
        let daemon_yaml = "workspaces:\n  - name: reviewer\n    kind: kubernetes\n    secrets:\n      - team-tokens\n  - name: writer\n    kind: kubernetes\n    secrets:\n      - infer-secret-inbox\n";
        let listed = list_on(
            Some(daemon_yaml),
            "secrets",
            "infer-secret-inbox",
            &["reviewer".into()],
        )
        .unwrap();
        let configs = parse_configs(Some(&listed), &[]);
        assert_eq!(
            configs[0].secrets,
            vec!["team-tokens", "infer-secret-inbox"]
        );
        assert!(configs[1].secrets.is_empty());
        assert!(!listed.contains("writer\n    kind: kubernetes\n    secrets"));
        let saved = merge_workspace(Some(&listed), &configs[0], Some("reviewer")).unwrap();
        assert_eq!(
            parse_configs(Some(&saved), &[])[0].secrets,
            configs[0].secrets
        );
    }

    #[test]
    fn configs_read_every_named_entry_with_env_values_as_text() {
        let configs = parse_configs(Some(DAEMON_YAML), &[]);
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].kind, "local");
        assert_eq!(configs[1].context, "build-cluster");
        assert_eq!(configs[1].gateway_env, vec!["OPENAI_API_KEY"]);
        assert_eq!(configs[1].env["INFER_COMPUTER_USE_ENABLED"], "true");
        assert!(parse_configs(None, &[]).is_empty());
        assert!(parse_configs(Some("workspaces: nope"), &[]).is_empty());
    }

    #[test]
    fn a_new_workspace_is_appended_and_the_rest_of_the_file_stays() {
        let merged = merge_workspace(Some(DAEMON_YAML), &kubernetes("dev"), None).unwrap();
        let configs = parse_configs(Some(&merged), &[]);
        assert_eq!(configs.len(), 3);
        assert_eq!(configs[2], kubernetes("dev"));
        assert!(merged.contains("token: secret"));
        let first = merge_workspace(None, &kubernetes("dev"), None).unwrap();
        assert_eq!(parse_configs(Some(&first), &[]), vec![kubernetes("dev")]);
    }

    #[test]
    fn an_edit_replaces_its_entry_keeps_unknown_keys_and_drops_emptied_ones() {
        let merged =
            merge_workspace(Some(DAEMON_YAML), &kubernetes("build-2"), Some("build")).unwrap();
        let configs = parse_configs(Some(&merged), &[]);
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[1], kubernetes("build-2"));
        assert!(merged.contains("mode: all-in-one"));
        assert!(!merged.contains("gateway_env"));
        assert!(!merged.contains("INFER_AGENT_MODEL"));
    }

    #[test]
    fn a_workspace_forwards_the_voice_samples_only_while_it_says_so() {
        let forwarding = WorkspaceConfig {
            forward_voice_samples: true,
            ..kubernetes("studio")
        };
        let written = merge_workspace(None, &forwarding, None).unwrap();
        assert!(written.contains("forward_voice_samples: true"));
        assert!(parse_configs(Some(&written), &[])[0].forward_voice_samples);

        let stopped =
            merge_workspace(Some(&written), &kubernetes("studio"), Some("studio")).unwrap();
        assert!(!stopped.contains("forward_voice_samples"));
        assert!(!parse_configs(Some(&stopped), &[])[0].forward_voice_samples);
    }

    #[test]
    fn a_workspace_keeps_its_cpu_and_memory_in_daemon_yaml_until_the_last_one_goes() {
        let daemon = "workspaces:\n  - name: studio\n    kind: kubernetes\n    resources:\n      requests:\n        cpu: 2\n        ephemeral-storage: 1Gi\n      limits:\n        memory: 8Gi\n";
        let studio = parse_configs(Some(daemon), &[]).remove(0);
        assert_eq!(studio.resources.cpu_request, "2");
        assert_eq!(studio.resources.memory_limit, "8Gi");
        assert_eq!(studio.resources.cpu_limit, "");
        let edited = WorkspaceConfig {
            resources: WorkspaceResources {
                cpu_limit: " 4 ".into(),
                memory_request: "4Gi".into(),
                ..WorkspaceResources::default()
            },
            ..studio
        };
        let saved = merge_workspace(Some(daemon), &edited, Some("studio")).unwrap();
        let entry = &serde_norway::from_str::<Value>(&saved).unwrap()["workspaces"][0];
        assert_eq!(entry["resources"]["limits"]["cpu"], "4");
        assert_eq!(entry["resources"]["requests"]["memory"], "4Gi");
        assert_eq!(entry["resources"]["requests"]["ephemeral-storage"], "1Gi");
        assert!(entry["resources"]["requests"].get("cpu").is_none());
        assert!(entry["resources"]["limits"].get("memory").is_none());
        let emptied = WorkspaceConfig {
            resources: WorkspaceResources::default(),
            ..edited
        };
        let plain = "workspaces:\n  - name: studio\n    kind: kubernetes\n    resources:\n      limits:\n        cpu: 4\n";
        let cleared = merge_workspace(Some(plain), &emptied, Some("studio")).unwrap();
        assert!(!cleared.contains("resources"), "{cleared}");
    }

    #[test]
    fn a_workspace_keeps_its_volumes_in_daemon_yaml_until_the_last_one_goes() {
        let yaml = "workspaces:\n  - name: desktop\n    kind: kubernetes\n    volumes:\n      - name: footage\n        claim: raw-footage\n        mount_path: /mnt/footage\n        read_only: true\n";
        let read = parse_configs(Some(yaml), &[]).remove(0);
        assert_eq!(read.volumes.len(), 1);
        assert_eq!(read.volumes[0].claim, "raw-footage");

        let saved = merge_workspace(Some(yaml), &read, Some("desktop")).unwrap();
        assert_eq!(parse_configs(Some(&saved), &[])[0], read);

        let emptied = WorkspaceConfig {
            volumes: Vec::new(),
            ..read
        };
        let saved = merge_workspace(Some(yaml), &emptied, Some("desktop")).unwrap();
        assert!(!saved.contains("volumes"));
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
        let configs = parse_configs(Some(&removed), &[]);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "laptop");
        assert!(removed.contains("token: secret"));
    }

    #[test]
    fn a_workspace_keeps_its_user_data_in_daemon_yaml() {
        let config = WorkspaceConfig {
            user_data: "#!/bin/sh\napt-get install -y gimp\n".into(),
            ..kubernetes("desktop")
        };
        let mut entry = Mapping::new();
        apply(&mut entry, &config);
        let read = config_of(&Value::Mapping(entry), &[]).unwrap();
        assert_eq!(read.user_data, "#!/bin/sh\napt-get install -y gimp");
    }

    #[test]
    fn a_workspace_on_a_cluster_reads_its_connection_and_saves_only_its_own_keys() {
        let cluster = ClusterConfig {
            name: "k3d".into(),
            context: "k3d-agents".into(),
            namespace: "agents".into(),
            gateway_env: vec!["ANTHROPIC_API_KEY".into()],
        };
        let daemon = "workspaces:\n  - name: reviewer\n    compute: k3d\n    image: infer:1\n";
        let read = parse_configs(Some(daemon), std::slice::from_ref(&cluster));
        assert_eq!(read[0].kind, "kubernetes");
        assert_eq!(read[0].context, "k3d-agents");
        assert_eq!(read[0].namespace, "agents");
        assert_eq!(read[0].gateway_env, ["ANTHROPIC_API_KEY"]);

        let legacy = "workspaces:\n  - name: reviewer\n    kind: kubernetes\n    context: old\n    namespace: infer\n    mode: all-in-one\n    gateway_env:\n      - OPENAI_API_KEY\n";
        let moved = WorkspaceConfig {
            compute: "k3d".into(),
            ..parse_configs(Some(legacy), &[])[0].clone()
        };
        let saved = merge_workspace(Some(legacy), &moved, Some("reviewer")).unwrap();
        for key in CLUSTER_KEYS {
            assert!(
                !saved.contains(&format!("{key}:")),
                "{key} stays on a workspace on a cluster: {saved}"
            );
        }
        assert!(saved.contains("compute: k3d"), "{saved}");
    }
}
