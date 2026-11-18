use crate::cli::compute::{
    COMPUTE_FILE, COMPUTE_LABEL, ClusterConfig, compute_path, label_value, merge_cluster,
    read_clusters, require_compute,
};
use crate::cli::config::text;
use crate::cli::workspace_config::{
    KUBERNETES, WorkspaceConfig, WorkspaceResources, merge_workspace, parse_configs,
    remove_workspace,
};
use crate::cli::workspace_volumes::WorkspaceVolume;
use crate::compute::local_cluster::Runtime;
use serde_norway::{Mapping, Value};
use std::path::{Path, PathBuf};

/// The Ubuntu desktop the local cluster's sessions run, released with the CLI.
const COMPUTER_IMAGE: &str = "ghcr.io/inference-gateway/computer";
pub(crate) const DETECTED_NAMESPACE: &str = "infer-workspaces";
const DETECTED_STORAGE: &str = "5Gi";
const KUBECTL_TIMEOUT: &str = "--request-timeout=5s";
pub(crate) const MANAGED_LABEL: &str = "inference-gateway.com/managed";
const REACHABLE: &str = "Cluster reachable";
pub(crate) const ORCHESTRATORS: &str = "orchestrators.core.inference-gateway.com";
const WORKSPACE_LABEL: &str = "inference-gateway.com/workspace";
const READY_STATUSES: &str =
    r#"jsonpath={range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}"#;
pub(crate) const MACHINES: &str = r#"jsonpath={range .items[*]}{.metadata.name}{"\t"}{.status.conditions[?(@.type=="Ready")].status}{"\t"}{.status.conditions[?(@.type=="Ready")].reason}{"\n"}{end}"#;
const LOCAL_WORKSPACE: &str = "ubuntu-desktop";
/// A local cluster's sessions run on a pod of their own, not the user's
/// machine, so its agents run commands and edit files without asking.
const UNATTENDED_ENV: [(&str, &str); 3] = [
    ("INFER_TOOLS_BASH_ALLOW_APPEND", ".*"),
    ("INFER_TOOLS_WRITE_REQUIRE_APPROVAL", "false"),
    ("INFER_TOOLS_EDIT_REQUIRE_APPROVAL", "false"),
];

/// Something a cluster must have installed before sessions can run in it,
/// told by the CRDs it brings.
pub(crate) struct Requirement {
    name: &'static str,
    pub(crate) crds: &'static [&'static str],
    fix: &'static str,
}

pub(crate) const REQUIREMENTS: [Requirement; 3] = [
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

/// One machine of a workspace: the Orchestrator of a session, whether it is
/// ready and the reason its cluster gives.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Machine {
    name: String,
    ready: bool,
    reason: String,
}

pub(crate) fn prerequisite(name: &str, met: bool, detail: &str) -> Prerequisite {
    Prerequisite {
        name: name.into(),
        met,
        detail: if met { String::new() } else { detail.into() },
    }
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

/// The runtime the local cluster's node is on, or the chosen one before a
/// node exists.
fn local_runtime() -> Runtime {
    let chosen = crate::compute::cluster_folders::read_runtime();
    crate::compute::local_cluster::running_on(chosen).unwrap_or(chosen)
}

/// kubectl for `context`. The local cluster's goes through its container, so
/// checking it needs no kubectl on this machine.
fn kubectl_for(context: &str) -> std::process::Command {
    if context == crate::compute::local_cluster::CONTEXT {
        return crate::compute::local_cluster::kubectl_command(local_runtime());
    }
    let mut command = crate::env::command("kubectl");
    command
        .env("PATH", crate::env::composed_path())
        .args(["--context", context]);
    command
}

/// kubectl's output for `args` in `context`. This and `kubectl_with_input` are
/// the only places that reach a cluster, so mock mode answers both from
/// `kubectl_mock` and everything above them runs as it does for real.
pub(crate) fn kubectl(context: &str, args: &[&str]) -> Result<String, String> {
    if crate::env::mock_mode() {
        return Ok(crate::compute::kubectl_mock::output(args));
    }
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
    if crate::env::mock_mode() {
        return Ok(crate::compute::kubectl_mock::output(args));
    }
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

/// Machines from kubectl's listing, one per line as name, Ready status and reason.
fn parse_machines(listing: &str) -> Vec<Machine> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().filter(|name| !name.trim().is_empty())?;
            Some(Machine {
                name: name.trim().to_owned(),
                ready: fields.next() == Some("True"),
                reason: fields.next().unwrap_or_default().trim().to_owned(),
            })
        })
        .collect()
}

fn machines_of(context: &str, namespace: &str, workspace: &str) -> Result<Vec<Machine>, String> {
    let selector = format!("{WORKSPACE_LABEL}={workspace}");
    kubectl(
        context,
        &[
            "get",
            ORCHESTRATORS,
            "-n",
            namespace,
            "-l",
            &selector,
            "-o",
            MACHINES,
        ],
    )
    .map(|listing| parse_machines(&listing))
}

/// The operator removes the machine's pod and volumes with its Orchestrator.
fn delete_machine_named(context: &str, namespace: &str, machine: &str) -> Result<(), String> {
    kubectl(
        context,
        &[
            "delete",
            ORCHESTRATORS,
            machine,
            "-n",
            namespace,
            "--ignore-not-found",
            "--wait=false",
        ],
    )
    .map(drop)
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
fn check_cluster_blocking(context: &str, namespace: &str, selector: &str) -> ClusterCheck {
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
    if has_gpu(context) {
        prerequisites.extend(crate::compute::cluster_gpu::prerequisites());
    }
    ClusterCheck {
        prerequisites,
        running: counts.map(|(running, _)| running),
        suspended: counts.map(|(_, suspended)| suspended),
    }
}

/// Whether `context` is the local cluster on Podman, the one cluster the app
/// gives a GPU. Mock mode runs no cluster.
fn has_gpu(context: &str) -> bool {
    context == crate::compute::local_cluster::CONTEXT
        && !crate::env::mock_mode()
        && local_runtime() == Runtime::Podman
}

fn installed_infer_version() -> Option<String> {
    crate::updates::installed_version(&crate::env::infer_bin_path(), "version")
        .filter(|version| version != "dev")
}

/// `repository` tagged with the installed `infer` version, so a session's pod
/// runs the same CLI as this machine.
fn of_installed_version(repository: &str) -> String {
    let version = installed_infer_version();
    format!("{repository}:{}", version.as_deref().unwrap_or("latest"))
}

/// The Ubuntu desktop image of the installed `infer` version. The workspace
/// form starts from it, since a new workspace starts with every capability.
#[tauri::command(async)]
pub(crate) fn default_workspace_image() -> String {
    of_installed_version(COMPUTER_IMAGE)
}

/// The names of the provider keys the user saved, for a workspace's gateway.
pub(crate) fn provider_keys() -> Vec<String> {
    crate::cli::config::auth_env()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// The local cluster as `compute.yaml` registers it, its gateway given every
/// provider key the user saved.
fn local_cluster(provider_keys: &[String]) -> ClusterConfig {
    ClusterConfig {
        name: crate::compute::local_cluster::CONTEXT.into(),
        context: crate::compute::local_cluster::CONTEXT.into(),
        namespace: DETECTED_NAMESPACE.into(),
        gateway_env: provider_keys.to_vec(),
    }
}

/// The workspace of the local cluster: an Ubuntu desktop pod per chat on it,
/// given every capability a workspace can have.
fn local_workspace() -> WorkspaceConfig {
    let capabilities = &crate::compute::workspace_capabilities::CAPABILITIES;
    WorkspaceConfig {
        name: LOCAL_WORKSPACE.into(),
        compute: crate::compute::local_cluster::CONTEXT.into(),
        image: of_installed_version(COMPUTER_IMAGE),
        storage: DETECTED_STORAGE.into(),
        env: capabilities
            .iter()
            .flat_map(|capability| capability.env)
            .chain(&UNATTENDED_ENV)
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        user_data: capabilities
            .iter()
            .map(|capability| capability.user_data)
            .collect(),
        forward_voice_samples: capabilities
            .iter()
            .any(|capability| capability.forward_voice_samples),
        ..WorkspaceConfig::default()
    }
}

/// The volumes the local cluster's workspace already mounts. Provisioning
/// again refreshes the workspace, and the folders it mounts are the user's.
fn local_workspace_volumes(existing: &[WorkspaceConfig]) -> Vec<WorkspaceVolume> {
    existing
        .iter()
        .find(|workspace| workspace.name == LOCAL_WORKSPACE)
        .map(|workspace| workspace.volumes.clone())
        .unwrap_or_default()
}

/// The CPU and memory the local cluster's workspace was already given, which
/// provisioning again keeps.
fn local_workspace_resources(existing: &[WorkspaceConfig]) -> WorkspaceResources {
    existing
        .iter()
        .find(|workspace| workspace.name == LOCAL_WORKSPACE)
        .map(|workspace| workspace.resources.clone())
        .unwrap_or_default()
}

/// Whether the local cluster's workspace already went without the GPU, which
/// provisioning again keeps.
fn local_workspace_opts_out_of_gpu(existing: &[WorkspaceConfig]) -> bool {
    existing
        .iter()
        .any(|workspace| workspace.name == LOCAL_WORKSPACE && workspace.no_gpu)
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
        Some(workspace) if workspace.context == crate::compute::local_cluster::CONTEXT => {
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

/// What provisioning the local cluster ended with: the workspace it
/// registered, and the secrets it could not put back into the cluster.
#[derive(Debug, PartialEq, serde::Serialize)]
pub(crate) struct Provisioned {
    workspace: String,
    unrestored: Vec<String>,
}

/// Provisions the local cluster, adds it to the user's kubeconfig and
/// registers its workspace. A workspace of the same name elsewhere stops it
/// before anything is created. `restore_secrets` puts the stored secrets back
/// into the cluster's context. Mock mode registers the workspace only.
pub(crate) fn provision_local(
    stage: impl Fn(&str),
    recreate: bool,
    restore_secrets: impl Fn(&str) -> Vec<String>,
) -> Result<Provisioned, String> {
    crate::cli::compute::require_compute()?;
    let existing = workspace_configs();
    let slot = local_workspace_slot(&existing)?;
    let workspace = WorkspaceConfig {
        volumes: local_workspace_volumes(&existing),
        no_gpu: local_workspace_opts_out_of_gpu(&existing),
        resources: local_workspace_resources(&existing),
        ..local_workspace()
    };
    let mut unrestored = Vec::new();
    if !crate::env::mock_mode() {
        let runtime = crate::compute::cluster_folders::read_runtime();
        if let Some(problem) = runtime_problem(runtime) {
            return Err(problem);
        }
        let folders = crate::compute::cluster_folders::read_folders();
        let binds = crate::compute::cluster_folders::binds(&folders);
        let kubeconfig =
            crate::compute::local_cluster::provision(&stage, runtime, &binds, recreate)?;
        stage("Creating the workspace namespace...");
        managed_namespace(crate::compute::local_cluster::CONTEXT, DETECTED_NAMESPACE)?;
        if runtime == Runtime::Podman {
            stage("Giving the cluster's computers the GPU...");
            kubectl_with_input(
                crate::compute::local_cluster::CONTEXT,
                &["apply", "-f", "-"],
                &crate::compute::cluster_gpu::manifest(DETECTED_NAMESPACE),
            )?;
            crate::compute::local_cluster::wait_until(
                "the GPU",
                crate::compute::cluster_gpu::offered,
            )?;
        }
        if let Some(manifest) =
            crate::compute::cluster_folders::claims_manifest(&folders, DETECTED_NAMESPACE)
        {
            stage("Sharing this computer's folders...");
            kubectl_with_input(
                crate::compute::local_cluster::CONTEXT,
                &["apply", "-f", "-"],
                &manifest,
            )?;
        }
        stage("Restoring the workspaces' secrets...");
        unrestored = restore_secrets(crate::compute::local_cluster::CONTEXT);
        stage("Adding the cluster to your kubeconfig...");
        crate::cli::config::update_file(&user_kubeconfig(), |existing| {
            merge_kubeconfig(existing, &kubeconfig)
        })?;
    }
    stage("Registering the cluster and its workspace...");
    let cluster = local_cluster(&provider_keys());
    let registered = crate::cli::compute::read_clusters()
        .iter()
        .any(|existing| existing.name == cluster.name);
    crate::cli::config::update_file(&crate::cli::compute::compute_path(), |existing| {
        crate::cli::compute::merge_cluster(
            existing,
            &cluster,
            registered.then_some(cluster.name.as_str()),
        )
    })?;
    crate::cli::config::update_file(&crate::env::daemon_config_path(), |existing| {
        merge_workspace(existing, &workspace, slot)
    })?;
    Ok(Provisioned {
        workspace: LOCAL_WORKSPACE.into(),
        unrestored,
    })
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
        std::fs::read_to_string(crate::env::daemon_config_path())
            .ok()
            .as_deref(),
        &crate::cli::compute::read_clusters(),
    )
}

/// Adds a workspace to `daemon.yaml`, or replaces the one `previous_name`
/// names. The daemon reads it on its next start.
#[tauri::command]
pub(crate) fn save_workspace(
    config: WorkspaceConfig,
    previous_name: Option<String>,
) -> Result<(), String> {
    crate::cli::config::update_file(&crate::env::daemon_config_path(), |existing| {
        merge_workspace(existing, &config, previous_name.as_deref())
    })
}

#[tauri::command]
pub(crate) fn delete_workspace(name: String) -> Result<(), String> {
    crate::cli::config::update_file(&crate::env::daemon_config_path(), |existing| {
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

/// What a cluster has of what sessions need, and how many sessions every
/// workspace on it runs.
#[tauri::command]
pub(crate) async fn check_cluster(name: String) -> ClusterCheck {
    let Some(cluster) = read_clusters().into_iter().find(|c| c.name == name) else {
        return unreachable(&format!("{name} is not in {COMPUTE_FILE}"));
    };
    let selector = format!("{COMPUTE_LABEL}={name}");
    check(cluster.context, cluster.namespace, selector).await
}

/// A cluster for every kube context no cluster and no workspace uses yet,
/// named after it, in the namespace detection always uses, with the names of
/// the saved provider keys for its gateway. Nothing configured is replaced.
fn detected_clusters(
    clusters: &[ClusterConfig],
    workspace_contexts: &[String],
    contexts: &[String],
    provider_keys: &[String],
) -> Vec<ClusterConfig> {
    let known = |context: &str| {
        clusters
            .iter()
            .any(|cluster| cluster.context == context || cluster.name == context)
            || workspace_contexts.iter().any(|used| used == context)
    };
    contexts
        .iter()
        .filter(|context| !known(context) && label_value(context))
        .map(|context| ClusterConfig {
            name: context.clone(),
            context: context.clone(),
            namespace: DETECTED_NAMESPACE.into(),
            gateway_env: provider_keys.to_vec(),
        })
        .collect()
}

/// Adds a cluster for every kubeconfig context that has none yet and answers
/// their names.
#[tauri::command]
pub(crate) async fn detect_clusters() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        require_compute()?;
        let workspace_contexts: Vec<String> = workspace_configs()
            .into_iter()
            .filter(|workspace| workspace.compute.is_empty())
            .map(|workspace| workspace.context)
            .collect();
        let detected = detected_clusters(
            &read_clusters(),
            &workspace_contexts,
            &kube_contexts(),
            &provider_keys(),
        );
        if detected.is_empty() {
            return Ok(Vec::new());
        }
        crate::cli::config::update_file(&compute_path(), |existing| {
            detected.iter().try_fold(
                existing.unwrap_or_default().to_owned(),
                |merged, cluster| merge_cluster(Some(&merged), cluster, None),
            )
        })?;
        Ok(detected.into_iter().map(|cluster| cluster.name).collect())
    })
    .await
    .map_err(|e| format!("detecting clusters stopped: {e}"))?
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
    let mut check = check(
        context.clone(),
        namespace.clone(),
        format!("{WORKSPACE_LABEL}={name}"),
    )
    .await;
    if check
        .prerequisites
        .iter()
        .all(|prerequisite| prerequisite.met)
    {
        let claims = tokio::task::spawn_blocking(move || {
            claim_prerequisites(&context, &namespace, &claims_of(&name))
        })
        .await
        .unwrap_or_default();
        check.prerequisites.extend(claims);
    }
    check
}

/// The claims the workspace called `name` mounts, as `daemon.yaml` lists them.
fn claims_of(name: &str) -> Vec<String> {
    workspace_configs()
        .into_iter()
        .find(|workspace| workspace.name == name)
        .map(|workspace| workspace.volumes.into_iter().map(|v| v.claim).collect())
        .unwrap_or_default()
}

/// Whether each claim a workspace mounts exists in its namespace. A computer
/// whose claim is missing never starts, so the check says which one to create.
fn claim_prerequisites(context: &str, namespace: &str, claims: &[String]) -> Vec<Prerequisite> {
    claims
        .iter()
        .map(|claim| {
            let exists = kubectl(
                context,
                &["get", "persistentvolumeclaim", claim, "-n", namespace],
            )
            .is_ok();
            prerequisite(
                &format!("Volume claim {claim}"),
                exists,
                &format!(
                    "Create the PersistentVolumeClaim {claim} in {namespace}, or remove the volume."
                ),
            )
        })
        .collect()
}

/// The machines a Kubernetes workspace has in its cluster, ready or not, so a
/// broken one shows up in Settings.
#[tauri::command]
pub(crate) async fn workspace_machines(
    context: String,
    namespace: String,
    name: String,
) -> Result<Vec<Machine>, String> {
    tokio::task::spawn_blocking(move || machines_of(&context, &namespace, &name))
        .await
        .map_err(|e| format!("listing task failed: {e}"))?
}

/// Deletes one machine of a workspace by name, whatever state it is in.
#[tauri::command]
pub(crate) async fn delete_workspace_machine(
    context: String,
    namespace: String,
    machine: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || delete_machine_named(&context, &namespace, &machine))
        .await
        .map_err(|e| format!("delete task failed: {e}"))?
}

/// Checks the cluster in `context`, counting the sessions `selector` picks.
pub(crate) async fn check(context: String, namespace: String, selector: String) -> ClusterCheck {
    tokio::task::spawn_blocking(move || check_cluster_blocking(&context, &namespace, &selector))
        .await
        .unwrap_or_else(|e| unreachable(&e.to_string()))
}

const NO_DOCKER: &str = "Install and start Docker to provision a local cluster with one click.";

/// Why the local cluster cannot be provisioned on `runtime` right now, with
/// what fixes it. Docker without a GPU setup needs only its daemon.
fn runtime_problem(runtime: Runtime) -> Option<String> {
    match runtime {
        Runtime::Docker => {
            (!crate::compute::local_cluster::answers(runtime)).then(|| NO_DOCKER.to_owned())
        }
        Runtime::Podman => crate::compute::cluster_gpu::podman_problem(),
    }
}

/// The local cluster as the Compute tab shows it: the runtime chosen for it
/// and the ones on offer, the one it runs on, why it cannot be provisioned,
/// and whether what is saved only applies once it is recreated.
#[derive(Debug, PartialEq, serde::Serialize)]
pub(crate) struct LocalCluster {
    runtime: Runtime,
    runtimes: Vec<Runtime>,
    running: Option<Runtime>,
    problem: Option<String>,
    pending: bool,
}

/// Podman reaches a GPU only on a Mac, so the choice is offered only there.
fn offered_runtimes() -> Vec<Runtime> {
    if cfg!(target_os = "macos") || crate::env::mock_mode() {
        Runtime::ALL.to_vec()
    } else {
        vec![Runtime::Docker]
    }
}

/// Mock mode runs no cluster. One that was provisioned there counts as
/// running on the runtime chosen for it.
fn mock_local_cluster(runtime: Runtime) -> LocalCluster {
    let provisioned = crate::cli::compute::read_clusters()
        .iter()
        .any(|cluster| cluster.name == crate::compute::local_cluster::CONTEXT);
    LocalCluster {
        runtime,
        runtimes: offered_runtimes(),
        running: provisioned.then_some(runtime),
        problem: None,
        pending: false,
    }
}

fn local_cluster_now() -> LocalCluster {
    let runtime = crate::compute::cluster_folders::read_runtime();
    if crate::env::mock_mode() {
        return mock_local_cluster(runtime);
    }
    let binds =
        crate::compute::cluster_folders::binds(&crate::compute::cluster_folders::read_folders());
    LocalCluster {
        runtime,
        runtimes: offered_runtimes(),
        running: crate::compute::local_cluster::running_on(runtime),
        problem: runtime_problem(runtime),
        pending: crate::compute::local_cluster::recreate_pending(runtime, &binds),
    }
}

/// The local cluster's runtime, state and what stands in its way.
#[tauri::command]
pub(crate) async fn local_cluster_status() -> Result<LocalCluster, String> {
    tokio::task::spawn_blocking(local_cluster_now)
        .await
        .map_err(|e| format!("reading the local cluster stopped: {e}"))
}

/// Saves the runtime the local cluster is provisioned on. A cluster that
/// runs on the other one moves only when it is recreated.
#[tauri::command]
pub(crate) fn save_local_cluster_runtime(runtime: Runtime) -> Result<(), String> {
    crate::compute::cluster_folders::save_runtime(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::workspace_config::on_cluster;
    use crate::cli::workspace_config::tests::kubernetes;

    #[test]
    fn the_mock_cluster_meets_every_prerequisite_and_runs_nothing() {
        let crds = crate::compute::kubectl_mock::output(&["get", "crd", "-o", "name"]);
        assert!(
            installed_prerequisites(&crd_names(&crds))
                .iter()
                .all(|check| check.met)
        );
        let labels = crate::compute::kubectl_mock::output(&["get", "namespace", "ns"]);
        assert!(
            namespace_prerequisites("ns", Ok(&labels))
                .iter()
                .all(|check| check.met)
        );
        let statuses =
            crate::compute::kubectl_mock::output(&["get", ORCHESTRATORS, "-o", READY_STATUSES]);
        assert_eq!(session_counts(&statuses), (0, 0));
    }

    #[test]
    fn the_mock_cluster_holds_one_broken_machine_until_it_is_deleted() {
        let list = || {
            parse_machines(&crate::compute::kubectl_mock::output(&[
                "get",
                ORCHESTRATORS,
                "-o",
                MACHINES,
            ]))
        };
        let listed = list();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].ready);
        assert_eq!(listed[0].reason, "SandboxNotReady");

        crate::compute::kubectl_mock::output(&["delete", ORCHESTRATORS, "-l", "workspace=any"]);
        assert_eq!(list().len(), 1);

        crate::compute::kubectl_mock::output(&[
            "delete",
            ORCHESTRATORS,
            &listed[0].name,
            "-n",
            "ns",
        ]);
        assert!(list().is_empty());
    }

    #[test]
    fn a_workspace_opts_out_of_the_gpu_by_asking_for_none_and_its_other_amounts_stay() {
        let daemon = "workspaces:\n  - name: studio\n    compute: infer-k3s\n    resources:\n      limits:\n        memory: 8Gi\n";
        let studio = parse_configs(Some(daemon), &[]).remove(0);
        assert!(!studio.no_gpu);
        let without = WorkspaceConfig {
            no_gpu: true,
            ..studio
        };
        let saved = merge_workspace(Some(daemon), &without, Some("studio")).unwrap();
        assert!(saved.contains("devic.es/dri: '0'"), "{saved}");
        assert!(saved.contains("memory: 8Gi"), "{saved}");
        let reread = parse_configs(Some(&saved), &[]).remove(0);
        assert!(reread.no_gpu);
        assert!(local_workspace_opts_out_of_gpu(&[WorkspaceConfig {
            name: LOCAL_WORKSPACE.into(),
            ..reread.clone()
        }]));
        let moved = WorkspaceConfig {
            compute: "k3d".into(),
            ..reread.clone()
        };
        let elsewhere = merge_workspace(Some(&saved), &moved, Some("studio")).unwrap();
        assert!(!elsewhere.contains("devic.es"), "{elsewhere}");
        let with = WorkspaceConfig {
            no_gpu: false,
            ..reread
        };
        let back = merge_workspace(Some(&saved), &with, Some("studio")).unwrap();
        assert!(!back.contains("devic.es"), "{back}");
        assert!(back.contains("memory: 8Gi"), "{back}");
        let plain = WorkspaceConfig {
            resources: WorkspaceResources::default(),
            ..with
        };
        let alone = merge_workspace(None, &plain, None).unwrap();
        assert!(!alone.contains("resources"), "{alone}");
    }

    #[test]
    fn provisioning_again_keeps_the_cpu_and_memory_of_the_local_workspace() {
        let yaml = "workspaces:\n  - name: ubuntu-desktop\n    compute: infer-k3s\n    resources:\n      limits:\n        memory: 12Gi\n";
        let existing = parse_configs(Some(yaml), &[]);
        assert_eq!(local_workspace_resources(&existing).memory_limit, "12Gi");
        assert_eq!(local_workspace_resources(&[]).memory_limit, "");
    }

    #[test]
    fn provisioning_again_keeps_the_volumes_of_the_local_workspace() {
        let yaml = "workspaces:\n  - name: ubuntu-desktop\n    compute: infer-k3s\n    volumes:\n      - name: footage\n        claim: folder-footage-ro\n        mount_path: /mnt/footage\n        read_only: true\n";
        let existing = parse_configs(Some(yaml), &[]);
        let volumes = local_workspace_volumes(&existing);
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].claim, "folder-footage-ro");
        assert!(local_workspace_volumes(&[]).is_empty());
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
    fn machines_are_listed_ready_or_not_with_the_reason() {
        let machines = parse_machines(
            "infer-a\tTrue\tSandboxReady\ninfer-b\tFalse\tSandboxNotReady\ninfer-c\t\t\n\n",
        );
        let summary: Vec<_> = machines
            .iter()
            .map(|machine| {
                (
                    machine.name.as_str(),
                    machine.ready,
                    machine.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("infer-a", true, "SandboxReady"),
                ("infer-b", false, "SandboxNotReady"),
                ("infer-c", false, ""),
            ]
        );
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
    fn the_local_workspace_runs_the_computer_image_on_the_local_cluster() {
        let cluster = local_cluster(&["OLLAMA_CLOUD_API_KEY".into()]);
        assert_eq!(cluster.name, crate::compute::local_cluster::CONTEXT);
        assert_eq!(cluster.context, crate::compute::local_cluster::CONTEXT);
        assert_eq!(cluster.gateway_env, ["OLLAMA_CLOUD_API_KEY"]);
        let workspace = local_workspace();
        assert_eq!(workspace.name, "ubuntu-desktop");
        assert_eq!(workspace.compute, cluster.name);
        assert!(
            workspace
                .image
                .starts_with("ghcr.io/inference-gateway/computer:")
        );
        assert_eq!(workspace.env["INFER_COMPUTER_USE_ENABLED"], "true");
        assert_eq!(workspace.env["INFER_COMPUTER_USE_APPROVAL"], "never");
        assert_eq!(
            workspace.env["INFER_COMPUTER_USE_RECORDING_ENABLED"],
            "true"
        );
        assert_eq!(workspace.env["INFER_TOOLS_BASH_ALLOW_APPEND"], ".*");
        assert_eq!(workspace.env["INFER_TEXT_TO_SPEECH_ENABLED"], "true");
        assert!(!workspace.env.contains_key("INFER_TEXT_TO_SPEECH_MODEL"));
        assert!(workspace.forward_voice_samples);
    }

    #[test]
    fn the_local_workspace_installs_firefox_with_opentask_when_it_boots() {
        let user_data = local_workspace().user_data;
        assert!(user_data.starts_with("#!"));
        assert!(user_data.contains("apt-get install -y --no-install-recommends firefox-esr"));
        assert!(user_data.contains("opentask@inference-gateway.github.io"));
    }

    #[test]
    fn the_local_workspace_leaves_the_browser_closed_when_it_boots() {
        assert!(!local_workspace().user_data.contains("autostart"));
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

    #[test]
    fn detection_adds_a_cluster_per_context_nothing_uses_yet() {
        let contexts = ["k3d-agents", "legacy-context", "fresh", "has space"].map(String::from);
        let keys = ["OPENAI_API_KEY".to_string()];
        let known_cluster = ClusterConfig {
            name: "k3d".into(),
            context: "k3d-agents".into(),
            ..ClusterConfig::default()
        };
        let detected = detected_clusters(
            &[known_cluster],
            &["legacy-context".to_string()],
            &contexts,
            &keys,
        );
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "fresh");
        assert_eq!(detected[0].namespace, DETECTED_NAMESPACE);
        assert_eq!(detected[0].gateway_env, keys);
    }
}
