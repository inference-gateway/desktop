import { useEffect, useState } from "react";
import { api, type ComputerResources } from "@/lib/tauri";

/**
 * What the computers of these sessions are and were given: each session's resources and the workspaces that
 * are sandboxes. Read again when the sessions change.
 */
export function useComputerFacts(sessionIds: string[]) {
  const [resources, setResources] = useState<Record<string, ComputerResources>>({});
  const [sandboxes, setSandboxes] = useState<Set<string>>(new Set());
  const sessions = sessionIds.join(" ");
  useEffect(() => {
    api
      .computerResources()
      .then(setResources)
      .catch(() => {});
    api
      .workspaceConfigs()
      .then((configs) => setSandboxes(new Set(configs.filter((c) => c.kind === "kubernetes").map((c) => c.name))))
      .catch(() => {});
  }, [sessions]);
  return { resources, sandboxes };
}
