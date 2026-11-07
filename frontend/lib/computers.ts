import type { Conversation, MarkdownAgent } from "./tauri";
import { COMPUTER_USE_TOOLS, pendingInput, type ChatState, type TranscriptItem } from "./transcript";

export type ComputerStatus = "running" | "awaiting" | "idle";

/** This computer, as the workspace of a chat that runs here. */
export const THIS_COMPUTER = "";

/** What a tile shows of the chat's agent. A chat without one shows as a plain chat. */
export type ComputerOwner = Pick<MarkdownAgent, "name" | "emoji" | "avatar">;

const PLAIN_CHAT: ComputerOwner = { name: "chat", emoji: "", avatar: null };

/** One chat running on a workspace, as the Computers view shows it. */
export type ComputerTile = {
  sessionId: string;
  agent: ComputerOwner;
  workspace: string;
  status: ComputerStatus;
  lastAction: string | null;
};

/** Where a chat runs: the workspace it started on or was moved to, else its agent's, else this computer. */
export function chatWorkspace(
  sessionId: string,
  workspaceOf: Record<string, string>,
  agentOf: Record<string, string>,
  agents: MarkdownAgent[],
): string {
  if (sessionId in workspaceOf) return workspaceOf[sessionId];
  return agents.find((agent) => agent.name === agentOf[sessionId])?.workspace || THIS_COMPUTER;
}

/** The moves that take a chat to `target`: a chat on one workspace comes home before it goes to another. */
export function moveHops(current: string, target: string): string[] {
  if (current === target) return [];
  return current !== THIS_COMPUTER && target !== THIS_COMPUTER ? [THIS_COMPUTER, target] : [target];
}

/**
 * The sidebar's chats: the stored ones, led by those the local store cannot list -
 * a running chat not yet saved, and a chat whose transcript lives on a workspace.
 */
export function sidebarConversations(
  conversations: Conversation[],
  runningIds: Set<string>,
  workspaceOf: Record<string, string>,
  titles: Record<string, string>,
  transcripts: Record<string, ChatState>,
): Conversation[] {
  const stored = new Set(conversations.map((c) => c.id));
  const onWorkspace = Object.keys(workspaceOf).filter((id) => workspaceOf[id] !== THIS_COMPUTER);
  const unlisted = [...new Set([...runningIds, ...onWorkspace])].filter((id) => !stored.has(id));
  const firstPrompt = (id: string) => {
    const first = transcripts[id]?.items.find((item) => item.kind === "user");
    return first?.kind === "user" ? first.text : undefined;
  };
  return [...unlisted.map((id) => ({ id, title: firstPrompt(id) ?? titles[id] ?? "New chat" })), ...conversations];
}

function lastComputerAction(items: TranscriptItem[]): string | null {
  const call = [...items].reverse().find((item) => item.kind === "tool" && COMPUTER_USE_TOOLS.has(item.name));
  if (call?.kind !== "tool") return null;
  try {
    return String(JSON.parse(call.args).action ?? call.name);
  } catch {
    return call.name;
  }
}

function statusOf(running: boolean, items: TranscriptItem[]): ComputerStatus {
  if (!running) return "idle";
  return pendingInput(items) ? "awaiting" : "running";
}

/**
 * The open chats that ran on a workspace, running ones first. A chat whose agent
 * has a workspace gets its computer with its first message there, not before.
 */
export function computerTiles(
  workspaceOf: Record<string, string>,
  agentOf: Record<string, string>,
  agents: MarkdownAgent[],
  transcripts: Record<string, ChatState>,
  runningIds: Set<string>,
): ComputerTile[] {
  return Object.entries(transcripts)
    .flatMap(([sessionId, chat]) => {
      const workspace = workspaceOf[sessionId] ?? THIS_COMPUTER;
      if (workspace === THIS_COMPUTER) return [];
      return [
        {
          sessionId,
          agent: agents.find((agent) => agent.name === agentOf[sessionId]) ?? PLAIN_CHAT,
          workspace,
          status: statusOf(runningIds.has(sessionId), chat.items),
          lastAction: lastComputerAction(chat.items),
        },
      ];
    })
    .sort((a, b) => Number(a.status === "idle") - Number(b.status === "idle"));
}
