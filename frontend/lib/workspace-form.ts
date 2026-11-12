import { onFolderClaim } from "./cluster-folders";
import type { Capability, WorkspaceConfig, WorkspaceResources, WorkspaceVolume } from "./tauri";

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
  forward_voice_samples: false,
  env: {},
  user_data: "",
  volumes: [],
  resources: { cpu_request: "", cpu_limit: "", memory_request: "", memory_limit: "" },
  no_gpu: false,
};

const QUANTITY = /^(\d+\.?\d*|\.\d+)(n|u|m|k|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$/;
const QUANTITY_SCALE: Record<string, number> = {
  "": 1,
  n: 1e-9,
  u: 1e-6,
  m: 1e-3,
  k: 1e3,
  M: 1e6,
  G: 1e9,
  T: 1e12,
  P: 1e15,
  E: 1e18,
  Ki: 2 ** 10,
  Mi: 2 ** 20,
  Gi: 2 ** 30,
  Ti: 2 ** 40,
  Pi: 2 ** 50,
  Ei: 2 ** 60,
};

/** The amount a Kubernetes quantity such as `500m`, `2` or `4Gi` stands for, or null when the text is not one. */
export function quantityOf(text: string): number | null {
  const match = QUANTITY.exec(text.trim());
  return match ? Number(match[1]) * QUANTITY_SCALE[match[2] ?? ""] : null;
}

const NOT_A_QUANTITY = "Not a Kubernetes quantity - write it like 500m, 2 or 4Gi.";

/** What is wrong with each amount, by field: one that is not a quantity, and a request above its limit. A field with
 * nothing wrong is left out. */
export function resourceProblems(resources: WorkspaceResources): Partial<Record<keyof WorkspaceResources, string>> {
  const problems: Partial<Record<keyof WorkspaceResources, string>> = {};
  for (const key of Object.keys(resources) as (keyof WorkspaceResources)[]) {
    if (resources[key].trim() && quantityOf(resources[key]) === null) problems[key] = NOT_A_QUANTITY;
  }
  for (const resource of ["cpu", "memory"] as const) {
    const request = quantityOf(resources[`${resource}_request`]);
    const limit = quantityOf(resources[`${resource}_limit`]);
    if (request !== null && limit !== null && request > limit)
      problems[`${resource}_request`] = "The request is above the limit.";
  }
  return problems;
}

/** Whether the draft's computers are pods, the only ones whose CPU and memory a workspace sets. */
export function runsInCluster(draft: Pick<WorkspaceConfig, "compute" | "kind">): boolean {
  return Boolean(draft.compute) || draft.kind === "kubernetes";
}

/** The cluster the app provisions in a container, the only one that can share this computer's folders. */
export const LOCAL_CLUSTER = "infer-k3s";

export const EMPTY_VOLUME: WorkspaceVolume = { name: "", claim: "", mount_path: "", read_only: false };

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
    volumes: draft.volumes.map(trimmedVolume),
  };
}

function trimmedVolume(volume: WorkspaceVolume): WorkspaceVolume {
  return onFolderClaim({
    ...volume,
    name: volume.name.trim(),
    claim: volume.claim.trim(),
    mount_path: volume.mount_path.trim(),
  });
}

/** Why the computers could not mount the volumes as listed, or null when they could. */
export function volumesProblem(volumes: WorkspaceVolume[]): string | null {
  const names = volumes.map((volume) => volume.name.trim());
  for (const [index, volume] of volumes.entries()) {
    const name = names[index];
    if (!name) return `Give volume ${index + 1} a name.`;
    if (names.indexOf(name) !== index) return `Two volumes are named ${name} - give each its own name.`;
    if (!volume.claim.trim()) return `Name the claim volume ${name} mounts.`;
    if (!volume.mount_path.trim().startsWith("/")) return `Give volume ${name} a mount path that starts with /.`;
  }
  return null;
}

/** Whether the draft's environment, user data and voice samples give its computers the capability. */
export function hasCapability(draft: WorkspaceDraft, capability: Capability): boolean {
  const env = parseEnv(draft.env);
  return (
    capability.env.every(([name, value]) => env[name] === value) &&
    draft.user_data.includes(capability.user_data) &&
    (!capability.forward_voice_samples || draft.forward_voice_samples)
  );
}

/** The draft with the capability's variables, boot script and voice samples added, or taken out. The rest stays as typed. */
export function withCapability(draft: WorkspaceDraft, capability: Capability, on: boolean): WorkspaceDraft {
  const names = new Set(capability.env.map(([name]) => name));
  const kept = Object.entries(parseEnv(draft.env)).filter(([name]) => !names.has(name));
  const script = capability.user_data;
  const rest = script ? draft.user_data.split(script).join("") : draft.user_data;
  return {
    ...draft,
    env: formatEnv(Object.fromEntries(on ? [...kept, ...capability.env] : kept)),
    user_data: on && script ? joinScripts(rest, script) : rest,
    forward_voice_samples: capability.forward_voice_samples ? on : draft.forward_voice_samples,
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
  const volumes = volumesProblem(draft.volumes);
  if (volumes) return volumes;
  if (runsInCluster(draft) && Object.keys(resourceProblems(draft.resources)).length > 0)
    return "Fix the CPU and memory fields.";
  if (draft.compute) return null;
  if (draft.kind === "kubernetes" && !draft.context.trim()) return "Pick the Kubernetes context of the cluster.";
  if (draft.kind === "kubernetes" && !draft.namespace.trim()) return "Name the namespace the sessions run in.";
  return null;
}
