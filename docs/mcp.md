# MCP servers

An MCP server hands the agent extra tools - send mail, look up docs, query a service - beside its file and shell tools. The servers live in `~/.infer/mcp.yaml`, and **Settings > MCPs** manages them: it saves a server as written, keeping every key it does not know about (a `connection_timeout`, `include_tools`, a container's `volumes`) and leaving a `${VARIABLE}` reference exactly as written. New chats pick up a change; a running chat keeps the servers it started with.

## The switch

**Use MCP servers on this computer** (`mcp-enabled`) is the master switch, saved under `enabled` at the top of `~/.infer/mcp.yaml`. When it is off, no chat on this computer connects to any server, whatever the list below holds - every row reads `MCP is off`.

## Hosted and container servers

**New hosted server** adds one that answers over HTTP at a URL, typically someone else's: a **Name** (`mcp-name`) and a **URL** (`mcp-url`, which must start with `http://` or `https://`). The name is lowercase letters, digits, `-` and `_` - the CLI names the server's tools after it, so the name must fit in a tool name.

**New container server** adds one this computer runs itself in Docker: a **Name** (`mcp-name`), an **Image** (`mcp-image`, such as `mcp/filesystem:latest`) and the **Port** (`mcp-port`) the image serves on. It is saved as `run: true` with `oci` and `port`, and the app starts or stops its container to match the **This computer** tick below - when you save, and once more when the app starts. Deleting a server stops its container first. A container server runs here only; it cannot move to a workspace, and an existing server cannot switch between hosted and container.

In `~/.infer/mcp.yaml` both look like this:

```yaml
enabled: true
servers:
  - name: deepwiki
    enabled: true
    run: false
    url: https://mcp.deepwiki.com/mcp
  - name: filesystem
    enabled: true
    run: true
    oci: mcp/filesystem:latest
    port: 3000
```

## Headers

A hosted server that authenticates gets headers. **Add header** appends one row to the form: the header's **name** (say `x-api-key` or `Authorization`), an optional **prefix** (`Bearer`), and the **secret** picked from a dropdown of the variables the secrets in **Settings > Secrets** and your API keys provide. Saved to `~/.infer/mcp.yaml`, the value is a reference of the form `<prefix>${VARIABLE}`:

```yaml
headers:
  Authorization: Bearer ${GITHUB_TOKEN}
```

The reference is written as it is, never expanded here - each computer expands it from the secret it was given, so the key itself stays out of the file. A header whose value was written by hand and is not a `${VARIABLE}` reference is kept as written, but the tab never shows its value; pick a secret to replace it. Every header must expand a variable some secret or API key provides, and two headers cannot share a name.

## Where a server goes

Under **Goes to**, every server has a **This computer** tick (`mcp-computer`) - whether chats on your machine connect to it - and one tick per Kubernetes workspace, `mcp-workspace-<name>`, which lists the server on that workspace's `mcp_servers` in `~/.infer/daemon.yaml`. A workspace's computers get the server once the `infer` CLI and the operator pass MCP servers to their pods, so give the workspace a secret for each header under **Settings > Secrets** as well. A server ticked nowhere reads `Goes nowhere`.

## When a change takes effect

New chats pick up a change, running chats keep what they started with. Changing where a server goes rewrites `daemon.yaml` instead, and the app then asks for a **Restart CLI**: the daemon hands servers to workspaces when it starts, so until it restarts a workspace keeps the servers it had.

## Checking

The refresh button, **Check MCP servers**, asks `infer mcp status` again and shows each row's state next to its name: `Connected` with its tool count, `Not connected` with the error, `Off on this computer`, or `MCP is off`. A row also warns when a place the server goes gets no secret for one of its header variables - `workspace support gets no AGENTMAIL_API_KEY` - which you add under **Settings > Secrets**.

## Per agent

Each agent picks its servers in the agent editor's **MCP servers** list, one checkbox per server (`agent-mcp-<server>`), saved as `mcpServers` in the agent's file. An agent that picks none uses every server of its computer - see [Agents and computers](agents.md). See [examples/mcp-agent](../examples/mcp-agent) for an agent set up against a hosted server.
