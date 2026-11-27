# Chat

A chat runs the `infer` agent in the directory of the project you picked from the sidebar (see [How it works](how-it-works.md)). The bar above the composer holds the chat's model picker, read-only for an agent that fixes its own model. Its **Choose project** moves an open chat to another project, or out of every project, with its history - as dragging the chat onto a project in the sidebar does. A running chat cannot move, and a chat on another computer keeps its project. This page documents the controls the chat itself puts in front of you: the four agent modes, plans and their approvals, the composer and its prefixes, and running several chats at once.

Every prompt on this page is answered by a scenario in `e2e/scenarios.yaml`, so you can try each one without provider tokens in mock mode: `DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev`.

## Agent modes

The status bar's mode button shows the chat's current agent mode. There are four, in the order Shift+Tab cycles them:

- **Standard mode** - protected tool actions ask first: a protected tool call pauses and shows an approval card before it runs.
- **Plan mode** - the agent plans before it changes anything: it works out a plan and hands it to you for a decision instead of acting (see [Plans and approvals](#plans-and-approvals)).
- **Auto mode** - tool actions run without asking.
- **Auto with judge mode** - a judge model decides protected tool actions, so nothing waits for you.

Clicking the mode button moves the chat on to the next mode, and Shift+Tab in the composer does the same: Standard, Plan, Auto, Auto with judge, back to Standard. Standard mode is invisible in the button (it is the default), so anything the button shows means the chat is not in Standard mode.

A mode belongs to its chat, not to the app: switching modes in one chat leaves every other chat alone, and a fresh chat opens in Standard mode again.

**Try in mock mode:** click the mode button into Auto mode and send `create a file named auto-approve.txt` - the Write runs with no approval card. In a fresh chat the same prompt pauses for approval, because the mode stayed behind with the old chat.

## Plans and approvals

In Plan mode the agent submits its plan instead of acting on it. The plan card shows the plan itself and three choices:

- **Accept** - the plan is approved and the chat continues in Auto mode: the plan's steps run without another approval card.
- **Approve each step** - the chat continues in Standard mode: the plan runs, and every protected step still shows its approval card first.
- **Reject** - nothing changes: the chat stays in Plan mode and keeps planning, and nothing ran.

Outside plan mode, in Standard mode, a protected tool call pauses with an **Approve** / **Deny** card. Press `a` to approve or `d` to deny. An approved call runs. A denied one does not - the denial is reported and the run ends, since a denied tool call never gets a follow-up turn.

**Try in mock mode:** click the mode button into Plan mode and send `plan a file named planned.txt` - the plan card appears with the plan on it. **Accept** writes the file in Auto mode, **Approve each step** asks once per protected step, and **Reject** sends the agent back to planning without writing anything.

## The composer

Enter sends. Shift+Enter adds a newline. A prefix at the start of the prompt changes what sending does, and three of them open menus while you type:

- `@` opens the file and agent menu: the project directory's files, then the agents, narrowed as you type across the menu's first 50 rows. Arrows move, Enter picks, Escape closes. A picked file lands in the prompt as `@<path>` and a picked agent as `@<name>`, and an agent row shows the agent's avatar.
- `#` opens the open-issue menu: the project's open GitHub issues as `#<number> <title>`, narrowed as you type. Picking one inserts `#<number>`.
- `/` opens the shortcut menu: the CLI's shortcut commands and the skills, narrowed as you type. A skill not on this machine yet is labelled _remote skill_ - picking one asks whether to download it first.
- `!command` is bash mode: the prompt runs as a shell command in the workspace without a model turn, and the command's output lands in the conversation. Typing `!` inserts the space after it for you.
- `!!` is tool mode: a menu of the agent's tools appears and Enter runs the picked tool call directly, without asking the model.

### The trigger table

Which menus the typing opens is not hard-coded, it is read from the CLI: `~/.infer/composer.yaml` (see the CLI's configuration reference) maps trigger characters to menus per context, and the desktop takes the CLI's resolved table for a chat as it is, just like the terminal's own composer. A trigger is any one special character, punctuation or symbol, ASCII or not, so a mapping such as `§` to the issues menu works the same as a built-in one. Every trigger of the table opens its sources in order, each section `files`, `agents` or `issues`, and every row inserts its own sigil: `@<path>`, `@<name>` and `#<number>`, whichever trigger opened the menu. The same rule as the TUI applies: the trigger starts the text or follows whitespace and does not recur in the token, so an email address, `C#` or a URL fragment opens nothing. The resolved table is used as is, never merged with the built-ins a newer or older CLI would bring, so a trigger the file leaves out of a context opens nothing there. An example `composer.yaml`:

```yaml
triggers:
  chat:
    "@": [files, agents]
    "^": [issues]
```

With this file `@` keeps the files and agents and `^` takes over the issues, while `#` opens nothing in a chat any more, since a context the file sets replaces that context's defaults. The `status_bar` section of the same file holds the status bar's switches: whether the bar shows at all, and which of its indicators it shows, with a key it leaves out keeping the default, which is on.

Arrow keys recall history: ArrowUp brings back your previous prompts, ArrowDown walks forward and returns to your draft. When a prompt is queued (below), ArrowUp returns the queued prompt to the composer instead, so you can edit it.

Snippets are the chips under the composer: each inserts a prepared prompt, such as the default _Init AGENTS.md_ or _Work on issue_. You edit their labels and prompts under **Settings > Snippets**, and reset one or all of them to the defaults there.

**Voice input** is the mic button in the composer. It records your microphone, shows the words as it hears them, and puts the transcript in the composer when you stop. It is disabled while the agent is running, when the microphone permission is missing, or when the whisper binary or model is missing.

Queued prompts: while a turn runs, the send button has turned into **Stop** (Esc cancels the run), and a prompt you send is queued rather than delivered - it shows as a `Queued:` line above the composer with a **Discard queued prompt** button, and is sent when the running turn ends.

Two buttons hang off the transcript: **Resend prompt** under a sent prompt runs that prompt again and drops everything that came after it, which rewinds a conversation that went somewhere you did not want. **Reply to message** quotes the message above your prompt, so the agent knows what the remark is about, and **Clear reply** cancels the quote.

**Try in mock mode:** send `say hi`, press **Reply to message** on the answer, and send `make it shorter` - the agent sees the quoted message and shortens it.

## Many chats at once

Chats run in parallel: each one is its own session, and a session that is working does not block opening another chat or starting another session. Under **Settings > General**, Sessions limits how many run at once: **Max concurrent sessions** caps it, five by default. A send past the cap stays in the composer, with an error like `Max 5 concurrent sessions reached - stop one to start another`.

**Broadcast to projects** is the button next to **New project** in the sidebar. It turns the project list into checkboxes, and whatever you type is sent to every selected project, each in its own new session, all running at once - a fan-out for asking every project the same thing. The count of children is shown, and an approval card in one child can be answered from that same view.

A chat that hands work to subagents through the CLI's `Agent` tool gets a chat for each subagent. The sidebar lists them indented under the chat that spawned them, each with the label its orchestrator gave it, so each subagent's prompt and its own tool calls are one click away from its orchestrator. Hover a chat to see who started it. Deleting a chat deletes its subagents' chats with it. When the subagents work as a team, a teammate's message shows in the chat it reaches as a **Message from** card with the sender's name, and a message the lead could not deliver or the team's message limit shows as a red card.

**Try in mock mode:** send `create a file named s1.txt` in one chat and `create a file named s2.txt` in a second - both sessions run at the same time, each pausing at its own Write approval card.
