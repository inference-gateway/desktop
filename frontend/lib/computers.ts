import type { MarkdownAgent } from "./tauri";
import { COMPUTER_USE_TOOLS, pendingInput, type ChatState, type TranscriptItem } from "./transcript";

export type ComputerStatus = "running" | "awaiting" | "idle";

/** One chat running on its agent's own computer, as the Computers view shows it. */
export type ComputerTile = {
  sessionId: string;
  agent: MarkdownAgent;
  workspace: string;
  status: ComputerStatus;
  lastAction: string | null;
};

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

/** The chats of agents that have a workspace, running ones first. */
export function computerTiles(
  agentOf: Record<string, string>,
  agents: MarkdownAgent[],
  transcripts: Record<string, ChatState>,
  runningIds: Set<string>,
): ComputerTile[] {
  return Object.entries(agentOf)
    .flatMap(([sessionId, name]) => {
      const agent = agents.find((a) => a.name === name);
      const chat = transcripts[sessionId];
      if (!agent?.workspace || !chat) return [];
      return [
        {
          sessionId,
          agent,
          workspace: agent.workspace,
          status: statusOf(runningIds.has(sessionId), chat.items),
          lastAction: lastComputerAction(chat.items),
        },
      ];
    })
    .sort((a, b) => Number(a.status === "idle") - Number(b.status === "idle"));
}
