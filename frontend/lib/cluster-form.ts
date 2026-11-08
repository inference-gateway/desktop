import type { ClusterConfig } from "./tauri";
import { splitNames } from "./workspace-form";

/** A cluster as the Compute tab's form holds it: its provider keys as the text the person types. */
export type ClusterDraft = Omit<ClusterConfig, "gateway_env"> & { gateway_env: string };

export const EMPTY_CLUSTER: ClusterConfig = { name: "", context: "", namespace: "infer-workspaces", gateway_env: [] };

/** The longest a cluster name may be, since it labels the Gateway and the sessions of its workspaces. */
const LABEL_VALUE_MAX = 63;
const LABEL_VALUE = /^[A-Za-z0-9]([-A-Za-z0-9_.]*[A-Za-z0-9])?$/;

export function clusterDraftOf(config: ClusterConfig): ClusterDraft {
  return { ...config, gateway_env: config.gateway_env.join(", ") };
}

export function clusterConfigOf(draft: ClusterDraft): ClusterConfig {
  return {
    ...draft,
    name: draft.name.trim(),
    namespace: draft.namespace.trim(),
    gateway_env: splitNames(draft.gateway_env),
  };
}

/** Why a cluster draft cannot be saved yet, or null when it can. */
export function clusterProblem(draft: ClusterDraft): string | null {
  const name = draft.name.trim();
  if (!name) return "Give the cluster a name.";
  if (name.length > LABEL_VALUE_MAX || !LABEL_VALUE.test(name)) {
    return "A cluster name is letters, digits, '-', '_' or '.', starting and ending with a letter or digit.";
  }
  if (!draft.namespace.trim()) return "Name the namespace the cluster's sessions run in.";
  return null;
}
