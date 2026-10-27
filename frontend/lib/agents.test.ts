import { expect, test } from "bun:test";
import { agentFace, agentInstructionsPreamble, agentThreadModel, normaliseAgentName } from "./agents";
import type { MarkdownAgent } from "./tauri";

const agent = (over: Partial<MarkdownAgent>): MarkdownAgent => ({
  name: "writer",
  description: "Writes prose.",
  emoji: "",
  model: "",
  instructions: "Write well.",
  avatar: null,
  path: "/tmp/writer.md",
  ...over,
});

test("agentFace prefers the picture, then the emoji, then the initial", () => {
  expect(agentFace(agent({ avatar: "data:image/gif;base64,R0", emoji: "✍️" }))).toEqual({
    kind: "picture",
    src: "data:image/gif;base64,R0",
  });
  expect(agentFace(agent({ emoji: " ✍️ " }))).toEqual({ kind: "emoji", emoji: "✍️" });
  expect(agentFace(agent({}))).toEqual({ kind: "initial", letter: "W" });
});

test("agentThreadModel uses a provider-prefixed frontmatter model, else the selected one", () => {
  expect(agentThreadModel(agent({ model: "deepseek/deepseek-v4-pro" }), "openai/gpt-4o")).toBe(
    "deepseek/deepseek-v4-pro",
  );
  expect(agentThreadModel(agent({ model: "inherit" }), "openai/gpt-4o")).toBe("openai/gpt-4o");
  expect(agentThreadModel(agent({ model: "sonnet" }), "openai/gpt-4o")).toBe("openai/gpt-4o");
  expect(agentThreadModel(undefined, "openai/gpt-4o")).toBe("openai/gpt-4o");
});

test("normaliseAgentName produces a CLI-valid name", () => {
  expect(normaliseAgentName("Code Reviewer")).toBe("code-reviewer");
  expect(normaliseAgentName("  Résumé  Bot! ")).toBe("-rsum-bot-");
  expect(normaliseAgentName("a".repeat(70))).toHaveLength(64);
});

test("agentInstructionsPreamble names the agent and lists the others", () => {
  const me = agent({ name: "writer" });
  const reviewer = agent({ name: "reviewer", description: "Reviews drafts." });
  expect(agentInstructionsPreamble(me, [me, reviewer])).toBe(
    [
      'You are running as the agent "writer": Writes prose.',
      "Other agents you can delegate to by passing their name as the Agent tool's `agent` argument:",
      "- reviewer: Reviews drafts.",
    ].join("\n"),
  );
  expect(agentInstructionsPreamble(me, [me])).toBe('You are running as the agent "writer": Writes prose.');
});
