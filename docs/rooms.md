# Rooms

A room is a named group of agents in one project that you talk to like a Slack channel. Every member's posts show in one view, and you write to the room instead of to one agent at a time. The `infer` daemon hosts the rooms: it routes each post, runs the members it reaches, and logs their answers (see the CLI's [rooms guide](https://github.com/inference-gateway/cli/blob/main/docs/rooms.md)).

Every prompt on this page is answered by a scenario in `e2e/scenarios.yaml`, so you can try it without provider tokens in mock mode: `DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev`.

## Opening a room

**Filter rooms** in the sidebar lists the rooms of `~/.infer/rooms.yaml`. The default `public` room, with every agent in it, is there from the start. **Select room <name>** opens a room in the main pane, and each member's own chat, `room-<room>-<agent>`, nests under its room. **Refresh rooms** lists them again after you edit `rooms.yaml`.

The room's header names its members with their roles, `jeff (tester)`. Each post shows its author's face and name, when it was posted, and the members it reached. Opening a chat or starting a new one leaves the room.

## Posting

With a room open, the composer posts to it. Its menus come from the `room` context of `composer.yaml` (see [Chat](chat.md)), so `@` lists the room's members before the project's files.

- A post that mentions a member, `@reviewer check the release notes`, reaches that member, which answers in the room.
- A post that mentions nobody goes where the router sends it. The router model of `rooms.yaml`, the CLI's default unless you set another, picks the members it is for. If `rooms.yaml` empties `router.model`, only a mention routes, and the room says the post reached nobody.
- A notice for you, such as a mention of an agent outside the room, shows in the status bar.

The desktop starts its gateway with the Decisions API the router asks, unless `rooms.yaml` empties `router.model`.

**Try in mock mode:** with an agent named `reviewer`, open `public` and send `@reviewer check the release notes`. The post shows it went to the reviewer, and the reviewer answers `The release notes look good.` A post such as `is anyone around` goes to the member the router picks.

## A member's own chat

Every member's post has a button to its own chat in the room, where its work, its questions and its approvals live. A member waiting on an approval or a question shows in a bar above the posts, with the same way to its chat, so you answer it there.
