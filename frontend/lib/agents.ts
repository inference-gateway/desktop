import type { MarkdownAgent } from "./tauri";

export type AgentFace =
  { kind: "picture"; src: string } | { kind: "emoji"; emoji: string } | { kind: "initial"; letter: string };

/** What to show for an agent: its picture, else its emoji, else its first letter. */
export function agentFace(agent: Pick<MarkdownAgent, "name" | "emoji" | "avatar">): AgentFace {
  if (agent.avatar) return { kind: "picture", src: agent.avatar };
  const emoji = agent.emoji.trim();
  if (emoji) return { kind: "emoji", emoji };
  return { kind: "initial", letter: (agent.name[0] ?? "?").toUpperCase() };
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
