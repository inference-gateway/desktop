import { expect, test } from "bun:test";
import { computerTiles } from "./computers";
import type { MarkdownAgent } from "./tauri";
import { initialChatState, type ChatState, type TranscriptItem } from "./transcript";

const agent = (name: string, workspace: string): MarkdownAgent => ({
  name,
  description: "d",
  emoji: "",
  model: "",
  workspace,
  instructions: "i",
  avatar: null,
  path: "",
});

const chat = (items: TranscriptItem[]): ChatState => ({ ...initialChatState, items });

const click: TranscriptItem = {
  kind: "tool",
  id: "t1",
  callId: "c1",
  name: "Computer",
  args: '{"action":"click","x":1,"y":2}',
  output: null,
  state: "done",
  skeleton: false,
};

const approval: TranscriptItem = {
  kind: "approval",
  id: "a1",
  callId: "c2",
  toolName: "Computer",
  toolArgs: "{}",
  status: "pending",
};

test("only chats of agents with a workspace get a tile, running ones first", () => {
  const agents = [agent("pilot", "ubuntu-desktop"), agent("local", "")];
  const tiles = computerTiles(
    { idle: "pilot", busy: "pilot", here: "local", unopened: "pilot" },
    agents,
    { idle: chat([click]), busy: chat([click, approval]), here: chat([]) },
    new Set(["busy"]),
  );
  expect(tiles.map((t) => [t.sessionId, t.status, t.lastAction])).toEqual([
    ["busy", "awaiting", "click"],
    ["idle", "idle", "click"],
  ]);
  expect(tiles[0].workspace).toBe("ubuntu-desktop");
});

test("a chat that has not used its computer yet has no last action", () => {
  const tiles = computerTiles({ s1: "pilot" }, [agent("pilot", "k8s")], { s1: chat([]) }, new Set(["s1"]));
  expect(tiles).toEqual([
    { sessionId: "s1", agent: agent("pilot", "k8s"), workspace: "k8s", status: "running", lastAction: null },
  ]);
});
