# MCP agent

One `repo-guide` agent answers questions about open-source projects through an MCP server, so you can ask about a repo you have not checked out.

It uses [DeepWiki](https://deepwiki.com), a public hosted MCP server at `https://mcp.deepwiki.com/mcp` that needs no account and no key: for a GitHub repository it reads the wiki pages DeepWiki builds from that code and answers pointed questions about it. This example adds the server under **Settings > MCPs**, adds the agent, whose frontmatter picks the server under a `mcpServers` list, and sends a prompt to try - and a second section repeats the setup for a second server that does need a key, to show a header whose value is a secret.

## What you need

- The app with a model picked in the top bar. DeepWiki answers over HTTPS, and the agent works in your project directory, so nothing else is needed - no Docker, no workspace, no provider keys beyond the model's.

## Set up

```bash
task up     # the repo-guide agent in ~/.infer/agents - the server itself you add in the app
```

Then, under **Settings > MCPs**:

1. Press **New hosted server**. Name it `deepwiki` and give it the URL `https://mcp.deepwiki.com/mcp`. Leave the headers empty - DeepWiki needs none - tick **This computer** under **Goes to** and press **Save server**.
2. The row reads `Goes to this computer`. Press the refresh button, **Check MCP servers**, and the row turns `Connected, N tools` with the tool count DeepWiki offers.
3. Select the agent: open the **Agents** filter and select `repo-guide`, so the composer reads **Message repo-guide...**

The agent's frontmatter picks the server - with no server picked, the agent would use every server of its computer (see [MCP servers](../../docs/mcp.md) and [Agents and computers](../../docs/agents.md)):

```markdown
---
name: repo-guide
emoji: 📖
mcpServers:
  - deepwiki
---
```

## Run it

Send:

```text
How does the tauri updater sign and verify a release? Ask the deepwiki server about tauri-apps/tauri.
```

A tool call to the deepwiki server appears in the transcript, then the answer: the CLI names a server's tools after it, and DeepWiki answers with tools such as `ask_question`. Follow up on the same repo without naming it again - `And where does the app get its public key from?` - and try another project by name. When you mention the server in the prompt, the agent reaches for it directly.

The agent answers only what its servers can answer: ask it something no wiki covers, like what your own project should do, and it says so instead of guessing - which is the point of the `mcpServers` pick.

## A server that needs a key

Most hosted MCP servers need a key, and [AgentMail](https://www.agentmail.to) is an easy one: it gives an agent an email inbox and its MCP server reads an API key from the `x-api-key` header. Add it the same way, plus a secret:

1. Create an account at [agentmail.to](https://www.agentmail.to) and copy the API key it shows.
2. Under **Settings > Secrets** press **New secret**: label it `agentmail`, name its variable `AGENTMAIL_API_KEY`, paste the key, tick **This computer** and press **Save secret**.
3. Under **Settings > MCPs** press **New hosted server**: name it `agentmail`, give it the URL `https://mcp.agentmail.to/mcp`, press **Add header** and fill the row - name `x-api-key`, prefix left empty, secret `AGENTMAIL_API_KEY` from the dropdown. Tick **This computer** and press **Save server**.
4. In the agent editor (`Edit agent repo-guide` on the agent's card) tick `agentmail` beside `deepwiki` - the frontmatter lists both servers once saved.

In `~/.infer/mcp.yaml` the header is saved as a reference, and the key itself stays in the secret:

```yaml
headers:
  x-api-key: ${AGENTMAIL_API_KEY}
```

**Check MCP servers** again and the agentmail row turns `Connected, N tools` too. Then send:

```text
Do I have new mail? Check with the agentmail server.
```

A tool call to the agentmail server appears in the transcript with what it found. Mail that exists sits in the real AgentMail inbox of your account - sending is a real action, so keep senders to people you trust when you try it.

## Clean up

```bash
task down   # the repo-guide agent, out of ~/.infer
```

`task down` removes only the agent from this example. Remove the two servers under **Settings > MCPs** and the secret under **Settings > Secrets** - DeepWiki keeps no account of yours, AgentMail does, so close that one at agentmail.to if you are done with it.
