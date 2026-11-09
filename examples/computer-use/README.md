# Computer use

Three agents, each on an Ubuntu desktop of its own, working in parallel while you watch their screens from the app.

Every chat with an agent that has a `workspace` is teleported to that workspace of the `infer` daemon before its first run. The `ubuntu-desktop` workspace here is a Kubernetes one, so each chat gets its own pod of the CLI's computer image, `ghcr.io/inference-gateway/computer` - an Ubuntu desktop with a taskbar - and the app's **Computers** view shows the latest screen of each.

The image ships no browser. The workspace's `user_data`, a cloud-init script each pod runs as root when it boots, installs Firefox with the [opentask](https://github.com/inference-gateway/opentask) extension, so the agents also browse with the browser tools. It is the app's own computer setup, [`backend/src/computer_user_data.sh`](../../backend/src/computer_user_data.sh), plus the mock model's scenarios from [`mock-scenarios.sh`](mock-scenarios.sh). The first boot of a computer takes about a minute longer while it installs.

Needs `docker`, `ctlptl`, `k3d`, `kubectl`, `yq`, `task` and `infer` v0.251.0 or newer. The cluster runs Kubernetes 1.37, as each pod boots in a user namespace, and operator v0.35.0. Runs on a mock model - no API key, no tokens - or on [a real one](#a-real-model).

```bash
task up     # cluster, workspace in ~/.infer/daemon.yaml, agents in ~/.infer/agents
```

Then in the app:

1. Press **Restart CLI** so the daemon reads the new workspace.
2. Open the **Agents** filter and send each agent its task, one after the other:
   - `researcher` - `research example.com`
   - `writer` - `write about ubuntu`
   - `tester` - `test example.org`
3. Open **Computers** in the top bar to watch all three. Click a tile to open its chat.

```bash
task pods   # one Orchestrator and pod per chat
task down   # delete the cluster, the workspace entry and the agents
```

`task up` keeps every other entry of `~/.infer/daemon.yaml` and saves the previous file as `daemon.yaml.bak`. It stops before touching anything if `~/.infer/agents` already holds a different `researcher`, `writer` or `tester`, and `task down` only removes the copies it installed.

## A real model

```bash
task up MODEL=ollama_cloud/glm-5.3-flash
```

`MODEL` is the `provider/model` the agents and the workspace run on, and it has to read screenshots. The provider's key is the one you already have: the workspace lists `<PROVIDER>_API_KEY` under `gateway_env`, the daemon reads its value from its own environment or from `~/.infer/auth.yaml`, and it lands in the gateway's Secret, never in the agent's pod. Press **Restart CLI**, then give the agents tasks of your own. `task up` without it goes back to the mock model.

The agents use their computer without asking for approval - it is a pod of their own, not your machine.

A chat started from `task dev` teleports the desktop checkout itself, as that is the directory it runs in.
