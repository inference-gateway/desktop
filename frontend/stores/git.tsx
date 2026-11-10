import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "@/lib/tauri";
import { useActivity } from "@/stores/activity";
import { BACKEND_EVENTS } from "@/lib/events";

function useGitStore() {
  const { logActivity } = useActivity();
  const [gitProjects, setGitProjects] = useState<Set<string>>(() => new Set());
  const [dirtyProjects, setDirtyProjects] = useState<Set<string>>(() => new Set());
  const [projectBranches, setProjectBranches] = useState<Record<string, string>>(() => ({}));
  const [projectDefaultBranches, setProjectDefaultBranches] = useState<Record<string, string>>(() => ({}));
  const [projectRemotes, setProjectRemotes] = useState<Record<string, string>>(() => ({}));

  const fetchGitProjects = useCallback(async () => {
    const status = await api.gitProjectStatus();
    setGitProjects(new Set(status.git));
    setDirtyProjects(new Set(status.dirty));
    setProjectBranches(status.branches);
    setProjectDefaultBranches(status.default_branches);
    setProjectRemotes(status.remotes);
  }, []);

  const refreshGitProjects = useCallback(() => fetchGitProjects().catch(() => {}), [fetchGitProjects]);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unlisten = listen(BACKEND_EVENTS.gitChanged, () => {
      clearTimeout(timer);
      timer = setTimeout(refreshGitProjects, 300);
    });
    return () => {
      clearTimeout(timer);
      unlisten.then((f) => f());
    };
  }, [refreshGitProjects]);

  const refreshProjects = useCallback(() => {
    logActivity(null, "Refresh projects", "running");
    return fetchGitProjects().then(
      () => logActivity(null, "Refresh projects", "done", "git status updated"),
      (e) => logActivity(null, "Refresh projects", "failed", String(e)),
    );
  }, [fetchGitProjects, logActivity]);

  const runGitAction = useCallback(
    (name: string, action: string, run: () => Promise<string>) => {
      logActivity(name, action, "running");
      return run().then(
        (message) => logActivity(name, action, "done", message),
        (e) => logActivity(name, action, "failed", String(e)),
      );
    },
    [logActivity],
  );
  const cleanupProject = useCallback(
    (name: string) => runGitAction(name, "Clean up branches", () => api.cleanupProject(name)),
    [runGitAction],
  );
  const syncDefaultBranch = useCallback(
    (name: string) => runGitAction(name, "Checkout default branch + pull", () => api.syncDefaultBranch(name)),
    [runGitAction],
  );

  return useMemo(
    () => ({
      gitProjects,
      dirtyProjects,
      projectBranches,
      projectDefaultBranches,
      projectRemotes,
      fetchGitProjects,
      refreshGitProjects,
      refreshProjects,
      cleanupProject,
      syncDefaultBranch,
    }),
    [
      gitProjects,
      dirtyProjects,
      projectBranches,
      projectDefaultBranches,
      projectRemotes,
      fetchGitProjects,
      refreshGitProjects,
      refreshProjects,
      cleanupProject,
      syncDefaultBranch,
    ],
  );
}

type GitStore = ReturnType<typeof useGitStore>;

const GitContext = createContext<GitStore | null>(null);

export function GitProvider({ children }: { children: ReactNode }) {
  return <GitContext.Provider value={useGitStore()}>{children}</GitContext.Provider>;
}

/** Git status per project (which are repos, dirty, their branches and remotes)
 * and the git actions that run on one project. */
export function useGit(): GitStore {
  const ctx = useContext(GitContext);
  if (!ctx) throw new Error("useGit must be used within GitProvider");
  return ctx;
}
