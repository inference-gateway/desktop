# The Mac's GPU on the local cluster

The local cluster under **Settings > Compute** is k3s in a container. Its computers reach this Mac's GPU when that container runs on Podman instead of Docker, because Docker's Linux VM has no GPU and a Podman machine on the `libkrun` provider has one.

## Set it up

```bash
brew install podman
brew tap slp/krunkit && brew install krunkit
CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --rootful --now
```

Then, under **Settings > Compute**:

1. Under **Local cluster runtime**, choose **Podman**. When something is missing - Podman, krunkit, a machine on `libkrun`, a rootful machine - the tab says what, with the command that fixes it.
2. Press the container button, **Provision local cluster**. The cluster's row then reads `Kubernetes on Podman with GPU`.
3. Press **Restart CLI**.

The choice is saved in `~/.infer/cluster_folders.yaml`, beside the cluster's folders. A cluster that already runs on the other runtime moves only when it is recreated, which deletes its machines and everything on them. The tab asks before it does that, the same way as for changed folders. A Mac that stays on Docker keeps its cluster as it is.

## What a computer gets

Provisioning installs a device plugin on the node that offers the GPU's render device, `/dev/dri/renderD128`, as the resource `devic.es/dri`. A default in the cluster's namespace gives it to every computer of every workspace there, so nothing has to be switched on. The **Computers** view marks each computer that has it with **GPU**.

A workspace on the cluster has a **GPU** switch in its form. Switched off, the workspace's new computers go without the device and render on the CPU. It needs `infer` CLI v0.255.0 or newer.

The computer image carries Vulkan with a Mesa Venus driver that works on an Apple silicon host, from `infer` CLI v0.256.0 on. An app finds the Apple GPU as its first Vulkan device and the software renderer `llvmpipe` as the second. To check, run `vulkaninfo --summary` on a computer - it lists `Virtio-GPU Venus` with the name of the Mac's chip.

## What it speeds up and what it does not

| | With the GPU |
| --- | --- |
| Vulkan compute and rendering | Yes. Gyroflow stabilised 150 frames of 4K in about 7 seconds, and had finished 18 frames after 10 minutes without it |
| OpenGL | No. It stays on `llvmpipe` |
| Video encoding and decoding | No. No Linux guest on a Mac reaches Apple's hardware codecs, so both stay on the CPU |

OpenGL was tried two ways on a computer with the GPU. Mesa's own driver for the device fails to start and falls back to `llvmpipe`. Zink, which runs OpenGL on Vulkan, was forced with `MESA_LOADER_DRIVER_OVERRIDE=zink` and refuses the device, because it lacks a feature Zink requires (`nullDescriptor`).

## Apps that do not use it

FilmCraft 0.19.0 picks the software renderer although the GPU is there. With the GPU allowed, an export of 300 frames ran at 4.5 frames a second and took 17.5 GB of memory, against 6.8 frames a second and 5.7 GB on software rendering. Most of its work is decoding and encoding video, which the GPU does not do here. Keep it on software rendering - switch **GPU** off on its workspace, or start it with `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json LIBGL_ALWAYS_SOFTWARE=1`.

## When the GPU stops answering

The virtual GPU is young. In one run its device stopped answering about 50 minutes after the VM started, and every app that held it blocked until the VM was restarted.

The cluster check under **Settings > Compute** asks the GPU with a time limit, using a computer image the node already holds. When the GPU does not answer in time, the check says so, instead of leaving chats to hang:

```bash
podman machine stop && podman machine start
```

Until then, switch **GPU** off on the workspace and its new computers run on the CPU.
