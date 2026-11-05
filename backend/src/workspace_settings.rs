use serde_norway::{Mapping, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const WORKSPACES_KEY: &str = "workspaces";
const KUBERNETES: &str = "kubernetes";
const CLI_IMAGE: &str = "ghcr.io/inference-gateway/cli";
const DETECTED_NAMESPACE: &str = "infer-workspaces";
const DETECTED_STORAGE: &str = "5Gi";
const KUBECTL_TIMEOUT: &str = "--request-timeout=5s";
const MANAGED_LABEL: &str = "inference-gateway.com/managed";
const REACHABLE: &str = "Cluster reachable";
const ORCHESTRATORS: &str = "orchestrators.core.inference-gateway.com";
const READY_STATUSES: &str =
    r#"jsonpath={range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}"#;

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

fn kubectl(context: &str, args: &[&str]) -> Result<String, String> {
    let output = crate::env::command("kubectl")
        .env("PATH", crate::env::composed_path())
        .args(["--context", context, KUBECTL_TIMEOUT])
        .args(args)
        .output()
        .map_err(|e| format!("kubectl could not run: {e}"))?;
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

fn unreachable(error: &str) -> ClusterCheck {
    ClusterCheck {
        prerequisites: vec![prerequisite(REACHABLE, false, error)],
        running: None,
        suspended: None,
    }
}

/// ponytail: CRDs and the namespace only, not whether the controllers run.
/// Check their deployments once a cluster with stale CRDs shows up.
fn check_cluster(context: &str, namespace: &str) -> ClusterCheck {
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
        &["get", ORCHESTRATORS, "-n", namespace, "-o", READY_STATUSES],
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

/// A Kubernetes workspace for every context that no workspace runs in yet,
/// named after its context. A context whose name a workspace already has is
/// skipped, so nothing the user configured is replaced.
fn detected_workspaces(
    existing: &[WorkspaceConfig],
    contexts: &[String],
    image: &str,
    provider_keys: &[String],
) -> Vec<WorkspaceConfig> {
    let known = |context: &str| {
        existing
            .iter()
            .any(|workspace| workspace.context == context || workspace.name == context)
    };
    contexts
        .iter()
        .filter(|context| !known(context))
        .map(|context| WorkspaceConfig {
            name: context.clone(),
            kind: KUBERNETES.into(),
            context: context.clone(),
            namespace: DETECTED_NAMESPACE.into(),
            image: image.into(),
            storage: DETECTED_STORAGE.into(),
            gateway_env: provider_keys.to_vec(),
            ..WorkspaceConfig::default()
        })
        .collect()
}

fn merge_detected(
    daemon_yaml: Option<&str>,
    detected: &[WorkspaceConfig],
) -> Result<String, String> {
    let mut merged = daemon_yaml.unwrap_or_default().to_owned();
    for workspace in detected {
        merged = merge_workspace(Some(&merged), workspace, None)?;
    }
    Ok(merged)
}

/// The CLI image of the installed `infer` version, so a session's pod runs
/// the same CLI as this machine.
fn cli_image() -> String {
    let version = crate::updates::installed_version(&crate::env::infer_bin_path(), "version")
        .filter(|version| version != "dev");
    format!("{CLI_IMAGE}:{}", version.as_deref().unwrap_or("latest"))
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

/// Adds a Kubernetes workspace for every kubeconfig context that has none
/// yet and answers their names. Each gets the provider keys the user saved,
/// by name, for its gateway.
#[tauri::command]
pub(crate) fn detect_workspaces() -> Result<Vec<String>, String> {
    let provider_keys: Vec<String> = crate::config::auth_env()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let detected = detected_workspaces(
        &workspace_configs(),
        &kube_contexts(),
        &cli_image(),
        &provider_keys,
    );
    if detected.is_empty() {
        return Ok(Vec::new());
    }
    crate::config::update_file(&crate::daemon::config_path(), |existing| {
        merge_detected(existing, &detected)
    })?;
    Ok(detected
        .into_iter()
        .map(|workspace| workspace.name)
        .collect())
}

/// What the cluster of a Kubernetes workspace has of what sessions need -
/// agent-sandbox, the Gateway API, the operator and a managed namespace - and
/// how many sessions run there.
#[tauri::command]
pub(crate) async fn check_workspace(context: String, namespace: String) -> ClusterCheck {
    if crate::env::mock_mode() {
        return ClusterCheck {
            prerequisites: vec![prerequisite(REACHABLE, true, "")],
            running: Some(0),
            suspended: Some(0),
        };
    }
    tokio::task::spawn_blocking(move || check_cluster(&context, &namespace))
        .await
        .unwrap_or_else(|e| unreachable(&e.to_string()))
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
    fn detection_adds_a_workspace_per_context_that_has_none() {
        let existing = parse_configs(Some(DAEMON_YAML));
        let contexts = ["build-cluster", "laptop", "k3d-dev"].map(String::from);
        let keys = ["OPENAI_API_KEY".to_string()];
        let detected = detected_workspaces(&existing, &contexts, "cli:1.2.3", &keys);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "k3d-dev");
        assert_eq!(detected[0].context, "k3d-dev");
        assert_eq!(detected[0].kind, "kubernetes");
        assert_eq!(detected[0].image, "cli:1.2.3");
        assert_eq!(detected[0].gateway_env, keys);

        let merged = merge_detected(Some(DAEMON_YAML), &detected).unwrap();
        let after = parse_configs(Some(&merged));
        assert_eq!(after.len(), 3);
        assert!(merged.contains("token: secret"));
        assert!(detected_workspaces(&after, &contexts, "cli:1.2.3", &keys).is_empty());
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
}
