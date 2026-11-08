import { expect, test } from "bun:test";
import { EMPTY_SERVER, missingSecrets, secretVariables, serverState } from "./mcp-form";
import type { McpStatus, Secret } from "./tauri";

const status = (over: Partial<McpStatus> = {}): McpStatus => ({
  enabled: true,
  total_servers: 1,
  connected_servers: 1,
  total_tools: 3,
  servers: [{ name: "agentmail", connected: true, tools: 3, error: "" }],
  ...over,
});

const secret = (over: Partial<Secret>): Secret => ({
  label: "reviewer-inbox",
  variable: "AGENTMAIL_API_KEY",
  computer: false,
  workspaces: [],
  mcp_servers: [],
  ...over,
});

const agentmail = {
  ...EMPTY_SERVER,
  name: "agentmail",
  url: "https://mcp.agentmail.to/mcp",
  headers: [{ name: "x-api-key", prefix: "", variable: "AGENTMAIL_API_KEY", literal: false }],
};

test("serverState reads the server's row of infer mcp status", () => {
  expect(serverState(agentmail, status())).toBe("Connected, 3 tools");
  expect(serverState({ ...agentmail, enabled: false }, status())).toBe("Off on this computer");
  expect(serverState(agentmail, null)).toBe("Checking...");
  expect(serverState(agentmail, status({ enabled: false }))).toBe("MCP is off");
  expect(serverState(agentmail, status({ servers: [] }))).toBe("Not checked");
  expect(
    serverState(agentmail, status({ servers: [{ name: "agentmail", connected: false, tools: 0, error: "401" }] })),
  ).toBe("Not connected: 401");
});

test("missingSecrets names each place without a secret for a header's variable", () => {
  const onPods = { ...agentmail, workspaces: ["reviewer", "writer"] };
  const secrets = [secret({ workspaces: ["reviewer"] }), secret({ label: "writer-inbox", workspaces: [] })];
  expect(missingSecrets(onPods, secrets)).toEqual([
    "this computer gets no AGENTMAIL_API_KEY",
    "workspace writer gets no AGENTMAIL_API_KEY",
  ]);
  expect(missingSecrets({ ...onPods, enabled: false, workspaces: ["reviewer"] }, secrets)).toEqual([]);
  const apiKey = {
    ...agentmail,
    headers: [{ name: "Authorization", prefix: "Bearer ", variable: "OPENAI_API_KEY", literal: false }],
  };
  expect(missingSecrets(apiKey, secrets)).toEqual([]);
});

test("secretVariables offers each variable once", () => {
  expect(
    secretVariables([
      secret({}),
      secret({ label: "writer-inbox" }),
      secret({ label: "github", variable: "GITHUB_TOKEN" }),
    ]),
  ).toEqual(["AGENTMAIL_API_KEY", "GITHUB_TOKEN"]);
});
