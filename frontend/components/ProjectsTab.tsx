import { useEffect, useState } from "react";
import { GitBranch, Paperclip } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { fmtBytes } from "@/lib/timeline";
import { api, type DesktopConfig, type GitRepo, type ProjectFile, type RepoEntry } from "@/lib/tauri";
import { useDesktop, type ProjectType } from "@/store";
import { useSidebar } from "@/stores/sidebar";
import { useGit } from "@/stores/git";
import { DEFAULT_CONFIG } from "@/lib/desktop-config";

export function ProjectsTab() {
  const { gitProjects, dirtyProjects } = useGit();
  const {
    projectNames,
    projectContexts,
    setProjectContext,
    projectPaths,
    setProjectPath,
    importProjects,
    deleteProjects,
    projectGroups,
    projectTypes,
  } = useSidebar();
  const { setProjectType, initialProjectFilter } = useDesktop();
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [scanOpen, setScanOpen] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [repos, setRepos] = useState<GitRepo[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [ghOpen, setGhOpen] = useState(false);
  const [ghLoading, setGhLoading] = useState(false);
  const [ghHint, setGhHint] = useState("");
  const [ghOwners, setGhOwners] = useState<string[]>([]);
  const [ghOwner, setGhOwner] = useState("");
  const [ghRepos, setGhRepos] = useState<RepoEntry[]>([]);
  const [ghSelected, setGhSelected] = useState<Set<string>>(() => new Set());
  const [cloning, setCloning] = useState("");
  const [filter, setFilter] = useState(initialProjectFilter);

  const importedPaths = new Set(Object.values(projectPaths));
  const isImported = (r: GitRepo) => projectNames.includes(r.name) || importedPaths.has(r.path);
  const ghImported = (name: string) => projectNames.includes(name);

  const scan = async () => {
    setScanning(true);
    setError("");
    try {
      const found = await api.scanGitRepos(config.projects_root);
      setRepos(found);
      setSelected(new Set(found.filter((r) => !isImported(r)).map((r) => r.path)));
      setScanOpen(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  };

  const importSelected = () => {
    importProjects(repos.filter((r) => selected.has(r.path)));
    setScanOpen(false);
  };

  const loadGhRepos = async (owner: string) => {
    setGhOwner(owner);
    setGhLoading(true);
    try {
      const found = await api.githubListRepos(owner);
      setGhRepos(found);
      setGhSelected(new Set());
    } catch (e) {
      setGhRepos([]);
      setGhSelected(new Set());
      setError(String(e));
    } finally {
      setGhLoading(false);
    }
  };

  const openGh = async () => {
    setError("");
    setGhHint("");
    setGhOpen(true);
    try {
      const status = await api.githubAuthStatus();
      if (!status.installed || !status.authenticated) {
        setGhHint("Install and authenticate the GitHub CLI in Settings > Tasks.");
        return;
      }
      const owners = await api.githubOwners();
      setGhOwners(owners);
      if (owners.length > 0) await loadGhRepos(owners[0]);
    } catch (e) {
      setError(String(e));
    }
  };

  const importGithub = async () => {
    const names = ghRepos.filter((r) => ghSelected.has(r.name)).map((r) => r.name);
    const cloned: GitRepo[] = [];
    setError("");
    for (let i = 0; i < names.length; i++) {
      setCloning(`${i + 1}/${names.length}`);
      try {
        cloned.push(await api.cloneGithubRepo(`${ghOwner}/${names[i]}`));
      } catch (e) {
        setError(String(e));
        break;
      }
    }
    setCloning("");
    if (cloned.length > 0) importProjects(cloned);
    const done = new Set(cloned.map((c) => c.name));
    setGhSelected((prev) => new Set([...prev].filter((n) => !done.has(n))));
    if (cloned.length === names.length) setGhOpen(false);
  };

  const [marked, setMarked] = useState<Set<string>>(() => new Set());
  const query = filter.trim().toLowerCase();
  const visible = query ? projectNames.filter((n) => n.toLowerCase().includes(query)) : projectNames;
  const toggleMarked = (name: string, on: boolean) =>
    setMarked((prev) => {
      const next = new Set(prev);
      if (on) next.add(name);
      else next.delete(name);
      return next;
    });
  const deleteMarked = () => {
    deleteProjects(Array.from(marked));
    setMarked(new Set());
  };

  useEffect(() => {
    api
      .getConfig()
      .then(setConfigs)
      .catch(() => {});
  }, []);

  const set = (k: keyof DesktopConfig, v: string) => {
    setConfigs((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setError("");
  };

  const apply = async () => {
    setSaving(true);
    try {
      await api.setConfig(config);
      setDirty(false);
      setSaved(true);
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Projects</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Per-project context sent as extra instructions with every message in that project. Changes save automatically.
      </p>

      <h3 className="text-[0.9rem] font-semibold">Files</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Every project gets its own directory under the root below, pre-authorized in the agent sandbox. Applies to newly
        created projects.
      </p>
      <div className="mb-4 flex flex-col gap-[0.7rem]">
        <div className="flex flex-col gap-1">
          <Label htmlFor="projects-root" className="text-[0.8rem] text-muted-foreground">
            Projects root directory
          </Label>
          <Input
            id="projects-root"
            value={config.projects_root}
            placeholder="~/Documents/Inference Gateway Desktop"
            onChange={(e) => set("projects_root", e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="projects-backend" className="text-[0.8rem] text-muted-foreground">
            Storage backend
          </Label>
          <select
            id="projects-backend"
            value={config.projects_backend}
            onChange={(e) => set("projects_backend", e.target.value)}
            className="h-9 rounded-md border border-input bg-transparent px-3 text-[0.85rem] text-foreground outline-none focus-visible:border-ring"
          >
            <option value="local">Local filesystem</option>
            <option value="github">GitHub repository</option>
          </select>
        </div>
        {config.projects_backend === "github" && (
          <div className="flex flex-col gap-1">
            <Label htmlFor="projects-repo" className="text-[0.8rem] text-muted-foreground">
              Repository (one folder per project; created if missing)
            </Label>
            <Input
              id="projects-repo"
              value={config.projects_github_repository}
              placeholder=".projects"
              onChange={(e) => set("projects_github_repository", e.target.value)}
            />
          </div>
        )}
        <div className="flex flex-col gap-1">
          <Label htmlFor="projects-max-size" className="text-[0.8rem] text-muted-foreground">
            Max file size (MB)
          </Label>
          <Input
            id="projects-max-size"
            value={config.projects_max_file_size_mb}
            placeholder="10"
            onChange={(e) => set("projects_max_file_size_mb", e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="projects-allowed-mimes" className="text-[0.8rem] text-muted-foreground">
            Allowed file types (comma-separated extensions)
          </Label>
          <Input
            id="projects-allowed-mimes"
            value={config.projects_allowed_mimes}
            onChange={(e) => set("projects_allowed_mimes", e.target.value)}
          />
        </div>
        {error && (
          <div role="status" className="text-[0.75rem] text-err">
            {error}
          </div>
        )}
        <div className="flex items-center gap-2">
          <Button size="sm" disabled={saving} onClick={apply}>
            {saving ? "Saving..." : "Save"}
          </Button>
          <Button size="sm" variant="outline" disabled={scanning} onClick={scan}>
            {scanning ? "Scanning..." : "Import git repos"}
          </Button>
          <Button size="sm" variant="outline" onClick={openGh}>
            Import GitHub repos
          </Button>
          {saved && !dirty && <span className="text-[0.75rem] text-muted-foreground">Saved</span>}
        </div>
      </div>

      <Dialog open={scanOpen} onOpenChange={setScanOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Import git repositories</DialogTitle>
            <DialogDescription>
              {repos.length === 0
                ? "No git repositories found under the projects root."
                : "Git repositories found under the projects root. Selected ones are added as projects."}
            </DialogDescription>
          </DialogHeader>
          {repos.length > 0 && (
            <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
              {repos.map((r) => {
                const done = isImported(r);
                return (
                  <label
                    key={r.path}
                    className={cn(
                      "flex items-center gap-2 rounded-md px-1.5 py-1 text-[0.8rem]",
                      done ? "text-muted-foreground/60" : "cursor-pointer hover:bg-secondary",
                    )}
                  >
                    <input
                      type="checkbox"
                      disabled={done}
                      checked={done || selected.has(r.path)}
                      onChange={(e) =>
                        setSelected((prev) => {
                          const next = new Set(prev);
                          if (e.target.checked) next.add(r.path);
                          else next.delete(r.path);
                          return next;
                        })
                      }
                    />
                    <GitBranch size={12} className="shrink-0" />
                    <span className="shrink-0">{r.name}</span>
                    <span className="min-w-0 truncate text-muted-foreground">{r.path}</span>
                    {done && <span className="ml-auto shrink-0 text-[0.7rem]">imported</span>}
                  </label>
                );
              })}
            </div>
          )}
          <DialogFooter showCloseButton>
            {repos.length > 0 && (
              <Button size="sm" disabled={selected.size === 0} onClick={importSelected}>
                Import {selected.size} selected
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={ghOpen} onOpenChange={setGhOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Import GitHub repositories</DialogTitle>
            <DialogDescription>
              {ghHint
                ? ghHint
                : `Repositories owned by ${ghOwner || "your GitHub account"} are cloned into the projects root and added as projects.`}
            </DialogDescription>
          </DialogHeader>
          {!ghHint && (
            <>
              <div className="flex flex-col gap-1">
                <Label htmlFor="gh-owner" className="text-[0.8rem] text-muted-foreground">
                  Owner
                </Label>
                <select
                  id="gh-owner"
                  value={ghOwner}
                  onChange={(e) => loadGhRepos(e.target.value)}
                  className="h-9 rounded-md border border-input bg-transparent px-3 text-[0.85rem] text-foreground outline-none focus-visible:border-ring"
                >
                  {ghOwners.map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
              </div>
              {!ghLoading &&
                (ghRepos.length === 0 ? (
                  <p className="text-[0.8rem] text-muted-foreground">No repositories found for {ghOwner}.</p>
                ) : (
                  <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
                    {ghRepos.map((r) => {
                      const done = ghImported(r.name);
                      return (
                        <label
                          key={r.name}
                          className={cn(
                            "flex items-center gap-2 rounded-md px-1.5 py-1 text-[0.8rem]",
                            done ? "text-muted-foreground/60" : "cursor-pointer hover:bg-secondary",
                          )}
                        >
                          <input
                            type="checkbox"
                            disabled={done}
                            checked={done || ghSelected.has(r.name)}
                            onChange={(e) =>
                              setGhSelected((prev) => {
                                const next = new Set(prev);
                                if (e.target.checked) next.add(r.name);
                                else next.delete(r.name);
                                return next;
                              })
                            }
                          />
                          <GitBranch size={12} className="shrink-0" />
                          <span className="shrink-0">{r.name}</span>
                          {done && <span className="ml-auto shrink-0 text-[0.7rem]">imported</span>}
                        </label>
                      );
                    })}
                  </div>
                ))}
              {error && (
                <div role="status" className="text-[0.75rem] text-err">
                  {error}
                </div>
              )}
            </>
          )}
          <DialogFooter showCloseButton>
            {!ghHint && (
              <Button size="sm" disabled={ghLoading || ghSelected.size === 0 || cloning !== ""} onClick={importGithub}>
                {cloning ? `Cloning ${cloning}...` : `Import ${ghSelected.size} selected`}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {projectNames.length === 0 ? (
        <p className="text-[0.8rem] text-muted-foreground">
          No projects yet. Create one from the sidebar with "New project".
        </p>
      ) : (
        <div className="flex flex-col gap-4">
          <Input
            id="project-search"
            aria-label="Search projects"
            value={filter}
            placeholder="Search projects"
            className="max-w-sm"
            onChange={(e) => {
              setFilter(e.target.value);
              setMarked(new Set());
            }}
          />
          <div className="flex items-center gap-2">
            <label className="flex cursor-pointer items-center gap-2 text-[0.8rem] text-muted-foreground">
              <input
                type="checkbox"
                aria-label="Select all projects"
                checked={visible.length > 0 && visible.every((n) => marked.has(n))}
                onChange={(e) => setMarked(e.target.checked ? new Set(visible) : new Set())}
              />
              Select all
            </label>
            {marked.size > 0 && (
              <>
                <Button size="sm" variant="destructive" onClick={deleteMarked}>
                  Delete {marked.size} selected
                </Button>
                <span className="text-[0.75rem] text-muted-foreground">
                  Removes projects from the app only - files on disk are untouched.
                </span>
              </>
            )}
          </div>
          {visible.length === 0 && <p className="text-[0.8rem] text-muted-foreground">No projects match "{filter}".</p>}
          {visible.map((name) => (
            <div key={name} className="rounded-lg border border-border bg-card p-4">
              <div className="flex flex-col gap-1">
                <div className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    aria-label={`Select project ${name}`}
                    checked={marked.has(name)}
                    onChange={(e) => toggleMarked(name, e.target.checked)}
                  />
                  <Label htmlFor={`project-context-${name}`} className="text-[0.8rem] font-medium">
                    {name}
                  </Label>
                  {gitProjects.has(name) && (
                    <span className="inline-flex h-5 items-center gap-1 rounded-md border border-border bg-secondary px-1.5 text-[0.68rem] text-muted-foreground">
                      <GitBranch size={10} className={cn("shrink-0", dirtyProjects.has(name) && "text-amber-500")} />
                      git
                    </span>
                  )}
                  {projectGroups[name] && (
                    <span className="inline-flex h-5 items-center rounded-md border border-border bg-secondary px-1.5 text-[0.68rem] text-muted-foreground">
                      {projectGroups[name]}
                    </span>
                  )}
                  <select
                    aria-label={`Type of project ${name}`}
                    value={projectTypes[name] ?? "code"}
                    onChange={(e) => setProjectType(name, e.target.value as ProjectType)}
                    className="ml-auto h-6 rounded-md border border-input bg-transparent px-1 text-[0.72rem] text-foreground"
                  >
                    <option value="code">Code</option>
                    <option value="content">Content</option>
                  </select>
                </div>
                {projectTypes[name] === "content" &&
                  (!config.text_to_speech_enabled || !config.vision_annotator_model) && (
                    <p role="alert" className="text-[0.75rem] text-amber-600 dark:text-amber-400">
                      Content projects need{" "}
                      {[
                        !config.text_to_speech_enabled && "Text to Speech enabled",
                        !config.vision_annotator_model && "a vision model",
                      ]
                        .filter(Boolean)
                        .join(" and ")}{" "}
                      - see the General tab.
                    </p>
                  )}
                <ProjectFiles project={name} />
                <Label htmlFor={`project-path-${name}`} className="text-[0.8rem] text-muted-foreground">
                  Directory (blank = default under the projects root)
                </Label>
                <Input
                  id={`project-path-${name}`}
                  value={projectPaths[name] ?? ""}
                  placeholder="/path/to/existing/repo"
                  onChange={(e) => setProjectPath(name, e.target.value)}
                />
                <textarea
                  id={`project-context-${name}`}
                  rows={4}
                  value={projectContexts[name] ?? ""}
                  onChange={(e) => setProjectContext(name, e.target.value)}
                  placeholder="e.g. This project is about... Always use..."
                  className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

// Files stored in a project's directory (local) or repository (github),
// rendered as attachment-style chips under the project's context.
function ProjectFiles({ project }: { project: string }) {
  const [files, setFiles] = useState<ProjectFile[] | null>(null);
  useEffect(() => {
    api
      .listProjectFiles(project)
      .then(setFiles)
      .catch(() => setFiles([]));
  }, [project]);
  if (!files?.length) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      {files.map((f) => (
        <span
          key={f.name}
          className="inline-flex h-7 max-w-full items-center gap-1.5 rounded-md border border-border bg-secondary px-2 text-[0.72rem] text-muted-foreground"
        >
          <Paperclip size={11} className="shrink-0" />
          <span className="min-w-0 truncate">{f.name}</span>
          <span className="shrink-0 tabular-nums">{fmtBytes(f.size)}</span>
        </span>
      ))}
    </div>
  );
}
