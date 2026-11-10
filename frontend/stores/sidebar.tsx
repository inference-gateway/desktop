import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "@/lib/tauri";
import { parseSidebarDocument, serializeSidebarDocument, type ProjectType } from "@/lib/sidebar-document";
import { useGit } from "@/stores/git";
import { useStatusActions } from "@/stores/status";

function useSidebarStore() {
  const { setError } = useStatusActions();
  const { fetchGitProjects } = useGit();
  const [projects, setProjects] = useState<Record<string, string>>(() => ({}));
  const [projectNames, setProjectNames] = useState<string[]>(() => []);
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => new Set());
  const [activeProject, setActiveProject] = useState<string | null>(null);
  const [projectContexts, setProjectContexts] = useState<Record<string, string>>(() => ({}));
  const [projectPaths, setProjectPaths] = useState<Record<string, string>>(() => ({}));
  const [projectGroups, setProjectGroups] = useState<Record<string, string>>(() => ({}));
  const [projectTypes, setProjectTypes] = useState<Record<string, ProjectType>>(() => ({}));
  const [agentOf, setAgentOf] = useState<Record<string, string>>(() => ({}));
  const [workspaceOf, setWorkspaceOf] = useState<Record<string, string>>(() => ({}));
  const [workspaceTitles, setWorkspaceTitles] = useState<Record<string, string>>(() => ({}));
  const [initSelection, setInitSelection] = useState<Set<string>>(() => new Set());
  const [projectsLoaded, setProjectsLoaded] = useState(false);
  const loadRan = useRef(false);
  const gitSigRef = useRef("");

  const loadProjects = useCallback(async () => {
    try {
      const doc = parseSidebarDocument(await api.readProjects());
      setProjects(doc.assignments);
      setProjectNames(doc.names);
      setProjectContexts(doc.contexts);
      setProjectPaths(doc.paths);
      setProjectGroups(doc.groups);
      setProjectTypes(doc.types);
      setAgentOf(doc.agents);
      setWorkspaceOf(doc.workspaces);
      setWorkspaceTitles(doc.titles);
      setInitSelection(doc.selected);
      setActiveProject(doc.active);
    } catch (e) {
      console.error("Failed to load projects:", e);
    } finally {
      setProjectsLoaded(true);
    }
  }, []);

  useEffect(() => {
    if (loadRan.current) return;
    loadRan.current = true;
    loadProjects();
  }, [loadProjects]);

  useEffect(() => {
    if (!projectsLoaded) return;
    // ponytail: only rescan git when a git-relevant field changed - a checkbox toggle persists `selected` without a full `git status` sweep.
    const sig = JSON.stringify([projectNames, projectPaths, projectGroups]);
    const gitChanged = sig !== gitSigRef.current;
    gitSigRef.current = sig;
    api
      .writeProjects(
        serializeSidebarDocument({
          assignments: projects,
          names: projectNames,
          contexts: projectContexts,
          paths: projectPaths,
          groups: projectGroups,
          types: projectTypes,
          agents: agentOf,
          workspaces: workspaceOf,
          titles: workspaceTitles,
          selected: initSelection,
          active: activeProject,
        }),
      )
      .then(() => (gitChanged ? fetchGitProjects() : undefined))
      .catch(() => {});
  }, [
    projectsLoaded,
    projects,
    projectNames,
    projectContexts,
    projectPaths,
    projectGroups,
    projectTypes,
    agentOf,
    workspaceOf,
    workspaceTitles,
    initSelection,
    activeProject,
    fetchGitProjects,
  ]);

  const placeInGroup = useCallback((name: string, group: string, path: string) => {
    setProjectGroups((prev) => ({ ...prev, [name]: group }));
    setProjectPaths((prev) => ({ ...prev, [name]: path }));
  }, []);

  const createProject = useCallback(
    (name: string, group?: string) => {
      setProjectNames((prev) => (prev.includes(name) ? prev : [...prev, name]));
      api
        .createProjectDir(name, group)
        .then((path) => group && placeInGroup(name, group, path))
        .catch((e) => console.error("Failed to create project directory:", e));
    },
    [placeInGroup],
  );

  const importProjects = useCallback(
    (repos: { name: string; path: string; group?: string; context?: string | null }[]) => {
      setProjectNames((prev) => {
        const fresh = repos.map((r) => r.name).filter((n) => !prev.includes(n));
        return fresh.length ? [...prev, ...fresh] : prev;
      });
      setProjectPaths((prev) => {
        const next = { ...prev };
        for (const r of repos) next[r.name] ??= r.path;
        return next;
      });
      setProjectContexts((prev) => {
        const next = { ...prev };
        for (const r of repos) if (r.context) next[r.name] ??= r.context;
        return next;
      });
      setProjectGroups((prev) => {
        const next = { ...prev };
        for (const r of repos) if (r.group) next[r.name] ??= r.group;
        return next;
      });
    },
    [],
  );

  const deleteProjects = useCallback((names: string[]) => {
    const gone = new Set(names);
    setProjects((prev) => Object.fromEntries(Object.entries(prev).filter(([, p]) => !gone.has(p))));
    setProjectNames((prev) => prev.filter((n) => !gone.has(n)));
    setProjectContexts((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setProjectPaths((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setProjectGroups((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setProjectTypes((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setActiveProject((p) => (p && gone.has(p) ? null : p));
  }, []);

  const deleteProject = useCallback((name: string) => deleteProjects([name]), [deleteProjects]);

  const renameProject = useCallback((oldName: string, newName: string) => {
    setProjects((prev) => Object.fromEntries(Object.entries(prev).map(([id, p]) => [id, p === oldName ? newName : p])));
    setProjectNames((prev) => prev.map((n) => (n === oldName ? newName : n)));
    setProjectContexts((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setProjectPaths((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setProjectGroups((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setProjectTypes((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setActiveProject((p) => (p === oldName ? newName : p));
  }, []);

  const moveProject = useCallback(
    (name: string, group: string) => {
      const oldPrefix = projectGroups[name] ? `${projectGroups[name]}/` : "";
      const renamed = name.startsWith(oldPrefix) && oldPrefix ? `${group}/${name.slice(oldPrefix.length)}` : name;
      api
        .moveProject(name, group)
        .then((path) => {
          if (renamed !== name) renameProject(name, renamed);
          placeInGroup(renamed, group, path);
        })
        .catch((e) => setError(String(e)));
    },
    [placeInGroup, projectGroups, renameProject, setError],
  );

  const setProjectContext = useCallback((name: string, context: string) => {
    setProjectContexts((prev) => ({ ...prev, [name]: context }));
  }, []);

  const setProjectPath = useCallback((name: string, path: string) => {
    setProjectPaths((prev) => {
      if (!path.trim()) {
        const next = { ...prev };
        delete next[name];
        return next;
      }
      return { ...prev, [name]: path };
    });
  }, []);

  const toggleCollapseProject = useCallback((name: string) => {
    setCollapsedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  return useMemo(
    () => ({
      projects,
      projectNames,
      collapsedProjects,
      activeProject,
      projectContexts,
      projectPaths,
      projectGroups,
      projectTypes,
      agentOf,
      workspaceOf,
      workspaceTitles,
      initSelection,
      loadProjects,
      setProjects,
      setProjectNames,
      setActiveProject,
      setProjectTypes,
      setAgentOf,
      setWorkspaceOf,
      setWorkspaceTitles,
      setInitSelection,
      createProject,
      importProjects,
      deleteProjects,
      deleteProject,
      renameProject,
      moveProject,
      setProjectContext,
      setProjectPath,
      toggleCollapseProject,
    }),
    [
      projects,
      projectNames,
      collapsedProjects,
      activeProject,
      projectContexts,
      projectPaths,
      projectGroups,
      projectTypes,
      agentOf,
      workspaceOf,
      workspaceTitles,
      initSelection,
      loadProjects,
      createProject,
      importProjects,
      deleteProjects,
      deleteProject,
      renameProject,
      moveProject,
      setProjectContext,
      setProjectPath,
      toggleCollapseProject,
    ],
  );
}

type SidebarStore = ReturnType<typeof useSidebarStore>;

const SidebarContext = createContext<SidebarStore | null>(null);

export function SidebarProvider({ children }: { children: ReactNode }) {
  return <SidebarContext.Provider value={useSidebarStore()}>{children}</SidebarContext.Provider>;
}

/** The sidebar document (`projects.yaml`): the project catalog and, per chat,
 * its project, agent, workspace and title. This store loads and saves it, the
 * raw setters are for the session code that files chats under projects. */
export function useSidebar(): SidebarStore {
  const ctx = useContext(SidebarContext);
  if (!ctx) throw new Error("useSidebar must be used within SidebarProvider");
  return ctx;
}
