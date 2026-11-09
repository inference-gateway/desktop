use serde_norway::{Mapping, Value};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// The kubeconfig context of the cluster this module runs in Docker.
pub(crate) const CONTEXT: &str = "infer-k3s";
const CONTAINER: &str = "infer-k3s";
const K3S_IMAGE: &str = "docker.io/rancher/k3s:v1.37.1-k3s1";
const API_PORT: &str = "6443/tcp";
const K3S_KUBECONFIG: &str = "/etc/rancher/k3s/k3s.yaml";
const AGENT_SANDBOX: &str =
    "https://github.com/kubernetes-sigs/agent-sandbox/releases/download/v1.0.8/sandbox.yaml";
const OPERATOR: &str =
    "https://github.com/inference-gateway/operator/releases/download/v0.35.0/install.yaml";
const GATEWAY_API_CRD: &str = "crd/gateways.gateway.networking.k8s.io";
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_secs(3);

fn docker_command() -> Command {
    let mut command = crate::env::command("docker");
    command.env("PATH", crate::env::composed_path());
    command
}

/// Stdout of a finished tool run, or the last line of its stderr, which is
/// where docker and kubectl say what went wrong.
fn outcome(output: Output, tool: &str) -> Result<String, String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(|| format!("{tool} failed"), str::to_owned))
}

fn docker(args: &[&str]) -> Result<String, String> {
    let output = docker_command()
        .args(args)
        .output()
        .map_err(|e| format!("docker could not run: {e}"))?;
    outcome(output, "docker")
}

/// kubectl inside the cluster's container, so the cluster needs nothing but
/// Docker on this machine. Its stdin stays open for a manifest to apply.
pub(crate) fn kubectl_command() -> Command {
    let mut command = docker_command();
    command.args(["exec", "-i", CONTAINER, "kubectl"]);
    command
}

fn kubectl(args: &[&str]) -> Result<String, String> {
    let output = kubectl_command()
        .args(args)
        .output()
        .map_err(|e| format!("docker could not run: {e}"))?;
    outcome(output, "kubectl")
}

/// Whether a Docker daemon answers, which is all the local cluster needs.
pub(crate) fn docker_running() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"]).is_ok()
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|e| format!("no free port for the cluster's API: {e}"))
}

/// Starts the k3s container, creating it on first use. Its API listens on a
/// fixed local port picked then, so the kubeconfig stays valid across restarts.
fn start_container() -> Result<(), String> {
    if docker(&["inspect", CONTAINER]).is_ok() {
        return docker(&["start", CONTAINER]).map(drop);
    }
    let published = format!("127.0.0.1:{}:6443", free_port()?);
    docker(&[
        "run",
        "--detach",
        "--name",
        CONTAINER,
        "--privileged",
        "--restart",
        "unless-stopped",
        "--publish",
        &published,
        K3S_IMAGE,
        "server",
    ])
    .map(drop)
}

/// The local port `docker port` reports for the container's API, as
/// `127.0.0.1:54321`.
fn port_of(docker_port: &str) -> Option<&str> {
    docker_port
        .lines()
        .next()
        .and_then(|line| line.trim().rsplit(':').next())
        .filter(|port| !port.is_empty())
}

fn wait_until(what: &str, check: impl Fn() -> Result<String, String>) -> Result<(), String> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match check() {
            Ok(_) => return Ok(()),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!("{what} did not come up: {error}"));
            }
            Err(_) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

fn install(manifest: &str, namespace: &str) -> Result<(), String> {
    kubectl(&["apply", "--server-side", "-f", manifest])?;
    kubectl(&[
        "-n",
        namespace,
        "wait",
        "--for=condition=Available",
        "deployment",
        "--all",
        "--timeout=180s",
    ])
    .map(drop)
}

fn named(name: &str, key: &str, value: Value) -> Value {
    let mut entry = Mapping::new();
    entry.insert("name".into(), name.into());
    entry.insert(key.into(), value);
    Value::Mapping(entry)
}

fn first_entry(kubeconfig: &Value, list: &str, key: &str) -> Result<Value, String> {
    kubeconfig
        .get(list)
        .and_then(Value::as_sequence)
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.get(key))
        .cloned()
        .ok_or_else(|| format!("the cluster's kubeconfig has no {list}"))
}

/// The cluster's own kubeconfig, renamed from k3s's `default` to `CONTEXT`
/// and pointed at the port Docker publishes, ready to merge into the user's.
fn standalone_kubeconfig(k3s_yaml: &str, port: &str) -> Result<String, String> {
    let k3s: Value = serde_norway::from_str(k3s_yaml).map_err(|e| e.to_string())?;
    let mut cluster = first_entry(&k3s, "clusters", "cluster")?;
    cluster
        .as_mapping_mut()
        .ok_or("the cluster's kubeconfig cluster is not a mapping")?
        .insert("server".into(), format!("https://127.0.0.1:{port}").into());
    let user = first_entry(&k3s, "users", "user")?;
    let mut context = Mapping::new();
    context.insert("cluster".into(), CONTEXT.into());
    context.insert("user".into(), CONTEXT.into());
    let mut kubeconfig = Mapping::new();
    kubeconfig.insert("apiVersion".into(), "v1".into());
    kubeconfig.insert("kind".into(), "Config".into());
    kubeconfig.insert(
        "clusters".into(),
        Value::Sequence(vec![named(CONTEXT, "cluster", cluster)]),
    );
    kubeconfig.insert(
        "users".into(),
        Value::Sequence(vec![named(CONTEXT, "user", user)]),
    );
    kubeconfig.insert(
        "contexts".into(),
        Value::Sequence(vec![named(CONTEXT, "context", Value::Mapping(context))]),
    );
    serde_norway::to_string(&kubeconfig).map_err(|e| e.to_string())
}

/// Runs k3s in Docker with agent-sandbox and the Inference Gateway operator.
/// Each step is safe to repeat, so a second run repairs or refreshes the
/// cluster. Answers the cluster's kubeconfig, reporting each step to `stage`.
pub(crate) fn provision(stage: impl Fn(&str)) -> Result<String, String> {
    stage("Starting k3s in Docker (the first run downloads it)...");
    start_container()?;
    stage("Waiting for the cluster...");
    wait_until("the cluster", || kubectl(&["get", "--raw=/readyz"]))?;
    stage("Installing agent-sandbox...");
    install(AGENT_SANDBOX, "agent-sandbox-system")?;
    stage("Waiting for the Gateway API...");
    wait_until("the Gateway API", || kubectl(&["get", GATEWAY_API_CRD]))?;
    stage("Installing the Inference Gateway operator...");
    install(OPERATOR, "inference-gateway-system")?;
    let published = docker(&["port", CONTAINER, API_PORT])?;
    let port = port_of(&published).ok_or("the cluster's API port is not published")?;
    standalone_kubeconfig(&docker(&["exec", CONTAINER, "cat", K3S_KUBECONFIG])?, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K3S_YAML: &str = "apiVersion: v1\nclusters:\n- cluster:\n    certificate-authority-data: CA\n    server: https://127.0.0.1:6443\n  name: default\ncontexts:\n- context:\n    cluster: default\n    user: default\n  name: default\ncurrent-context: default\nkind: Config\npreferences: {}\nusers:\n- name: default\n  user:\n    client-certificate-data: CERT\n    client-key-data: KEY\n";

    #[test]
    fn the_kubeconfig_is_renamed_and_points_at_the_published_port() {
        let kubeconfig: Value =
            serde_norway::from_str(&standalone_kubeconfig(K3S_YAML, "54321").unwrap()).unwrap();
        let cluster = &kubeconfig["clusters"][0];
        assert_eq!(cluster["name"], CONTEXT);
        assert_eq!(cluster["cluster"]["server"], "https://127.0.0.1:54321");
        assert_eq!(cluster["cluster"]["certificate-authority-data"], "CA");
        assert_eq!(kubeconfig["users"][0]["name"], CONTEXT);
        assert_eq!(kubeconfig["users"][0]["user"]["client-key-data"], "KEY");
        assert_eq!(kubeconfig["contexts"][0]["name"], CONTEXT);
        assert_eq!(kubeconfig["contexts"][0]["context"]["cluster"], CONTEXT);
        assert_eq!(kubeconfig["contexts"][0]["context"]["user"], CONTEXT);
        assert!(kubeconfig.get("current-context").is_none());
    }

    #[test]
    fn a_kubeconfig_without_a_cluster_is_refused() {
        assert!(standalone_kubeconfig("apiVersion: v1\n", "54321").is_err());
    }

    #[test]
    fn the_published_port_is_read_from_docker_port() {
        assert_eq!(port_of("127.0.0.1:54321\n"), Some("54321"));
        assert_eq!(port_of(""), None);
    }
}
