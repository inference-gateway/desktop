import { expect, test } from "bun:test";
import { providerKey, workspaceModel } from "./workspace-model";

const modelEnv = (model: string): Record<string, string> => (model ? { INFER_AGENT_MODEL: model } : {});

const workspace = (gateway_env: string[], model = "") => ({
  name: "ubuntu-desktop",
  gateway_env,
  env: modelEnv(model),
});

test("a provider key is named after the model's provider", () => {
  expect(providerKey("ollama_cloud/glm-5.3-flash")).toBe("OLLAMA_CLOUD_API_KEY");
  expect(providerKey("anthropic/claude-haiku-5-5")).toBe("ANTHROPIC_API_KEY");
});

test("a model the workspace's gateway serves stays", () => {
  const served = workspaceModel("anthropic/claude-haiku-5-5", workspace(["ANTHROPIC_API_KEY"]));
  expect(served).toEqual({ model: "anthropic/claude-haiku-5-5" });
});

test("a chat on this computer or on a workspace without its own gateway keeps its model", () => {
  expect(workspaceModel("anthropic/claude-haiku-5-5", undefined)).toEqual({ model: "anthropic/claude-haiku-5-5" });
  expect(workspaceModel("mock/openai/gpt-4o", workspace([]))).toEqual({ model: "mock/openai/gpt-4o" });
});

test("a model of a provider the gateway lacks falls back to the workspace's own model, with a note", () => {
  const moved = workspaceModel(
    "anthropic/claude-haiku-5-5",
    workspace(["OLLAMA_CLOUD_API_KEY"], "ollama_cloud/glm-5.3-flash"),
  );
  expect(moved).toMatchObject({ model: "ollama_cloud/glm-5.3-flash" });
  expect("note" in moved && moved.note).toContain("no anthropic key");
});

test("a fixed model never falls back to the workspace's own model", () => {
  const fixed = workspaceModel(
    "anthropic/claude-haiku-5-5",
    workspace(["OLLAMA_CLOUD_API_KEY"], "ollama_cloud/glm-5.3-flash"),
    true,
  );
  expect("problem" in fixed && fixed.problem).toContain("ANTHROPIC_API_KEY");
});

test("without a model the gateway serves, the problem names the key to add", () => {
  const stuck = workspaceModel("anthropic/claude-haiku-5-5", workspace(["OLLAMA_CLOUD_API_KEY"]));
  expect("problem" in stuck && stuck.problem).toContain("ANTHROPIC_API_KEY");
  const unserved = workspaceModel("anthropic/claude-haiku-5-5", workspace(["OLLAMA_CLOUD_API_KEY"], "openai/gpt-4o"));
  expect("problem" in unserved).toBe(true);
});
