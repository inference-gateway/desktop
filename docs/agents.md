# Agents and computers

## Your own agents

The sidebar's **Agents** filter lists your own agents: Markdown files (`<name>.md`, the Claude Code / infer format - frontmatter `name`, `description`, optional `model`, plus a desktop-only `emoji`, and the instructions as the body) under `~/.infer/agents`, or the directory set in Settings > Agents.

**New agent** opens an editor for the name, description, face, model and instructions. Every agent starts with a robot drawn from its name, and you can pick one of Microsoft's animated 3D Fluent emoji (downloaded beside the agent file) or your own GIF or picture. Saving drops you into a chat with that agent: its instructions become the chat's system prompt.

Agents in `~/.infer/agents` can also hand work to each other through the CLI's `Agent` tool, so one agent's instructions can tell it to consult another by name.

## A computer of their own

An agent can also get a computer of its own: add the compute under **Settings > Workspaces** (a Kubernetes cluster for now, saved as a workspace of the `infer` daemon in `~/.infer/daemon.yaml`), pick it as the **Computer** in the agent's editor (`workspace` in the frontmatter) and every new chat with it is teleported there before its first run, so its computer use drives that machine, not yours.

**Computers** in the top bar shows the latest screen of each such chat side by side. **Take over** on a computer, or in its agent's chat, holds the agent and lets you drive that screen with your own mouse and keyboard - a login, a captcha, a dialog in its way - until **Hand back** lets the agent continue from what it sees.

**Settings > Workspaces** lists the machines each workspace has in its cluster, ready or not, and deletes any one of them - a machine that fails to boot shows there as not ready. The chat of a deleted machine starts a new one on its next message.

A chat that already started can move too: **Move to workspace** in its composer sends it to a workspace or brings it back to this computer, between turns.

A Kubernetes workspace sets the **CPU** and **memory** of its computers, each as a request and a limit, in Kubernetes quantities such as `500m`, `2` or `4Gi`. They are saved as the `resources` block of the workspace in `daemon.yaml` and need `infer` CLI v0.255.0 or newer. Empty fields leave the amounts to the cluster. A running computer keeps what it started with, and the next one gets the new values. The **Computers** view shows what each computer was given - its CPUs, its memory and its GPUs - and whether it is a sandbox, a pod of its own in a cluster, or a real system.

A workspace's **User data** is a cloud-init script each of its computers runs as root when it boots, before the agent starts, to install what the agents need. The local cluster's workspace runs the CLI's Ubuntu desktop image, `ghcr.io/inference-gateway/computer`, and its user data installs Firefox with the opentask extension for [browser use](browser-use.md#on-an-agents-computer). A computer boots to an empty desktop - the browser stays closed until an agent opens it for a task that needs the web.

See [examples/computer-use](../examples/computer-use) for three agents on Ubuntu desktops in a local Kubernetes cluster.
