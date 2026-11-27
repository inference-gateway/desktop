import { expect, test } from "bun:test";
import {
  agentDefaultProject,
  agentFace,
  agentIdentity,
  agentInboxes,
  agentModelServed,
  agentPinsModel,
  agentThreadModel,
  listedAgentOf,
  normaliseAgentName,
} from "./agents";
import type { MarkdownAgent } from "./tauri";
import { FLUENT_EMOJIS, fluentAnimatedUrl, fluentStaticUrl } from "./fluent-emoji";

const agent = (over: Partial<MarkdownAgent>): MarkdownAgent => ({
  name: "writer",
  description: "Writes prose.",
  role: "",
  emoji: "",
  model: "",
  workspace: "",
  project: "",
  email: "",
  allowed_recipients: [],
  mcp_servers: [],
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

test("agentDefaultProject opens the agent's project only while it still exists", () => {
  expect(agentDefaultProject(agent({ project: "desktop" }), ["cli", "desktop"])).toBe("desktop");
  expect(agentDefaultProject(agent({ project: "renamed" }), ["cli", "desktop"])).toBeNull();
  expect(agentDefaultProject(agent({}), ["cli", "desktop"])).toBeNull();
  expect(agentDefaultProject(undefined, ["cli", "desktop"])).toBeNull();
});

test("only a provider-prefixed frontmatter model pins an agent's model", () => {
  expect(agentPinsModel(agent({ model: "deepseek/deepseek-v4-pro" }))).toBe(true);
  expect(agentPinsModel(agent({ model: "inherit" }))).toBe(false);
  expect(agentPinsModel(agent({ model: "sonnet" }))).toBe(false);
  expect(agentPinsModel(undefined)).toBe(false);
});

test("normaliseAgentName produces a CLI-valid name", () => {
  expect(normaliseAgentName("Code Reviewer")).toBe("code-reviewer");
  expect(normaliseAgentName("  Résumé  Bot! ")).toBe("-rsum-bot-");
  expect(normaliseAgentName("a".repeat(70))).toHaveLength(64);
});

test("agentIdentity names the agent a chat starts as", () => {
  expect(agentIdentity(agent({ name: "writer" }))).toBe('You are running as the agent "writer": Writes prose.');
});

test("agentInboxes lists every agent's inbox and whom it may mail", () => {
  const writer = agent({
    name: "writer",
    email: "writer@agentmail.to",
    allowed_recipients: [" human@example.com ", "", "reviewer@agentmail.to"],
  });
  const reviewer = agent({ name: "reviewer", email: "reviewer@agentmail.to" });
  const addressless = agent({ name: "drafter", allowed_recipients: ["nobody@example.com"] });

  expect(agentInboxes([writer, reviewer, addressless])).toBe(
    [
      "Agent email inboxes. The inbox of the agent you are running as is yours - people and other agents can write to you there:",
      "- writer: writer@agentmail.to - may only send mail to human@example.com, reviewer@agentmail.to",
      "- reviewer: reviewer@agentmail.to",
    ].join("\n"),
  );
  expect(agentInboxes([addressless]), "agents without an address add nothing").toBe("");
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

test("agentModelServed flags only a pinned model the gateway does not serve", () => {
  const served = ["openai/gpt-4o", "anthropic/claude-sonnet-5-5"];
  expect(agentModelServed(agent({ model: "openai/gpt-4o" }), served)).toBe(true);
  expect(agentModelServed(agent({ model: "mock/openai/gpt-4o" }), served)).toBe(false);
  expect(agentModelServed(agent({ model: "" }), served)).toBe(true);
  expect(agentModelServed(agent({ model: "inherit" }), served)).toBe(true);
  expect(agentModelServed(agent({ model: "sonnet" }), served)).toBe(true);
  expect(agentModelServed(agent({ model: "mock/openai/gpt-4o" }), [])).toBe(true);
});

test("listedAgentOf files a top-level chat under the agent the CLI lists it as", () => {
  expect(
    listedAgentOf([
      { id: "peer", agent: "tester", parent_session_id: "" },
      { id: "subagent", agent: "tester", parent_session_id: "peer" },
      { id: "main", agent: "main" },
      { id: "older", agent: null },
      { id: "plain" },
    ]),
  ).toEqual({ peer: "tester" });
});
