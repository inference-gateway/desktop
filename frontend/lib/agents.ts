import { Avatar, Style } from "@dicebear/core";
import bottts from "@dicebear/styles/bottts.json";
import type { MarkdownAgent } from "./tauri";

export type AgentFace =
  { kind: "picture"; src: string } | { kind: "emoji"; emoji: string } | { kind: "initial"; letter: string };

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

/** The model an agent chat launches with: a provider-prefixed frontmatter
 * model wins, anything else (empty, `inherit`, a bare alias) inherits. */
export function agentThreadModel(agent: Pick<MarkdownAgent, "model"> | undefined, selected: string): string {
  const model = agent?.model.trim() ?? "";
  return model.includes("/") ? model : selected;
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

/** The extra instruction an agent chat carries: who it is, and which sibling
 * agents it can delegate to through the Agent tool's `agent` argument. */
export function agentInstructionsPreamble(agent: MarkdownAgent, all: MarkdownAgent[]): string {
  const others = all.filter((a) => a.name !== agent.name);
  const lines = [`You are running as the agent "${agent.name}": ${agent.description.trim()}`];
  if (others.length > 0) {
    lines.push(
      "Other agents you can delegate to by passing their name as the Agent tool's `agent` argument:",
      ...others.map((a) => `- ${a.name}: ${a.description.trim()}`),
    );
  }
  return lines.join("\n");
}
