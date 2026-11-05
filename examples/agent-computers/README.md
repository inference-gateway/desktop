# Agent computers

Three agents, each on an Ubuntu desktop of its own, working in parallel while you watch their screens from the app.

Every chat with an agent that has a `workspace` is teleported to that workspace of the `infer` daemon before its first run. The `ubuntu-desktop` workspace here is a Kubernetes one, so each chat gets its own pod - Xvfb, openbox and Firefox - and the app's **Computers** view shows the latest screen of each.

Needs `docker`, `ctlptl`, `k3d`, `kubectl`, `yq`, `task` and `infer` v0.244.0 or newer. Runs on a mock model - no API key, no tokens.

```bash
task up     # cluster, image, workspace in ~/.infer/daemon.yaml, agents in ~/.infer/agents
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

`task up` keeps every other entry of `~/.infer/daemon.yaml` and saves the previous file as `daemon.yaml.bak`.

For a real model, drop the two `INFER_GATEWAY_MOCK*` lines from `workspace.yaml`, set `INFER_AGENT_MODEL` there and in the agents' `model`, and list the provider key under `gateway_env`. A chat started from `task dev` teleports the desktop checkout itself, as that is the directory it runs in.
