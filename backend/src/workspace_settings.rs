use crate::compute_settings::ClusterConfig;
use serde_norway::{Mapping, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;

const WORKSPACES_KEY: &str = "workspaces";
pub(crate) const KUBERNETES: &str = "kubernetes";
const CLI_IMAGE: &str = "ghcr.io/inference-gateway/cli";
pub(crate) const DETECTED_NAMESPACE: &str = "infer-workspaces";
const DETECTED_STORAGE: &str = "5Gi";
const KUBECTL_TIMEOUT: &str = "--request-timeout=5s";
const MANAGED_LABEL: &str = "inference-gateway.com/managed";
const REACHABLE: &str = "Cluster reachable";
const ORCHESTRATORS: &str = "orchestrators.core.inference-gateway.com";
const WORKSPACE_LABEL: &str = "inference-gateway.com/workspace";
const READY_STATUSES: &str =
    r#"jsonpath={range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}"#;
const LOCAL_WORKSPACE: &str = "ubuntu-desktop";
/// A local cluster's sessions run on a pod of their own, not the user's
/// machine, so its agents use the computer and edit files without asking.
const COMPUTER_ENV: [(&str, &str); 5] = [
    ("INFER_COMPUTER_USE_ENABLED", "true"),
    ("INFER_COMPUTER_USE_APPROVAL", "never"),
    ("INFER_TOOLS_BASH_ALLOW_APPEND", ".*"),
    ("INFER_TOOLS_WRITE_REQUIRE_APPROVAL", "false"),
    ("INFER_TOOLS_EDIT_REQUIRE_APPROVAL", "false"),
];

/// Something a cluster must have installed before sessions can run in it,
/// told by the CRDs it brings.
struct Requirement {
    name: &'static str,
    crds: &'static [&'static str],
    fix: &'static str,
}

const REQUIREMENTS: [Requirement; 3] = [
    Requirement {
        name: "agent-sandbox",
        crds: &["sandboxes.agents.x-k8s.io"],
        fix: "Install agent-sandbox before the operator, which looks for it only when it starts.",
    },
    Requirement {
        name: "Gateway API",
        crds: &["gateways.gateway.networking.k8s.io"],
        fix: "Install the Gateway API CRDs the operator requires.",
    },
    Requirement {
        name: "Inference Gateway operator",
        crds: &[
            "orchestrators.core.inference-gateway.com",
            "gateways.core.inference-gateway.com",
        ],
        fix: "Install the Inference Gateway operator v0.29.0 or later.",
    },
];

/// One thing a Kubernetes workspace needs from its cluster, whether the
/// cluster has it and, when it does not, what to do about it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Prerequisite {
    name: String,
    met: bool,
    detail: String,
}

/// What a Kubernetes workspace's cluster looks like right now: what it has of
/// what sessions need, and how many sessions it holds. The counts are absent
/// when the cluster cannot list sessions.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct ClusterCheck {
    prerequisites: Vec<Prerequisite>,
    running: Option<usize>,
    suspended: Option<usize>,
}

fn prerequisite(name: &str, met: bool, detail: &str) -> Prerequisite {
    Prerequisite {
        name: name.into(),
        met,
        detail: if met { String::new() } else { detail.into() },
    }
}

/// A workspace entry of `daemon.yaml` as the Settings tab edits it. A key the
/// tab does not know, such as `mode` or `address`, stays in the file as it is.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceConfig {
    pub(crate) name: String,
    /// The cluster of `compute.yaml` the workspace runs on, which then gives it
    /// its kind, context, namespace and provider keys. Empty when the workspace
    /// carries its own connection.
    pub(crate) compute: String,
    pub(crate) kind: String,
    pub(crate) context: String,
    pub(crate) namespace: String,
    image: String,
    storage: String,
    idle_ttl: String,
    retention: String,
    gateway_env: Vec<String>,
    forward_env: Vec<String>,
    env: BTreeMap<String, String>,
    /// The Kubernetes Secrets the pod daemon reads, which Settings > Secrets
    /// lists and the tab never edits.
    #[serde(skip)]
    pub(crate) secrets: Vec<String>,
    /// The servers of `mcp.yaml` the pod gets, which Settings > MCPs lists.
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
        env: environment(entry),
        secrets: names(entry, "secrets"),
        mcp_servers: names(entry, "mcp_servers"),
    };
    Some(on_cluster(config, clusters))
}

fn on_cluster(config: WorkspaceConfig, clusters: &[ClusterConfig]) -> WorkspaceConfig {
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

fn parse_configs(daemon_yaml: Option<&str>, clusters: &[ClusterConfig]) -> Vec<WorkspaceConfig> {
    crate::config::config_mapping(daemon_yaml)
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
    set_or_remove(entry, "env", mapped(&config.env));
    if !config.compute.trim().is_empty() {
        for key in CLUSTER_KEYS {
            entry.remove(key);
        }
    }
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

/// `daemon.yaml` with `item` listed under `key` by exactly the workspaces `on`
/// names, such as a Secret under `secrets`. A workspace whose list ends up
/// empty loses the key.
pub(crate) fn list_on(
    daemon_yaml: Option<&str>,
    key: &str,
    item: &str,
    on: &[String],
) -> Result<String, String> {
    let mut yaml = crate::config::config_mapping(daemon_yaml);
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

fn remove_workspace(daemon_yaml: Option<&str>, name: &str) -> Result<String, String> {
    let mut yaml = crate::config::config_mapping(daemon_yaml);
    let root = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    workspaces_of(root)?.retain(|entry| !named(entry, name));
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

fn crd_names(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .filter_map(|line| line.trim().rsplit('/').next())
        .filter(|name| !name.is_empty())
        .collect()
}

fn installed_prerequisites(crds: &[&str]) -> Vec<Prerequisite> {
    REQUIREMENTS
        .iter()
        .map(|requirement| {
            let met = requirement.crds.iter().all(|crd| crds.contains(crd));
            prerequisite(requirement.name, met, requirement.fix)
        })
        .collect()
}

/// The namespace's prerequisites from its labels as kubectl prints them, or
/// from the error when it has no such namespace.
fn namespace_prerequisites(namespace: &str, labels: Result<&str, &str>) -> Vec<Prerequisite> {
    let exists = format!("Namespace {namespace}");
    let Ok(labels) = labels else {
        let create = format!("Create it: kubectl create namespace {namespace}");
        return vec![prerequisite(&exists, false, &create)];
    };
    let managed = serde_json::from_str::<serde_json::Value>(labels)
        .is_ok_and(|labels| labels[MANAGED_LABEL] == "true");
    let label = format!("kubectl label namespace {namespace} {MANAGED_LABEL}=true");
    vec![
        prerequisite(&exists, true, ""),
        prerequisite(
            "Namespace managed by the operator",
            managed,
            &format!("The operator only deploys a gateway in a labelled namespace: {label}"),
        ),
    ]
}

/// kubectl for `context`. The local cluster's goes through its container, so
/// checking it needs no kubectl on this machine.
fn kubectl_for(context: &str) -> std::process::Command {
    if context == crate::local_cluster::CONTEXT {
        return crate::local_cluster::kubectl_command();
    }
    let mut command = crate::env::command("kubectl");
    command
        .env("PATH", crate::env::composed_path())
        .args(["--context", context]);
    command
}

pub(crate) fn kubectl(context: &str, args: &[&str]) -> Result<String, String> {
    let output = kubectl_for(context)
        .arg(KUBECTL_TIMEOUT)
        .args(args)
        .output()
        .map_err(|e| format!("kubectl could not run: {e}"))?;
    outcome(output)
}

/// kubectl reading `input` on stdin, so a manifest holding a secret value
/// never shows on a command line.
pub(crate) fn kubectl_with_input(
    context: &str,
    args: &[&str],
    input: &str,
) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = kubectl_for(context)
        .arg(KUBECTL_TIMEOUT)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("kubectl could not run: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("kubectl has no stdin")?
        .write_all(input.as_bytes())
        .map_err(|e| format!("writing to kubectl: {e}"))?;
    outcome(
        child
            .wait_with_output()
            .map_err(|e| format!("kubectl did not finish: {e}"))?,
    )
}

fn outcome(output: std::process::Output) -> Result<String, String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(last_line(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// kubectl logs its retries before the line that says what went wrong.
fn last_line(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("kubectl failed")
        .to_owned()
}

/// Deletes every machine a Kubernetes workspace still has in its cluster and
/// answers how many went. The operator removes each machine's pod and volumes
/// with its Orchestrator.
pub(crate) fn delete_machines(workspace: &WorkspaceConfig) -> Result<usize, String> {
    if workspace.kind != KUBERNETES || workspace.context.is_empty() {
        return Ok(0);
    }
    let selector = format!("{WORKSPACE_LABEL}={}", workspace.name);
    let mut args = vec![
        "delete",
        ORCHESTRATORS,
        "-l",
        &selector,
        "--ignore-not-found",
        "--wait=false",
    ];
    if !workspace.namespace.is_empty() {
        args.extend(["-n", &workspace.namespace]);
    }
    kubectl(&workspace.context, &args)
        .map(|deleted| {
            deleted
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .map_err(|error| format!("{}: {error}", workspace.name))
}

/// Running and suspended sessions from the Ready status of each session's
/// Orchestrator, one per line as kubectl prints them.
fn session_counts(ready_statuses: &str) -> (usize, usize) {
    let sessions = ready_statuses.lines().count();
    let running = ready_statuses
        .lines()
        .filter(|status| status.trim() == "True")
        .count();
    (running, sessions - running)
}

pub(crate) fn unreachable(error: &str) -> ClusterCheck {
    ClusterCheck {
        prerequisites: vec![prerequisite(REACHABLE, false, error)],
        running: None,
        suspended: None,
    }
}

/// ponytail: CRDs and the namespace only, not whether the controllers run.
/// Check their deployments once a cluster with stale CRDs shows up.
/// What the cluster in `context` has of what sessions need, and how many of
/// the sessions `selector` picks in `namespace` run and are suspended.
fn check_cluster(context: &str, namespace: &str, selector: &str) -> ClusterCheck {
    let crds = match kubectl(context, &["get", "crd", "-o", "name"]) {
        Ok(listing) => listing,
        Err(error) => return unreachable(&error),
    };
    let labels = kubectl(
        context,
        &[
            "get",
            "namespace",
            namespace,
            "-o",
            "jsonpath={.metadata.labels}",
        ],
    );
    let counts = kubectl(
        context,
        &[
            "get",
            ORCHESTRATORS,
            "-n",
            namespace,
            "-l",
            selector,
            "-o",
            READY_STATUSES,
        ],
    )
    .ok()
    .map(|statuses| session_counts(&statuses));
    let mut prerequisites = vec![prerequisite(REACHABLE, true, "")];
    prerequisites.extend(installed_prerequisites(&crd_names(&crds)));
    prerequisites.extend(namespace_prerequisites(
        namespace,
        labels.as_deref().map_err(String::as_str),
    ));
    ClusterCheck {
        prerequisites,
        running: counts.map(|(running, _)| running),
        suspended: counts.map(|(_, suspended)| suspended),
    }
}

fn installed_infer_version() -> Option<String> {
    crate::updates::installed_version(&crate::env::infer_bin_path(), "version")
        .filter(|version| version != "dev")
}

/// The CLI image of the installed `infer` version, so a session's pod runs
/// the same CLI as this machine. The workspace form starts from it.
#[tauri::command]
pub(crate) fn default_workspace_image() -> String {
    let version = installed_infer_version();
    format!("{CLI_IMAGE}:{}", version.as_deref().unwrap_or("latest"))
}

/// The names of the provider keys the user saved, for a workspace's gateway.
pub(crate) fn provider_keys() -> Vec<String> {
    crate::config::auth_env()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// The local cluster as `compute.yaml` registers it, its gateway given every
/// provider key the user saved.
fn local_cluster(provider_keys: &[String]) -> ClusterConfig {
    ClusterConfig {
        name: crate::local_cluster::CONTEXT.into(),
        context: crate::local_cluster::CONTEXT.into(),
        namespace: DETECTED_NAMESPACE.into(),
        gateway_env: provider_keys.to_vec(),
    }
}

/// The workspace of the local cluster: an Ubuntu desktop pod per chat on it.
fn local_workspace() -> WorkspaceConfig {
    WorkspaceConfig {
        name: LOCAL_WORKSPACE.into(),
        compute: crate::local_cluster::CONTEXT.into(),
        image: crate::local_cluster::COMPUTER_IMAGE.into(),
        storage: DETECTED_STORAGE.into(),
        env: COMPUTER_ENV
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        ..WorkspaceConfig::default()
    }
}

/// The entry the local cluster's workspace replaces: its own from an earlier
/// run, or none. A workspace of that name on another cluster is the user's,
/// so it is never re-pointed.
fn local_workspace_slot(existing: &[WorkspaceConfig]) -> Result<Option<&'static str>, String> {
    match existing
        .iter()
        .find(|workspace| workspace.name == LOCAL_WORKSPACE)
    {
        None => Ok(None),
        Some(workspace) if workspace.context == crate::local_cluster::CONTEXT => {
            Ok(Some(LOCAL_WORKSPACE))
        }
        Some(workspace) => Err(format!(
            "A workspace named {LOCAL_WORKSPACE} already runs in {} - delete it to provision the local cluster",
            workspace.context
        )),
    }
}

/// Merges `addition`'s clusters, users and contexts into the user's
/// kubeconfig, replacing entries of the same name. The current context and
/// everything else stay. An unreadable kubeconfig is refused, never replaced.
fn merge_kubeconfig(existing: Option<&str>, addition: &str) -> Result<String, String> {
    let mut merged = match existing.filter(|text| !text.trim().is_empty()) {
        Some(text) => serde_norway::from_str::<Value>(text)
            .map_err(|e| format!("your kubeconfig is not valid YAML: {e}"))?,
        None => Value::Mapping(Mapping::new()),
    };
    let addition: Value = serde_norway::from_str(addition).map_err(|e| e.to_string())?;
    let root = merged
        .as_mapping_mut()
        .ok_or("your kubeconfig is not a mapping")?;
    for (key, default) in [("apiVersion", "v1"), ("kind", "Config")] {
        root.entry(key.into()).or_insert_with(|| default.into());
    }
    for list in ["clusters", "users", "contexts"] {
        let added = addition
            .get(list)
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        let entries = root
            .entry(list.into())
            .or_insert_with(|| Value::Sequence(Vec::new()));
        if entries.is_null() {
            *entries = Value::Sequence(Vec::new());
        }
        let entries = entries
            .as_sequence_mut()
            .ok_or_else(|| format!("your kubeconfig's {list} is not a list"))?;
        entries.retain(|entry| !added.iter().any(|new| new.get("name") == entry.get("name")));
        entries.extend(added);
    }
    serde_norway::to_string(&merged).map_err(|e| e.to_string())
}

/// Creates `namespace` if it is missing and labels it for the operator.
fn managed_namespace(context: &str, namespace: &str) -> Result<(), String> {
    match kubectl(context, &["create", "namespace", namespace]) {
        Err(error) if !error.contains("AlreadyExists") => return Err(error),
        _ => {}
    }
    let label = format!("{MANAGED_LABEL}=true");
    kubectl(
        context,
        &["label", "namespace", namespace, &label, "--overwrite"],
    )
    .map(drop)
}

fn user_kubeconfig() -> PathBuf {
    let kubeconfig_env = std::env::var("KUBECONFIG").ok();
    kubeconfig_path(&crate::env::home_dir(), kubeconfig_env.as_deref())
}

/// Provisions the local cluster, adds it to the user's kubeconfig and
/// registers its workspace. A workspace of the same name elsewhere stops it
/// before anything is created. Mock mode registers the workspace only.
fn provision_local(stage: impl Fn(&str)) -> Result<String, String> {
    crate::compute_settings::require_compute()?;
    let slot = local_workspace_slot(&workspace_configs())?;
    if !crate::env::mock_mode() {
        let kubeconfig =
            crate::local_cluster::provision(installed_infer_version().as_deref(), &stage)?;
        stage("Creating the workspace namespace...");
        managed_namespace(crate::local_cluster::CONTEXT, DETECTED_NAMESPACE)?;
        stage("Adding the cluster to your kubeconfig...");
        crate::config::update_file(&user_kubeconfig(), |existing| {
            merge_kubeconfig(existing, &kubeconfig)
        })?;
    }
    stage("Registering the cluster and its workspace...");
    let cluster = local_cluster(&provider_keys());
    let registered = crate::compute_settings::read_clusters()
        .iter()
        .any(|existing| existing.name == cluster.name);
    crate::config::update_file(&crate::compute_settings::compute_path(), |existing| {
        crate::compute_settings::merge_cluster(
            existing,
            &cluster,
            registered.then_some(cluster.name.as_str()),
        )
    })?;
    crate::config::update_file(&crate::daemon::config_path(), |existing| {
        merge_workspace(existing, &local_workspace(), slot)
    })?;
    Ok(LOCAL_WORKSPACE.into())
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
        &crate::compute_settings::read_clusters(),
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
    std::fs::read_to_string(user_kubeconfig())
        .map(|kubeconfig| parse_contexts(&kubeconfig))
        .unwrap_or_default()
}

/// What the cluster of a Kubernetes workspace has of what sessions need -
/// agent-sandbox, the Gateway API, the operator and a managed namespace - and
/// how many of the workspace's own sessions run there.
#[tauri::command]
pub(crate) async fn check_workspace(
    context: String,
    namespace: String,
    name: String,
) -> ClusterCheck {
    check(context, namespace, format!("{WORKSPACE_LABEL}={name}")).await
}

/// Checks the cluster in `context`, counting the sessions `selector` picks.
pub(crate) async fn check(context: String, namespace: String, selector: String) -> ClusterCheck {
    if crate::env::mock_mode() {
        return ClusterCheck {
            prerequisites: vec![prerequisite(REACHABLE, true, "")],
            running: Some(0),
            suspended: Some(0),
        };
    }
    tokio::task::spawn_blocking(move || check_cluster(&context, &namespace, &selector))
        .await
        .unwrap_or_else(|e| unreachable(&e.to_string()))
}

/// Whether Docker answers, which is all provisioning a local cluster needs.
#[tauri::command]
pub(crate) async fn docker_available() -> bool {
    crate::env::mock_mode()
        || tokio::task::spawn_blocking(crate::local_cluster::docker_running)
            .await
            .unwrap_or(false)
}

/// Provisions a k3s cluster in Docker and registers it and its Ubuntu desktop
/// workspace, reporting each step. Answers the workspace's name.
#[tauri::command]
pub(crate) async fn provision_local_cluster(on_stage: Channel<String>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        provision_local(|stage| {
            let _ = on_stage.send(stage.to_owned());
        })
    })
    .await
    .map_err(|e| format!("provisioning stopped: {e}"))?
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
    fn a_cluster_meets_a_requirement_only_with_all_of_its_crds() {
        let listing = "customresourcedefinition.apiextensions.k8s.io/sandboxes.agents.x-k8s.io\ncustomresourcedefinition.apiextensions.k8s.io/orchestrators.core.inference-gateway.com\n";
        let crds = crd_names(listing);
        assert_eq!(crds.len(), 2);
        let met: Vec<(String, bool)> = installed_prerequisites(&crds)
            .into_iter()
            .map(|p| (p.name, p.met))
            .collect();
        assert_eq!(
            met,
            vec![
                ("agent-sandbox".to_string(), true),
                ("Gateway API".to_string(), false),
                ("Inference Gateway operator".to_string(), false),
            ]
        );
        assert!(installed_prerequisites(&crds)[0].detail.is_empty());
        assert!(
            installed_prerequisites(&crds)[2]
                .detail
                .contains("operator")
        );
    }

    #[test]
    fn a_namespace_has_to_exist_and_carry_the_managed_label() {
        let missing = namespace_prerequisites("agents", Err("not found"));
        assert_eq!(missing.len(), 1);
        assert!(!missing[0].met);
        assert!(
            missing[0]
                .detail
                .contains("kubectl create namespace agents")
        );

        let unlabelled = namespace_prerequisites("agents", Ok(""));
        assert!(unlabelled[0].met);
        assert!(!unlabelled[1].met);
        assert!(
            unlabelled[1]
                .detail
                .contains("kubectl label namespace agents")
        );

        let managed = r#"{"inference-gateway.com/managed":"true"}"#;
        assert!(
            namespace_prerequisites("agents", Ok(managed))
                .iter()
                .all(|p| p.met)
        );
    }

    #[test]
    fn a_kubectl_failure_is_told_by_its_last_line() {
        let stderr = "E1105 memcache.go:265] couldn't get current server API group list\nUnable to connect to the server: no such host\n\n";
        assert_eq!(
            last_line(stderr),
            "Unable to connect to the server: no such host"
        );
        assert_eq!(last_line(""), "kubectl failed");
    }

    #[test]
    fn sessions_count_as_running_when_their_orchestrator_is_ready() {
        assert_eq!(session_counts("True\nFalse\nTrue\n\n"), (2, 2));
        assert_eq!(session_counts(""), (0, 0));
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

    #[test]
    fn the_local_workspace_runs_the_desktop_image_on_the_local_cluster() {
        let cluster = local_cluster(&["OLLAMA_CLOUD_API_KEY".into()]);
        assert_eq!(cluster.name, crate::local_cluster::CONTEXT);
        assert_eq!(cluster.context, crate::local_cluster::CONTEXT);
        assert_eq!(cluster.gateway_env, ["OLLAMA_CLOUD_API_KEY"]);
        let workspace = local_workspace();
        assert_eq!(workspace.name, "ubuntu-desktop");
        assert_eq!(workspace.compute, cluster.name);
        assert_eq!(workspace.image, crate::local_cluster::COMPUTER_IMAGE);
        assert_eq!(workspace.env["INFER_COMPUTER_USE_ENABLED"], "true");
    }

    #[test]
    fn the_local_workspace_replaces_only_its_own_entry() {
        assert_eq!(local_workspace_slot(&[kubernetes("build")]), Ok(None));
        let own = on_cluster(local_workspace(), &[local_cluster(&[])]);
        assert_eq!(local_workspace_slot(&[own]), Ok(Some("ubuntu-desktop")));
        let elsewhere = WorkspaceConfig {
            compute: String::new(),
            context: "k3d-computer-use".into(),
            ..local_workspace()
        };
        let refused = local_workspace_slot(&[elsewhere]).unwrap_err();
        assert!(refused.contains("k3d-computer-use"), "{refused}");
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

    #[test]
    fn a_kubeconfig_merge_replaces_same_named_entries_and_keeps_the_rest() {
        let existing = "apiVersion: v1\nkind: Config\ncurrent-context: prod\nclusters:\n  - name: prod\n    cluster: {server: https://prod}\n  - name: infer-k3s\n    cluster: {server: https://127.0.0.1:1}\ncontexts:\n  - name: prod\n    context: {cluster: prod, user: prod}\nusers:\n  - name: prod\n    user: {token: t}\n";
        let addition = "clusters:\n  - name: infer-k3s\n    cluster: {server: https://127.0.0.1:2}\ncontexts:\n  - name: infer-k3s\n    context: {cluster: infer-k3s, user: infer-k3s}\nusers:\n  - name: infer-k3s\n    user: {token: k}\n";
        let merged: Value =
            serde_norway::from_str(&merge_kubeconfig(Some(existing), addition).unwrap()).unwrap();
        assert_eq!(merged["current-context"], "prod");
        let clusters = merged["clusters"].as_sequence().unwrap();
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0]["name"], "prod");
        assert_eq!(clusters[1]["cluster"]["server"], "https://127.0.0.1:2");
        assert_eq!(merged["contexts"].as_sequence().unwrap().len(), 2);
        assert_eq!(merged["users"][0]["user"]["token"], "t");
    }

    #[test]
    fn a_kubeconfig_merge_starts_a_missing_file_and_refuses_a_broken_one() {
        let addition = "clusters:\n  - name: infer-k3s\n    cluster: {}\n";
        let fresh: Value =
            serde_norway::from_str(&merge_kubeconfig(None, addition).unwrap()).unwrap();
        assert_eq!(fresh["kind"], "Config");
        assert_eq!(fresh["clusters"][0]["name"], "infer-k3s");
        assert!(merge_kubeconfig(Some("clusters: [unclosed"), addition).is_err());
    }
}
