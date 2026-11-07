import type { WorkspaceConfig } from "./tauri";

type Gateway = Pick<WorkspaceConfig, "name" | "gateway_env" | "env">;

/** What a chat's model becomes on a workspace: itself, the workspace's own model with a note, or a problem. */
export type WorkspaceModel = { model: string; note?: string } | { problem: string };

const providerOf = (model: string) => model.split("/")[0];

/** The key a workspace's gateway needs to serve `model`, such as OPENAI_API_KEY. */
export function providerKey(model: string): string {
  return `${providerOf(model).toUpperCase()}_API_KEY`;
}

/**
 * The model a chat runs on in a workspace. A workspace's gateway only serves the providers whose keys it gets,
 * so a model of another provider falls back to the workspace's own model - unless the model is `fixed` (an
 * agent's), which then is a problem. A workspace without a gateway of its own serves any model.
 */
export function workspaceModel(model: string, workspace: Gateway | undefined, fixed = false): WorkspaceModel {
  if (!workspace || workspace.gateway_env.length === 0) return { model };
  const served = (candidate: string) => workspace.gateway_env.includes(providerKey(candidate));
  if (served(model)) return { model };
  const own = workspace.env.INFER_AGENT_MODEL ?? "";
  if (!fixed && own.includes("/") && served(own)) {
    return { model: own, note: `${workspace.name} has no ${providerOf(model)} key - the chat runs on ${own} there` };
  }
  return {
    problem: `${workspace.name} has no key for ${providerOf(model)}. Add ${providerKey(model)} to its provider keys in Settings > Workspaces, or pick a model it serves (${workspace.gateway_env.join(", ")}).`,
  };
}
