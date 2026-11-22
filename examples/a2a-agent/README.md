# A2A agent

A chat hands a task to an [A2A](https://a2a-protocol.org) agent and gets its answer back. The agent here is [mock-agent](https://github.com/inference-gateway/mock-agent), served on `http://localhost:8085`: it speaks the real A2A protocol but thinks with a mock model, so it needs no key and spends no tokens. You add it under **Settings > Agents** as a remote agent and send one prompt - see [A2A agents](../../docs/agents.md#a2a-agents) for how they differ from your Markdown agents.

## What you need

- `task`, `curl` and Docker, or Podman with `RUNTIME=podman` below. Nothing else - the agent runs from its published image.

## Try it token-free

From the checkout:

```bash
DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=examples/a2a-agent/scenarios.yaml task dev
```

The chat's own turns come from [scenarios.yaml](scenarios.yaml), while the task really goes to the agent and its reply is the agent's own. For a real run, start the app normally with a model picked in the top bar - the chat then words its turns itself, and the agent's reply stays a mock one.

## Set up

```bash
task up     # mock-agent 0.5.3 on http://localhost:8085
```

`task up RUNTIME=podman` runs it on Podman instead. Then, under **Settings > Agents**, type `http://localhost:8085` into the **Remote agents** field and press **Add** - a row with the URL appears. Every chat is now told about the agent.

## Run it

Send:

```text
Ask the mock agent at http://localhost:8085 to check it is reachable.
```

The chat calls `A2A_SubmitTask` with the agent's URL and a task for it. In Standard mode the call asks first - press **Approve**, or move the status bar's mode to Auto. The task goes to the agent, which answers with its mock reply:

```text
This is a mock response to: "Task completed successfully. I executed the requested operation based on: Check you are reachable: echo the message hello from the desktop.". I'm a mock agent designed for testing purposes.
```

Once the task is done its result comes back to the chat, and the chat answers that the agent is reachable.

## Clean up

```bash
task down   # stops the agent - task down RUNTIME=podman if you started it on Podman
```

`task down` only stops the container. Press **Remove** on the `http://localhost:8085` row under **Settings > Agents > Remote agents** to take the agent out of every chat.
