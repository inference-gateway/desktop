use serde_norway::{Mapping, Value};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// The kubeconfig context of the cluster this module runs in a container.
pub(crate) const CONTEXT: &str = "infer-k3s";
const CONTAINER: &str = "infer-k3s";
const K3S_IMAGE: &str = "docker.io/rancher/k3s:v1.37.1-k3s1";
const API_PORT: &str = "6443/tcp";
const K3S_KUBECONFIG: &str = "/etc/rancher/k3s/k3s.yaml";
const AGENT_SANDBOX: &str =
    "https://github.com/kubernetes-sigs/agent-sandbox/releases/download/v1.0.8/sandbox.yaml";
const OPERATOR: &str =
    "https://github.com/inference-gateway/operator/releases/download/v0.37.0/install.yaml";
const GATEWAY_API_CRD: &str = "crd/gateways.gateway.networking.k8s.io";
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
const GPU_DEVICES: &str = "/dev/dri";
/// shortcut: a fixed address on Podman's default network, read the subnet
/// once someone runs that network on another one.
const PODMAN_NODE_IP: &str = "10.88.0.200";
/// What a Docker or Podman node runs with: root in its container and a
/// restart by the daemon, which Apple's container has neither flag for.
const DAEMON_NODE_ARGS: [&str; 3] = ["--privileged", "--restart", "unless-stopped"];
/// What the node gets on Apple containers in place of `--privileged`: every
/// capability and a writable /proc/sys, which kubelet needs for its sysctls.
/// shortcut: a fixed size for the node's VM, whose default is 1 GiB. Make it
/// a setting once someone needs a bigger node.
const APPLE_NODE_ARGS: [&str; 10] = [
    "--cap-add",
    "ALL",
    "--read-only-path",
    "NONE",
    "--cpus",
    "4",
    "--memory",
    "8g",
    "--entrypoint",
    "/bin/sh",
];
/// How k3s starts on Apple containers, where the node gets a new address on
/// every start and kube-proxy exits when the node's changes: the node keeps a
/// fixed one on its loopback and only advertises the API on the current one.
/// shortcut: that address is fixed, pick another once a pod or service
/// network uses 10.200.0.0.
const APPLE_SERVER: &str = "ip addr add 10.200.0.1/32 dev lo; IP=$(ip -4 route get 1.1.1.1 | sed -n 's/.*src \\([0-9.]*\\).*/\\1/p'); IF=$(ip -4 route get 1.1.1.1 | sed -n 's/.*dev \\([^ ]*\\).*/\\1/p'); exec k3s server --node-ip 10.200.0.1 --advertise-address \"$IP\" --flannel-iface \"$IF\"";
const INSTALL_CONTAINER: &str = "brew install container";
const START_CONTAINER: &str = "container system start";
const SET_KERNEL: &str = "container system kernel set --recommended";

/// What runs the cluster's node. Podman on a libkrun machine is the one that
/// hands the node this Mac's GPU. Apple containers run it in a VM of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Runtime {
    #[default]
    Docker,
    Podman,
    Apple,
}

impl Runtime {
    pub(crate) const ALL: [Runtime; 3] = [Runtime::Docker, Runtime::Podman, Runtime::Apple];

    pub(crate) fn tool(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Podman => "podman",
            Runtime::Apple => "container",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Runtime::Docker => "Docker",
            Runtime::Podman => "Podman",
            Runtime::Apple => "Apple containers",
        }
    }

    /// What the node's container gets on this runtime alone. Podman gives a
    /// container a new address on every start and k3s exits when its address
    /// changes, so the node's is fixed there.
    fn node_args(self) -> Vec<&'static str> {
        match self {
            Runtime::Docker => DAEMON_NODE_ARGS.to_vec(),
            Runtime::Podman => [
                &DAEMON_NODE_ARGS[..],
                &["--device", GPU_DEVICES, "--ip", PODMAN_NODE_IP],
            ]
            .concat(),
            Runtime::Apple => APPLE_NODE_ARGS.to_vec(),
        }
    }

    /// What follows the k3s image: its `server` command, which Apple
    /// containers reach through a shell that fixes the node's address first.
    fn server_args(self) -> &'static [&'static str] {
        match self {
            Runtime::Apple => &["-c", APPLE_SERVER],
            _ => &["server"],
        }
    }
}

fn tool_command(runtime: Runtime) -> Command {
    let mut command = crate::env::command(runtime.tool());
    command.env("PATH", crate::env::composed_path());
    command
}

/// Stdout of a finished tool run, or the last line of its stderr, which is
/// where the runtime and kubectl say what went wrong.
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

/// The output of the runtime's own command line for `args`.
pub(crate) fn run(runtime: Runtime, args: &[&str]) -> Result<String, String> {
    let tool = runtime.tool();
    let output = tool_command(runtime)
        .args(args)
        .output()
        .map_err(|e| format!("{tool} could not run: {e}"))?;
    outcome(output, tool)
}

/// A command inside the cluster's node, its stdin open.
pub(crate) fn node_command(runtime: Runtime) -> Command {
    let mut command = tool_command(runtime);
    command.args(["exec", "-i", CONTAINER]);
    command
}

/// kubectl inside the cluster's container, so the cluster needs nothing but
/// its runtime on this machine. Its stdin stays open for a manifest to apply.
pub(crate) fn kubectl_command(runtime: Runtime) -> Command {
    let mut command = node_command(runtime);
    command.arg("kubectl");
    command
}

fn kubectl(runtime: Runtime, args: &[&str]) -> Result<String, String> {
    let output = kubectl_command(runtime)
        .args(args)
        .output()
        .map_err(|e| format!("{} could not run: {e}", runtime.tool()))?;
    outcome(output, "kubectl")
}

/// Whether the runtime's daemon answers, which is all the node needs of it.
pub(crate) fn answers(runtime: Runtime) -> bool {
    match runtime {
        Runtime::Apple => run(runtime, &["system", "status"]).is_ok(),
        _ => run(runtime, &["info", "--format", "{{.ServerVersion}}"]).is_ok(),
    }
}

/// What Apple's container looks like to someone about to run a cluster on
/// it. A Homebrew install comes without a Linux kernel.
#[derive(Debug, Default)]
struct AppleSetup {
    installed: bool,
    service_running: bool,
    kernel: bool,
}

/// The first thing missing for a cluster on Apple containers, with the
/// command that fixes it. None when everything is there.
fn apple_setup_problem(setup: &AppleSetup) -> Option<String> {
    if !setup.installed {
        return Some(format!(
            "Apple's container is not installed. It needs macOS 26 or newer on Apple silicon. Install it: {INSTALL_CONTAINER} (or the pkg from https://github.com/apple/container/releases)"
        ));
    }
    if !setup.service_running {
        return Some(format!(
            "The container service is not running. Start it: {START_CONTAINER}"
        ));
    }
    (!setup.kernel).then(|| {
        format!("The container service has no Linux kernel yet. Install the recommended one: {SET_KERNEL}")
    })
}

/// Whether the service has a kernel, from the properties it lists: the
/// `kernel` table names a binary once one is set.
fn has_kernel(properties: &str) -> bool {
    properties
        .split("\n[")
        .any(|table| table.starts_with("kernel]") && table.contains("binaryPath"))
}

/// Why this Mac cannot run a cluster on Apple containers yet, with the
/// command that fixes it.
pub(crate) fn apple_problem() -> Option<String> {
    let installed = run(Runtime::Apple, &["--version"]).is_ok();
    let service_running = installed && answers(Runtime::Apple);
    let properties = run(Runtime::Apple, &["system", "property", "list"]).unwrap_or_default();
    apple_setup_problem(&AppleSetup {
        installed,
        service_running,
        kernel: service_running && has_kernel(&properties),
    })
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|e| format!("no free port for the cluster's API: {e}"))
}

/// What provisioning answers when the cluster's folders changed, since a
/// folder is mounted only into a container as it is created.
pub(crate) const FOLDERS_CHANGED: &str = "The local cluster's folders changed. Recreate the cluster to mount them - this deletes its machines and everything on them";
/// What provisioning answers when the cluster runs on another runtime.
pub(crate) const RUNTIME_CHANGED: &str = "The local cluster runs on another runtime. Recreate the cluster to move it - this deletes its machines and everything on them";

/// A mount as it was asked for, from the form the runtime lists it in. Podman
/// appends the options it chose itself, of which only `ro` was asked for.
fn bind_as_asked(listed: &str) -> String {
    match listed.rsplit_once(':') {
        Some((mount, options)) if !options.starts_with('/') => {
            let read_only = options.split(',').any(|option| option == "ro");
            format!("{mount}{}", if read_only { ":ro" } else { "" })
        }
        _ => listed.to_owned(),
    }
}

/// The folder mounts a container was created with, sorted, from the JSON
/// `inspect` prints for them. Podman lists the image's own volumes too, which
/// have a name where a folder has a path.
fn binds_of(inspected: &str) -> Vec<String> {
    let mut binds: Vec<String> = serde_json::from_str::<Option<Vec<String>>>(inspected.trim())
        .ok()
        .flatten()
        .unwrap_or_default()
        .iter()
        .filter(|listed| listed.starts_with('/') || listed.contains('\\'))
        .map(|listed| bind_as_asked(listed))
        .collect();
    binds.sort();
    binds
}

/// The node's configuration from the JSON Apple's `inspect` prints: a list
/// with the one container, its mounts and published ports under `configuration`.
fn apple_configuration(inspected: &str) -> Option<serde_json::Value> {
    let mut containers: Vec<serde_json::Value> = serde_json::from_str(inspected.trim()).ok()?;
    containers
        .pop()?
        .get_mut("configuration")
        .map(serde_json::Value::take)
}

/// The folder mounts of a node on Apple containers, sorted, in the form they
/// were asked for. The image's own filesystems have no host path as source.
fn binds_of_apple_inspect(inspected: &str) -> Vec<String> {
    let configuration = apple_configuration(inspected).unwrap_or_default();
    let mut binds: Vec<String> = configuration["mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mount| {
            let source = mount["source"]
                .as_str()
                .filter(|source| source.starts_with('/'))?;
            let destination = mount["destination"].as_str()?;
            let read_only = mount["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option == "ro"));
            Some(format!(
                "{source}:{destination}{}",
                if read_only { ":ro" } else { "" }
            ))
        })
        .collect();
    binds.sort();
    binds
}

/// The local port a node on Apple containers publishes its API on, from the
/// JSON `inspect` prints.
fn port_of_apple_inspect(inspected: &str) -> Option<String> {
    apple_configuration(inspected)?["publishedPorts"]
        .as_array()?
        .iter()
        .find(|port| port["containerPort"] == 6443)
        .and_then(|port| port["hostPort"].as_u64())
        .map(|port| port.to_string())
}

fn container_binds(runtime: Runtime) -> Result<Vec<String>, String> {
    match runtime {
        Runtime::Apple => run(runtime, &["inspect", CONTAINER])
            .map(|inspected| binds_of_apple_inspect(&inspected)),
        _ => run(
            runtime,
            &[
                "inspect",
                "--type",
                "container",
                "--format",
                "{{json .HostConfig.Binds}}",
                CONTAINER,
            ],
        )
        .map(|inspected| binds_of(&inspected)),
    }
}

/// The runtime the cluster's node is on right now, looking at `chosen` first.
/// None when no node exists.
pub(crate) fn running_on(chosen: Runtime) -> Option<Runtime> {
    std::iter::once(chosen)
        .chain(
            Runtime::ALL
                .into_iter()
                .filter(|runtime| *runtime != chosen),
        )
        .find(|runtime| container_binds(*runtime).is_ok())
}

/// Whether the cluster runs with other folders than `binds` or on another
/// runtime than `runtime`, so they only take effect once it is recreated. A
/// cluster that does not exist yet has nothing pending.
pub(crate) fn recreate_pending(runtime: Runtime, binds: &[String]) -> bool {
    match running_on(runtime) {
        Some(host) if host == runtime => {
            container_binds(runtime).is_ok_and(|mounted| mounted != binds)
        }
        Some(_) => true,
        None => false,
    }
}

fn remove_container(runtime: Runtime) -> Result<(), String> {
    match runtime {
        Runtime::Apple => run(runtime, &["delete", "--force", CONTAINER]).map(drop),
        _ => run(runtime, &["rm", "--force", "--volumes", CONTAINER]).map(drop),
    }
}

/// Starts the k3s container, creating it on first use with `binds` mounted.
/// One that runs with other folders or on another runtime is replaced only
/// when `recreate` says so, because that deletes the cluster. Its API listens
/// on a fixed local port picked at creation, so the kubeconfig stays valid.
fn start_container(runtime: Runtime, binds: &[String], recreate: bool) -> Result<(), String> {
    match running_on(runtime) {
        Some(host) if host != runtime => {
            if !recreate {
                return Err(RUNTIME_CHANGED.into());
            }
            remove_container(host)?;
        }
        Some(_) => {
            if container_binds(runtime)? == binds {
                return run(runtime, &["start", CONTAINER]).map(drop);
            }
            if !recreate {
                return Err(FOLDERS_CHANGED.into());
            }
            remove_container(runtime)?;
        }
        None => {}
    }
    let published = format!("127.0.0.1:{}:6443", free_port()?);
    let mut args = vec![
        "run",
        "--detach",
        "--name",
        CONTAINER,
        "--publish",
        &published,
    ];
    args.extend(runtime.node_args());
    for bind in binds {
        args.extend(["--volume", bind]);
    }
    args.push(K3S_IMAGE);
    args.extend(runtime.server_args());
    run(runtime, &args).map(drop)
}

/// The local port the runtime's `port` reports for the container's API, as
/// `127.0.0.1:54321`.
fn port_of(docker_port: &str) -> Option<&str> {
    docker_port
        .lines()
        .next()
        .and_then(|line| line.trim().rsplit(':').next())
        .filter(|port| !port.is_empty())
}

/// The local port the node's API is published on.
fn published_port(runtime: Runtime) -> Result<String, String> {
    let port = match runtime {
        Runtime::Apple => port_of_apple_inspect(&run(runtime, &["inspect", CONTAINER])?),
        _ => port_of(&run(runtime, &["port", CONTAINER, API_PORT])?).map(str::to_owned),
    };
    port.ok_or_else(|| "the cluster's API port is not published".to_owned())
}

pub(crate) fn wait_until(
    what: &str,
    check: impl Fn() -> Result<String, String>,
) -> Result<(), String> {
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

fn install(runtime: Runtime, manifest: &str, namespace: &str) -> Result<(), String> {
    kubectl(runtime, &["apply", "--server-side", "-f", manifest])?;
    kubectl(
        runtime,
        &[
            "-n",
            namespace,
            "wait",
            "--for=condition=Available",
            "deployment",
            "--all",
            "--timeout=180s",
        ],
    )
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
/// and pointed at the port the runtime publishes, ready to merge into the user's.
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

/// Runs k3s on `runtime` with agent-sandbox and the Inference Gateway
/// operator, its node mounting `binds`. Each step is safe to repeat, so a
/// second run repairs or refreshes the cluster. Answers the cluster's
/// kubeconfig, reporting each step to `stage`.
pub(crate) fn provision(
    stage: impl Fn(&str),
    runtime: Runtime,
    binds: &[String],
    recreate: bool,
) -> Result<String, String> {
    stage(&format!(
        "Starting k3s in {} (the first run downloads it)...",
        runtime.label()
    ));
    start_container(runtime, binds, recreate)?;
    stage("Waiting for the cluster...");
    wait_until("the cluster", || {
        kubectl(runtime, &["get", "--raw=/readyz"])
    })?;
    stage("Installing agent-sandbox...");
    install(runtime, AGENT_SANDBOX, "agent-sandbox-system")?;
    stage("Waiting for the Gateway API...");
    wait_until("the Gateway API", || {
        kubectl(runtime, &["get", GATEWAY_API_CRD])
    })?;
    stage("Installing the Inference Gateway operator...");
    install(runtime, OPERATOR, "inference-gateway-system")?;
    let port = published_port(runtime)?;
    standalone_kubeconfig(
        &run(runtime, &["exec", CONTAINER, "cat", K3S_KUBECONFIG])?,
        &port,
    )
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
    fn a_containers_folder_mounts_are_read_sorted_and_none_reads_as_empty() {
        assert_eq!(
            binds_of("[\"/b:/node/b:ro\",\"/a:/node/a\"]\n"),
            ["/a:/node/a", "/b:/node/b:ro"]
        );
        assert!(binds_of("null\n").is_empty());
        assert!(binds_of("").is_empty());
    }

    #[test]
    fn podmans_listing_of_mounts_reads_as_the_folders_that_were_asked_for() {
        let listed = r#"["60f1a11ee2ec:/var/lib/cni:rprivate,rw,nodev,exec,nosuid,rbind","/b:/node/b:ro,rprivate,rbind","/a:/node/a:rprivate,rbind"]"#;
        assert_eq!(binds_of(listed), ["/a:/node/a", "/b:/node/b:ro"]);
    }

    const APPLE_INSPECT: &str = r#"[{"configuration":{"id":"infer-k3s","mounts":[{"destination":"/var/lib/rancher/k3s/folders/raw/ro/raw","options":["ro"],"source":"/Users/me/raw","type":{"virtiofs":{}}},{"destination":"/var/lib/rancher/k3s/folders/raw/rw/raw","options":[],"source":"/Users/me/raw","type":{"virtiofs":{}}},{"destination":"/","options":[],"source":"rootfs","type":{"block":{}}}],"publishedPorts":[{"containerPort":6443,"hostAddress":"127.0.0.1","hostPort":54321,"proto":"tcp"}]},"status":{"state":"running"}}]"#;

    #[test]
    fn only_podman_hands_the_node_the_gpu_and_a_fixed_address() {
        assert!(Runtime::Docker.node_args().contains(&"--privileged"));
        assert!(!Runtime::Docker.node_args().contains(&GPU_DEVICES));
        assert!(Runtime::Podman.node_args().contains(&"--privileged"));
        assert!(Runtime::Podman.node_args().contains(&GPU_DEVICES));
        assert!(Runtime::Podman.node_args().contains(&PODMAN_NODE_IP));
    }

    #[test]
    fn the_published_port_is_read_from_docker_port() {
        assert_eq!(port_of("127.0.0.1:54321\n"), Some("54321"));
        assert_eq!(port_of(""), None);
    }

    #[test]
    fn apple_containers_run_the_node_with_every_capability_and_a_sized_vm() {
        let args = Runtime::Apple.node_args();
        assert_eq!(args[..2], ["--cap-add", "ALL"]);
        assert!(args.contains(&"--read-only-path"));
        assert_eq!(args[args.len() - 2..], ["--entrypoint", "/bin/sh"]);
        assert_eq!(Runtime::Docker.server_args(), ["server"]);
        assert_eq!(Runtime::Apple.server_args()[0], "-c");
        assert!(APPLE_SERVER.contains("ip addr add 10.200.0.1/32 dev lo"));
        assert!(APPLE_SERVER.contains("exec k3s server --node-ip"));
        assert!(args.contains(&"--cpus") && args.contains(&"--memory"));
        assert!(!args.contains(&"--privileged") && !args.contains(&"--restart"));
        assert!(!args.contains(&GPU_DEVICES));
    }

    #[test]
    fn the_published_port_and_folder_mounts_are_read_from_apple_inspect() {
        assert_eq!(port_of_apple_inspect(APPLE_INSPECT), Some("54321".into()));
        assert_eq!(
            binds_of_apple_inspect(APPLE_INSPECT),
            [
                "/Users/me/raw:/var/lib/rancher/k3s/folders/raw/ro/raw:ro",
                "/Users/me/raw:/var/lib/rancher/k3s/folders/raw/rw/raw",
            ]
        );
        assert_eq!(port_of_apple_inspect("[]"), None);
        assert_eq!(port_of_apple_inspect(""), None);
        assert!(binds_of_apple_inspect("").is_empty());
    }

    #[test]
    fn each_missing_piece_of_the_apple_setup_names_its_fix() {
        let ready = AppleSetup {
            installed: true,
            service_running: true,
            kernel: true,
        };
        assert_eq!(apple_setup_problem(&ready), None);
        let problem = |setup: AppleSetup| apple_setup_problem(&setup).unwrap();
        assert!(problem(AppleSetup::default()).contains(INSTALL_CONTAINER));
        let stopped = AppleSetup {
            service_running: false,
            ..ready
        };
        assert!(problem(stopped).contains(START_CONTAINER));
        let no_kernel = AppleSetup {
            kernel: false,
            ..ready
        };
        assert!(problem(no_kernel).contains(SET_KERNEL));
    }

    #[test]
    fn the_kernel_is_set_once_its_table_names_a_binary() {
        let set = "\n[dns]\n\n[kernel]\nbinaryPath = \"opt/kata/vmlinux\"\nurl = \"https://x\"\n\n[machine]\ncpus = 7\n";
        assert!(has_kernel(set));
        assert!(!has_kernel("\n[dns]\n\n[kernel]\n\n[machine]\ncpus = 7\n"));
        assert!(!has_kernel(""));
    }
}
