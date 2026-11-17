# The local cluster on Apple containers

The local cluster under **Settings > Compute** is k3s in a container. On a Mac it can run on Apple's own [`container`](https://github.com/apple/container) instead of Docker or Podman. Each container is a lightweight VM of its own on Virtualization.framework, so there is no Docker Desktop and no Podman machine to keep running.

## What it needs

- macOS 26 or newer on Apple silicon
- `container` 1.7 or newer, with a Linux kernel set

```bash
brew install container
container system start
container system kernel set --recommended
```

The signed installer package from the [releases page](https://github.com/apple/container/releases) works too, and brings its own kernel.

Then, under **Settings > Compute**:

1. Under **Local cluster runtime**, choose **Apple containers**. When something is missing - `container`, its service, the kernel - the tab says what, with the command that fixes it.
2. Press the container button, **Provision local cluster**. The cluster's row then reads `Kubernetes on Apple containers`.
3. Press **Restart CLI**.

The choice is saved in `~/.infer/cluster_folders.yaml`, beside the cluster's folders. A cluster that already runs on another runtime moves only when it is recreated, which deletes its machines and everything on them. The tab asks before it does that, the same way as for changed folders.

## How the node runs

The node is the same k3s image as on Docker, started with `container run --cap-add ALL --read-only-path NONE` in place of `--privileged`, because k3s needs every capability and a writable `/proc/sys`. Its API is published on a port of `127.0.0.1`, which the `infer-k3s` kubeconfig context points at, and the folders of **Settings > Compute** reach the node as virtiofs bind mounts, read-only ones included.

The node's VM gets 4 CPUs and 8 GiB of memory, up from the 1 GiB a container gets by default, so the operator, the gateway, agent-sandbox and a computer fit. The VM holds that memory while it runs.

## What differs from Docker and Podman

- **No GPU.** The VM has no `/dev/dri`, so computers render on the CPU, as on Docker. The workspace form has no **GPU** switch and the **Computers** view shows no **GPU** mark. Podman stays the runtime for [the Mac's GPU](gpu.md).
- **No automatic restart.** `container` has no restart policy, so the node does not come back after a Mac reboot or `container system stop`. Press **Provision local cluster** again - it starts the node that is there, with its machines and secrets, instead of creating a new one.
- **The node's address changes** on every start, and k3s exits when its node address changes. So the node keeps a fixed address on its loopback, `10.200.0.1`, and only advertises the API on the address it got. The kubeconfig points at `127.0.0.1` either way.

## Why not `container cluster create`

Since 1.5.0 `container` ships an experimental Kubernetes plugin: `container cluster create` boots a kind node with kubeadm and merges a context into `~/.kube/config`. It was looked at and not used, because it has no way to mount folders into the node, so the folders of **Settings > Compute** could not reach a computer, a cluster whose control-plane node stopped has to be deleted and created again, which deletes its machines, the node is kind rather than k3s, so the manifests and readiness checks would differ from the other two runtimes, and the command is experimental, with subcommands removed between minor versions. Worth another look once it gains mounts.
