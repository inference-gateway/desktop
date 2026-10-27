import { expect, test } from "bun:test";
import { agentFace, agentInstructionsPreamble, agentThreadModel, normaliseAgentName } from "./agents";
import type { MarkdownAgent } from "./tauri";
import { FLUENT_EMOJIS, fluentAnimatedUrl, fluentStaticUrl } from "./fluent-emoji";

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
  const robot = agentFace(agent({}));
  expect(robot.kind).toBe("picture");
  expect(robot).toEqual(agentFace(agent({})));
  expect(robot).not.toEqual(agentFace(agent({ name: "reviewer" })));
  expect(agentFace(agent({ name: "" }))).toEqual({ kind: "initial", letter: "?" });
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

test("fluent emoji entries are unique and resolve to the static and animated asset URLs", () => {
  expect(new Set(FLUENT_EMOJIS.map((e) => e.folder)).size).toBe(FLUENT_EMOJIS.length);
  const nerd = FLUENT_EMOJIS.find((e) => e.folder === "Nerd face")!;
  expect(fluentStaticUrl(nerd)).toBe(
    "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/assets/Nerd%20face/3D/nerd_face_3d.png",
  );
  expect(fluentAnimatedUrl(nerd)).toBe(
    "https://media.githubusercontent.com/media/microsoft/fluentui-emoji-animated/main/assets/Nerd%20face/animated/nerd_face_animated.png",
  );
  const ninja = FLUENT_EMOJIS.find((e) => e.folder === "Ninja")!;
  expect(fluentAnimatedUrl(ninja)).toBe(
    "https://media.githubusercontent.com/media/microsoft/fluentui-emoji-animated/main/assets/Ninja/Default/animated/ninja_animated_default.png",
  );
});
