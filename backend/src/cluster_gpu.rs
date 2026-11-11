use crate::local_cluster::Runtime;
use crate::workspace_settings::{Prerequisite, prerequisite};

/// The resource the node offers its GPU's render device as, which a computer
/// asks for to get it.
pub(crate) const RESOURCE: &str = "devic.es/dri";
const RENDER_DEVICE: &str = "/dev/dri/renderD128";
const DEVICE_PLUGIN_IMAGE: &str = "ghcr.io/squat/generic-device-plugin:latest@sha256:8dc84958301f054bf72d9753a7a2c9af3d6642a9844b34d1171b3f2cdb4a15f6";
/// How many containers share the one render device at a time.
const SHARES: u32 = 100;
const VENUS: &str = "Virtio-GPU Venus";
const COMPUTER_IMAGE: &str = "inference-gateway/computer";
const PROBE_SECONDS: &str = "30";
/// What the node's `timeout` exits with once the probe ran out of time: its
/// own code, or the one of a probe it had to kill.
const TIMED_OUT: [i32; 2] = [124, 137];
const INSTALL_PODMAN: &str = "brew install podman";
const INSTALL_KRUNKIT: &str = "brew tap slp/krunkit && brew install krunkit";
const CREATE_MACHINE: &str =
    "CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --rootful --now";
const LIBKRUN: &str = "libkrun";
const GPU_NAME: &str = "GPU";
const HUNG: &str = "The virtual GPU stopped answering, so an app that uses it hangs. Restart the VM: podman machine stop && podman machine start. Until then switch GPU off on the workspace";
const ALLOCATABLE: &str = r"jsonpath={.items[*].status.allocatable.devic\.es/dri}";

/// What gives a cluster on Podman its GPU: a device plugin that offers the
/// node's render device, and a default in `namespace` that hands it to every
/// container there. A workspace opts out by asking for none.
pub(crate) fn manifest(namespace: &str) -> String {
    format!(
        r#"apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: infer-gpu-device-plugin
  namespace: kube-system
spec:
  selector:
    matchLabels:
      app.kubernetes.io/name: infer-gpu-device-plugin
  template:
    metadata:
      labels:
        app.kubernetes.io/name: infer-gpu-device-plugin
    spec:
      priorityClassName: system-node-critical
      tolerations:
        - operator: Exists
      containers:
        - name: generic-device-plugin
          image: {DEVICE_PLUGIN_IMAGE}
          args:
            - --domain
            - devic.es
            - --device
            - |
              name: dri
              groups:
                - count: {SHARES}
                  paths:
                    - path: {RENDER_DEVICE}
          securityContext:
            privileged: true
          volumeMounts:
            - name: device-plugin
              mountPath: /var/lib/kubelet/device-plugins
            - name: dev
              mountPath: /dev
      volumes:
        - name: device-plugin
          hostPath:
            path: /var/lib/kubelet/device-plugins
        - name: dev
          hostPath:
            path: /dev
---
apiVersion: v1
kind: LimitRange
metadata:
  name: infer-gpu
  namespace: {namespace}
spec:
  limits:
    - type: Container
      default:
        {RESOURCE}: "1"
      defaultRequest:
        {RESOURCE}: "1"
"#
    )
}

/// What Podman on this Mac looks like to someone about to run a GPU cluster
/// on it. `machine_type` is the provider of the machine that runs.
#[derive(Debug, Default)]
struct PodmanSetup {
    podman: bool,
    krunkit: bool,
    machines: usize,
    machine_type: Option<String>,
    rootful: bool,
}

/// The first thing missing for a GPU cluster on Podman, with the command
/// that fixes it. None when everything is there.
fn setup_problem(setup: &PodmanSetup) -> Option<String> {
    if !setup.podman {
        return Some(format!(
            "Podman is not installed. Install it: {INSTALL_PODMAN}"
        ));
    }
    if !setup.krunkit {
        return Some(format!(
            "krunkit is not installed, which is what gives a Podman machine the GPU. Install it: {INSTALL_KRUNKIT}"
        ));
    }
    match setup.machine_type.as_deref() {
        None if setup.machines == 0 => Some(format!(
            "Podman has no machine. Create one with the GPU: {CREATE_MACHINE}"
        )),
        None => Some("The Podman machine is not running. Start it: podman machine start".into()),
        Some(provider) if provider != LIBKRUN => Some(format!(
            "The Podman machine runs on {provider}, which has no GPU. Replace it, which deletes its containers: podman machine rm --force && {CREATE_MACHINE}"
        )),
        Some(_) if !setup.rootful => Some(
            "The Podman machine is rootless, and k3s needs a rootful one. Switch it: podman machine stop && podman machine set --rootful && podman machine start"
                .into(),
        ),
        Some(_) => None,
    }
}

/// The provider of the machine that runs and how many machines there are,
/// from `podman machine list` printing a provider and `true` or `false` each.
fn machines_of(listing: &str) -> (Option<String>, usize) {
    let machines: Vec<(&str, &str)> = listing
        .lines()
        .filter_map(|line| line.trim().split_once('\t'))
        .collect();
    let running = machines
        .iter()
        .find(|(_, running)| *running == "true")
        .map(|(provider, _)| (*provider).to_owned());
    (running, machines.len())
}

fn podman(args: &[&str]) -> Result<String, String> {
    crate::local_cluster::run(Runtime::Podman, args)
}

fn krunkit_installed() -> bool {
    crate::env::command("krunkit")
        .env("PATH", crate::env::composed_path())
        .arg("--version")
        .output()
        .is_ok()
}

/// Why this Mac cannot run a GPU cluster on Podman yet, with the command that
/// fixes it.
pub(crate) fn podman_problem() -> Option<String> {
    let listing = podman(&["machine", "list", "--format", "{{.VMType}}\t{{.Running}}"]);
    let (machine_type, machines) = machines_of(listing.as_deref().unwrap_or_default());
    setup_problem(&PodmanSetup {
        podman: podman(&["--version"]).is_ok(),
        krunkit: krunkit_installed(),
        machines,
        machine_type,
        rootful: podman(&["info", "--format", "{{.Host.Security.Rootless}}"])
            .is_ok_and(|rootless| rootless.trim() == "false"),
    })
}

/// A computer image the node already holds, which has the tool that asks the
/// GPU. None until a computer ran there.
fn probe_image(images: &str) -> Option<&str> {
    images
        .lines()
        .map(str::trim)
        .find(|image| image.contains(COMPUTER_IMAGE) && !image.contains("@sha256:"))
}

/// Whether the GPU answers, from a run of `vulkaninfo` on the node under a
/// timeout. A GPU whose device stopped answering blocks it for good.
fn probe_prerequisite(exit_code: Option<i32>, output: &str) -> Prerequisite {
    if exit_code.is_some_and(|code| TIMED_OUT.contains(&code)) {
        return prerequisite(GPU_NAME, false, HUNG);
    }
    prerequisite(
        GPU_NAME,
        output.contains(VENUS),
        "No computer finds the Mac's GPU. Check the Podman machine runs on libkrun, then provision the local cluster again",
    )
}

/// A name no earlier probe has, since one that was killed leaves its
/// container on the node.
fn probe_name() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    format!("infer-gpu-probe-{}", now.unwrap_or_default().as_millis())
}

/// Runs `vulkaninfo` on the node. The time limit kills, because a call
/// blocked on the GPU does not end when it is asked to.
fn probe(image: &str) -> Prerequisite {
    let output = crate::local_cluster::node_command(Runtime::Podman)
        .args(["timeout", "-s", "KILL", PROBE_SECONDS])
        .args(["ctr", "-n", "k8s.io", "run", "--rm"])
        .args(["--device", RENDER_DEVICE, image, &probe_name()])
        .args(["vulkaninfo", "--summary"])
        .output();
    match output {
        Ok(output) => probe_prerequisite(
            output.status.code(),
            &String::from_utf8_lossy(&output.stdout),
        ),
        Err(error) => prerequisite(GPU_NAME, false, &format!("podman could not run: {error}")),
    }
}

/// Whether the GPU of the cluster on Podman answers. Nothing to say before
/// the first computer ran, since only its image can ask.
pub(crate) fn prerequisites() -> Vec<Prerequisite> {
    let images = crate::local_cluster::node_command(Runtime::Podman)
        .args(["ctr", "-n", "k8s.io", "images", "ls", "-q"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    probe_image(&images).map(probe).into_iter().collect()
}

/// Whether the node offers the GPU yet, which a computer needs to be
/// scheduled. The device plugin registers it a moment after it is applied.
pub(crate) fn offered() -> Result<String, String> {
    let amount = crate::workspace_settings::kubectl(
        crate::local_cluster::CONTEXT,
        &["get", "nodes", "-o", ALLOCATABLE],
    )?;
    match amount.trim() {
        "" | "0" => Err("the node offers no GPU yet".into()),
        _ => Ok(amount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_norway::Value;

    fn ready() -> PodmanSetup {
        PodmanSetup {
            podman: true,
            krunkit: true,
            machines: 1,
            machine_type: Some(LIBKRUN.into()),
            rootful: true,
        }
    }

    #[test]
    fn the_manifest_offers_the_render_device_and_makes_it_the_namespaces_default() {
        let manifest = manifest("agents");
        let documents: Vec<Value> = manifest
            .split("---\n")
            .map(|document| serde_norway::from_str(document).unwrap())
            .collect();
        let plugin = &documents[0]["spec"]["template"]["spec"]["containers"][0];
        assert!(plugin["args"][3].as_str().unwrap().contains(RENDER_DEVICE));
        assert_eq!(documents[1]["kind"], "LimitRange");
        assert_eq!(documents[1]["metadata"]["namespace"], "agents");
        assert_eq!(documents[1]["spec"]["limits"][0]["default"][RESOURCE], "1");
    }

    #[test]
    fn each_missing_piece_of_the_podman_setup_names_its_fix() {
        assert_eq!(setup_problem(&ready()), None);
        let problem = |setup: PodmanSetup| setup_problem(&setup).unwrap();
        let no_podman = PodmanSetup::default();
        assert!(problem(no_podman).contains(INSTALL_PODMAN));
        let no_krunkit = PodmanSetup {
            krunkit: false,
            ..ready()
        };
        assert!(problem(no_krunkit).contains(INSTALL_KRUNKIT));
        let no_machine = PodmanSetup {
            machines: 0,
            machine_type: None,
            ..ready()
        };
        assert!(problem(no_machine).contains(CREATE_MACHINE));
        let stopped = PodmanSetup {
            machine_type: None,
            ..ready()
        };
        assert!(problem(stopped).contains("podman machine start"));
        let applehv = PodmanSetup {
            machine_type: Some("applehv".into()),
            ..ready()
        };
        assert!(problem(applehv).contains("applehv, which has no GPU"));
        let rootless = PodmanSetup {
            rootful: false,
            ..ready()
        };
        assert!(problem(rootless).contains("podman machine set --rootful"));
    }

    #[test]
    fn the_running_machine_gives_the_provider() {
        assert_eq!(
            machines_of("applehv\tfalse\nlibkrun\ttrue\n"),
            (Some("libkrun".into()), 2)
        );
        assert_eq!(machines_of("libkrun\tfalse\n"), (None, 1));
        assert_eq!(machines_of(""), (None, 0));
    }

    #[test]
    fn a_probe_that_times_out_is_a_gpu_that_stopped_answering() {
        for code in TIMED_OUT {
            let hung = probe_prerequisite(Some(code), "");
            assert_eq!(hung, prerequisite(GPU_NAME, false, HUNG));
        }
        let answers = "deviceName = Virtio-GPU Venus (Apple M4 Pro)\ndeviceName = llvmpipe\n";
        assert_eq!(
            probe_prerequisite(Some(0), answers),
            prerequisite(GPU_NAME, true, "")
        );
        let software = probe_prerequisite(Some(0), "deviceName = llvmpipe\n");
        assert_ne!(software, prerequisite(GPU_NAME, true, ""));
    }

    #[test]
    fn the_probe_runs_on_a_computer_image_the_node_holds() {
        let images = "docker.io/rancher/mirrored-pause:3.6\nghcr.io/inference-gateway/computer@sha256:abc\nghcr.io/inference-gateway/computer:0.256.0\n";
        assert_eq!(
            probe_image(images),
            Some("ghcr.io/inference-gateway/computer:0.256.0")
        );
        assert_eq!(probe_image("docker.io/rancher/mirrored-pause:3.6\n"), None);
    }
}
