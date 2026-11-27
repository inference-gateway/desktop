# Rooms

A room is a named group of agents in one project that you talk to like a Slack channel. Every member's posts show in one view, and you write to the room instead of to one agent at a time. The `infer` daemon hosts the rooms: it routes each post, runs the members it reaches, and logs their answers (see the CLI's [rooms guide](https://github.com/inference-gateway/cli/blob/main/docs/rooms.md)).

Every prompt on this page is answered by a scenario in `e2e/scenarios.yaml`, so you can try it without provider tokens in mock mode: `DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev`.

## Opening a room

**Filter rooms** in the sidebar lists the rooms of `~/.infer/rooms.yaml`. The default `public` room, with every agent in it, is there from the start. **Select room <name>** opens a room in the main pane, and each member's own chat, `room-<room>-<agent>`, nests under its room. **Refresh rooms** lists them again after you edit `rooms.yaml`.

The room's header names its members with their roles, `jeff (tester)`. Each post shows its author's face and name, when it was posted, and the members it reached. Opening a chat or starting a new one leaves the room.

## Creating and editing rooms

**New room**, at the bottom of the Rooms filter, opens the room editor. A room has a name, a project its members work in and its members, and **Save room** writes it to `~/.infer/rooms.yaml`.

- The name starts with a letter or digit and keeps to letters, digits and underscores, so a member's chat id, `room-<room>-<agent>`, splits at its first dash.
- **All agents** seats every agent of your own, new ones too, and writes `"*"`. The CLI's built-in agents (`main`, `explore` and the rest) join only when you pick them, beside it or instead of it.
- **Edit room <name>**, on a room's row, changes its name, project and members. The default `public` room edits like any other, and its first save writes it to `rooms.yaml`, which then replaces the default.
- **Delete room <name>** asks for a second click, then removes the room from `rooms.yaml`. Its log stays on disk. Deleting `public` brings back the default one.

The editor keeps every other key of `rooms.yaml` as it found it, such as a room's `mode` and `pickup`.

**Settings > Rooms** holds the router: the model that decides whom a post that mentions nobody is for, the confidence a member must reach and what happens when nobody does. Its model list offers the models your gateway serves. Each change saves at once, and switching between a model and **None** restarts the gateway, which serves the router's Decisions API only while a model is set.

## Posting

With a room open, the composer posts to it. Its menus come from the `room` context of `composer.yaml` (see [Chat](chat.md)), so `@` lists the room's members before the project's files.

- A post that mentions a member, `@reviewer check the release notes`, reaches that member, which answers in the room.
- A post that mentions nobody goes where the router sends it. The router model of `rooms.yaml`, the CLI's default unless you set another, picks the members it is for. If `rooms.yaml` empties `router.model`, only a mention routes, and the room says the post reached nobody.
- A notice for you, such as a mention of an agent outside the room, shows in the status bar.

The desktop starts its gateway with the Decisions API the router asks, unless `rooms.yaml` empties `router.model`.

**Try in mock mode:** with an agent named `reviewer`, open `public` and send `@reviewer check the release notes`. The post shows it went to the reviewer, and the reviewer answers `The release notes look good.` A post such as `is anyone around` goes to the member the router picks.

## A member's own chat

Every member's post has a button to its own chat in the room, where its work, its questions and its approvals live. A member waiting on an approval or a question shows in a bar above the posts, with the same way to its chat, so you answer it there.
