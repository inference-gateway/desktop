import { Avatar, Style } from "@dicebear/core";
import bottts from "@dicebear/styles/bottts.json";
import type { Conversation, MarkdownAgent } from "./tauri";

export type AgentFace =
  { kind: "picture"; src: string } | { kind: "emoji"; emoji: string } | { kind: "initial"; letter: string };

/** The CLI's default agent, which a chat without an agent of its own runs as. */
export const MAIN_AGENT = "main";

const BOTTTS = new Style(bottts);
const ROBOT_BACKGROUNDS = ["1e1b4b", "312e81", "0f172a", "134e4a"];
const generated = new Map<string, string>();

/** A robot drawn deterministically from the agent's name (DiceBear bottts,
 * generated locally), so every agent has a face before anyone picks one. */
export function generatedRobot(name: string): string {
  let src = generated.get(name);
  if (!src) {
    const svg = new Avatar(BOTTTS, { seed: name, backgroundColor: ROBOT_BACKGROUNDS }).toString();
    src = `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
    generated.set(name, src);
  }
  return src;
}

/** What to show for an agent: its picture, else its emoji, else a robot
 * generated from its name, else a question mark while the name is empty. */
export function agentFace(agent: Pick<MarkdownAgent, "name" | "emoji" | "avatar">): AgentFace {
  if (agent.avatar) return { kind: "picture", src: agent.avatar };
  const emoji = agent.emoji.trim();
  if (emoji) return { kind: "emoji", emoji };
  if (agent.name) return { kind: "picture", src: generatedRobot(agent.name) };
  return { kind: "initial", letter: "?" };
}

/** The agent each top-level chat runs as, as the CLI's listing records it. A
 * subagent's chat stays under its orchestrator, and a chat of the main agent
 * belongs to no agent. */
export function listedAgentOf(
  conversations: Pick<Conversation, "id" | "agent" | "parent_session_id">[],
): Record<string, string> {
  return Object.fromEntries(
    conversations.flatMap((c) => (c.agent && c.agent !== MAIN_AGENT && !c.parent_session_id ? [[c.id, c.agent]] : [])),
  );
}

/** The model an agent chat runs on, the rule the CLI applies to the agent
 * file: a provider-prefixed frontmatter model wins, anything else (empty,
 * `inherit`, a bare alias) inherits the selected one. */
export function agentThreadModel(agent: Pick<MarkdownAgent, "model"> | undefined, selected: string): string {
  const model = agent?.model.trim() ?? "";
  return model.includes("/") ? model : selected;
}

/** Whether the agent fixes its chats' model, so no picker may change it. */
export function agentPinsModel(agent: Pick<MarkdownAgent, "model"> | undefined): boolean {
  return agentThreadModel(agent, "") !== "";
}

/** Whether this computer's gateway serves the model the agent's file pins, so
 * a chat or subagent here can run it. True for an agent that pins none, and
 * while the served list has not loaded. */
export function agentModelServed(agent: Pick<MarkdownAgent, "model">, served: string[]): boolean {
  const pinned = agentThreadModel(agent, "");
  return !pinned || served.length === 0 || served.includes(pinned);
}

/** The project a new chat with the agent opens in: its frontmatter project
 * while that project still exists, else none. */
export function agentDefaultProject(
  agent: Pick<MarkdownAgent, "project"> | undefined,
  projectNames: string[],
): string | null {
  const project = agent?.project.trim() ?? "";
  return projectNames.includes(project) ? project : null;
}

/** Turns free text into a valid agent name: lowercase, `-` for spaces, only
 * letters, digits, `-` and `_`, at most 64 characters. */
export function normaliseAgentName(input: string): string {
  return input
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^a-z0-9_-]/g, "")
    .slice(0, 64);
}

/** The line an agent chat opens with: the agent it starts as. A later switch
 * is announced by the CLI, whose reminder supersedes this line. */
export function agentIdentity(agent: MarkdownAgent): string {
  return `You are running as the agent "${agent.name}": ${agent.description.trim()}`;
}

/** The inbox of every agent that has one and whom it may mail. Every chat
 * carries the whole list, so a chat that switches agents still knows which
 * inbox is its own. The CLI's Agent tool lists the agents themselves. */
export function agentInboxes(all: MarkdownAgent[]): string {
  const lines = all
    .filter((a) => a.email.trim())
    .map((a) => {
      const recipients = a.allowed_recipients.map((r) => r.trim()).filter(Boolean);
      const limit = recipients.length > 0 ? ` - may only send mail to ${recipients.join(", ")}` : "";
      return `- ${a.name}: ${a.email.trim()}${limit}`;
    });
  if (lines.length === 0) return "";
  return [
    "Agent email inboxes. The inbox of the agent you are running as is yours - people and other agents can write to you there:",
    ...lines,
  ].join("\n");
}
