import type { Capability, WorkspaceConfig } from "./tauri";

/** A workspace as the Settings form holds it: lists and the environment as the text the person types. */
export type WorkspaceDraft = Omit<WorkspaceConfig, "gateway_env" | "forward_env" | "env"> & {
  gateway_env: string;
  forward_env: string;
  env: string;
};

export const WORKSPACE_KINDS = [{ id: "kubernetes", label: "Kubernetes" }] as const;

export const EMPTY_WORKSPACE: WorkspaceConfig = {
  name: "",
  compute: "",
  kind: "kubernetes",
  context: "",
  namespace: "",
  image: "",
  storage: "",
  idle_ttl: "",
  retention: "",
  gateway_env: [],
  forward_env: [],
  env: {},
  user_data: "",
};

/** How a cloud-init user-data script starts - the one user-data format a workspace runs. */
const USER_DATA_SCRIPT_PREFIX = "#!";

export function splitNames(text: string): string[] {
  return text.split(/[\s,]+/).filter(Boolean);
}

/** `NAME=value` lines as a map. A line without a name or an equals sign is dropped. */
export function parseEnv(text: string): Record<string, string> {
  const entries = text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.indexOf("=") > 0)
    .map((line) => [line.slice(0, line.indexOf("=")).trim(), line.slice(line.indexOf("=") + 1).trim()]);
  return Object.fromEntries(entries);
}

export function formatEnv(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([name, value]) => `${name}=${value}`)
    .join("\n");
}

export function draftOf(config: WorkspaceConfig): WorkspaceDraft {
  return {
    ...config,
    gateway_env: config.gateway_env.join(", "),
    forward_env: config.forward_env.join(", "),
    env: formatEnv(config.env),
  };
}

export function configOf(draft: WorkspaceDraft): WorkspaceConfig {
  return {
    ...draft,
    name: draft.name.trim(),
    gateway_env: splitNames(draft.gateway_env),
    forward_env: splitNames(draft.forward_env),
    env: parseEnv(draft.env),
  };
}

/** Whether the draft's environment and user data give its computers the capability. */
export function hasCapability(draft: WorkspaceDraft, capability: Capability): boolean {
  const env = parseEnv(draft.env);
  return capability.env.every(([name, value]) => env[name] === value) && draft.user_data.includes(capability.user_data);
}

/** The draft with the capability's variables and boot script added, or taken out. The rest stays as typed. */
export function withCapability(draft: WorkspaceDraft, capability: Capability, on: boolean): WorkspaceDraft {
  const names = new Set(capability.env.map(([name]) => name));
  const kept = Object.entries(parseEnv(draft.env)).filter(([name]) => !names.has(name));
  const script = capability.user_data;
  const rest = script ? draft.user_data.split(script).join("") : draft.user_data;
  return {
    ...draft,
    env: formatEnv(Object.fromEntries(on ? [...kept, ...capability.env] : kept)),
    user_data: on && script ? joinScripts(rest, script) : rest,
  };
}

function joinScripts(first: string, second: string): string {
  return first.trim() ? `${first.trimEnd()}\n${second}` : second;
}

/** Why a draft cannot be saved yet, or null when it can. */
export function draftProblem(draft: WorkspaceDraft): string | null {
  if (!draft.name.trim()) return "Give the workspace a name.";
  if (draft.user_data.trim() && !draft.user_data.trimStart().startsWith(USER_DATA_SCRIPT_PREFIX))
    return "Start the user data with #!, like #!/bin/sh - it runs as a script.";
  if (draft.compute) return null;
  if (draft.kind === "kubernetes" && !draft.context.trim()) return "Pick the Kubernetes context of the cluster.";
  if (draft.kind === "kubernetes" && !draft.namespace.trim()) return "Name the namespace the sessions run in.";
  return null;
}
