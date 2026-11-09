# Agents and computers

## Your own agents

The sidebar's **Agents** filter lists your own agents: Markdown files (`<name>.md`, the Claude Code / infer format - frontmatter `name`, `description`, optional `model`, plus a desktop-only `emoji`, and the instructions as the body) under `~/.infer/agents`, or the directory set in Settings > Agents.

**New agent** opens an editor for the name, description, face, model and instructions. Every agent starts with a robot drawn from its name, and you can pick one of Microsoft's animated 3D Fluent emoji (downloaded beside the agent file) or your own GIF or picture. Saving drops you into a chat with that agent: its instructions become the chat's system prompt.

Agents in `~/.infer/agents` can also hand work to each other through the CLI's `Agent` tool, so one agent's instructions can tell it to consult another by name.

## A computer of their own

An agent can also get a computer of its own: add the compute under **Settings > Workspaces** (a Kubernetes cluster for now, saved as a workspace of the `infer` daemon in `~/.infer/daemon.yaml`), pick it as the **Computer** in the agent's editor (`workspace` in the frontmatter) and every new chat with it is teleported there before its first run, so its computer use drives that machine, not yours.

**Computers** in the top bar shows the latest screen of each such chat side by side. **Take over** on a computer, or in its agent's chat, holds the agent and lets you drive that screen with your own mouse and keyboard - a login, a captcha, a dialog in its way - until **Hand back** lets the agent continue from what it sees.

A chat that already started can move too: **Move to workspace** in its composer sends it to a workspace or brings it back to this computer, between turns.

A workspace's **User data** is a cloud-init script each of its computers runs as root when it boots, before the agent starts, to install what the agents need. The local cluster's workspace runs the CLI's Ubuntu desktop image, `ghcr.io/inference-gateway/computer`, and its user data installs Firefox with the opentask extension for [browser use](browser-use.md#on-an-agents-computer). A computer boots to an empty desktop - the browser stays closed until an agent opens it for a task that needs the web.

See [examples/computer-use](../examples/computer-use) for three agents on Ubuntu desktops in a local Kubernetes cluster.
