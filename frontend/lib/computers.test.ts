import { expect, test } from "bun:test";
import { chatWorkspace, computerSummary, computerTiles, moveHops, sidebarConversations } from "./computers";
import type { MarkdownAgent } from "./tauri";
import { initialChatState, type ChatState, type TranscriptItem } from "./transcript";

const agent = (name: string, workspace: string): MarkdownAgent => ({
  name,
  description: "d",
  emoji: "",
  model: "",
  workspace,
  project: "",
  email: "",
  allowed_recipients: [],
  mcp_servers: [],
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

test("only chats that ran on a workspace get a tile, running ones first", () => {
  const agents = [agent("pilot", "ubuntu-desktop"), agent("local", "")];
  const tiles = computerTiles(
    { idle: "ubuntu-desktop", busy: "ubuntu-desktop", unopened: "ubuntu-desktop" },
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
  const tiles = computerTiles(
    { s1: "k8s" },
    { s1: "pilot" },
    [agent("pilot", "k8s")],
    { s1: chat([]) },
    new Set(["s1"]),
  );
  expect(tiles).toEqual([
    { sessionId: "s1", agent: agent("pilot", "k8s"), workspace: "k8s", status: "running", lastAction: null },
  ]);
});

test("a chat whose agent has a workspace has no computer before it ran there", () => {
  const tiles = computerTiles({}, { s1: "pilot" }, [agent("pilot", "k8s")], { s1: chat([]) }, new Set());
  expect(tiles).toEqual([]);
});

test("a moved chat runs where it was moved, whatever its agent says", () => {
  const agents = [agent("pilot", "k8s")];
  expect(chatWorkspace("s1", {}, { s1: "pilot" }, agents)).toBe("k8s");
  expect(chatWorkspace("s1", { s1: "" }, { s1: "pilot" }, agents)).toBe("");
  expect(chatWorkspace("s2", { s2: "build" }, {}, agents)).toBe("build");
  expect(chatWorkspace("s3", {}, {}, agents)).toBe("");
});

test("a chat without an agent gets a tile once it is on a workspace, and loses it at home", () => {
  const agents = [agent("pilot", "k8s")];
  const chats = { plain: chat([]), home: chat([]) };
  const tiles = computerTiles({ plain: "build", home: "" }, { home: "pilot" }, agents, chats, new Set());
  expect(tiles.map((t) => [t.sessionId, t.agent.name, t.workspace])).toEqual([["plain", "chat", "build"]]);
});

test("a chat on one workspace comes home before it moves to another", () => {
  expect(moveHops("", "k8s")).toEqual(["k8s"]);
  expect(moveHops("k8s", "")).toEqual([""]);
  expect(moveHops("k8s", "build")).toEqual(["", "build"]);
  expect(moveHops("k8s", "k8s")).toEqual([]);
});

test("a chat on a workspace stays in the sidebar once its run ends, a chat at home is listed once", () => {
  const stored = [{ id: "local", title: "Local" }];
  const listed = sidebarConversations(
    stored,
    new Set(["local", "fresh"]),
    { remote: "k8s", restarted: "k8s", home: "", local: "k8s" },
    { remote: "stale title", restarted: "Hi" },
    { remote: chat([{ kind: "user", id: "u1", text: "open the docs" } as TranscriptItem]) },
  );
  expect(listed).toEqual([
    { id: "fresh", title: "New chat" },
    { id: "remote", title: "open the docs" },
    { id: "restarted", title: "Hi" },
    { id: "local", title: "Local" },
  ]);
});

test("a computer says whether it is a sandbox and what it was given", () => {
  expect(computerSummary(false)).toBe("Real system");
  expect(computerSummary(true)).toBe("Sandbox");
  expect(computerSummary(true, { cpu: "8", memory: "", gpus: 0 })).toBe("Sandbox - CPU 8 - Memory default - GPU none");
  expect(computerSummary(true, { cpu: "", memory: "12Gi", gpus: 1 })).toBe(
    "Sandbox - CPU default - Memory 12Gi - GPU 1",
  );
});
