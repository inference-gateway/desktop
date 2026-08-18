import { Fragment, useCallback, useEffect, useState } from "react";
import { ExternalLink, Play, RotateCw } from "lucide-react";
import {
  api,
  type TaskIssue,
  type WorkflowRun,
  type WorkflowStatus,
} from "@/lib/tauri";
import { Button } from "@/components/ui/button";

const TASKS_REPO_KEY = "tasksRepo";

// Long-horizon tasks panel (Settings -> GitHub -> Tasks): pick any repository
// you own, install the infer-action workflow into it, then create GitHub
// issues the workflow picks up and watch the resulting issues and runs.
export function TasksPanel() {
  const [owners, setOwners] = useState<string[]>([]);
  const [owner, setOwner] = useState(() => {
    const saved = localStorage.getItem(TASKS_REPO_KEY) || "";
    return saved.split("/")[0] || "";
  });
  const [repos, setRepos] = useState<string[]>([]);
  const [name, setName] = useState(() => {
    const saved = localStorage.getItem(TASKS_REPO_KEY) || "";
    return saved.split("/")[1] || "";
  });
  const [agentModel, setAgentModel] = useState("");
  const repo = owner && name ? `${owner}/${name}` : "";

  const [issues, setIssues] = useState<TaskIssue[]>([]);
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [loadError, setLoadError] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdUrl, setCreatedUrl] = useState("");
  const [createError, setCreateError] = useState("");
  const [installed, setInstalled] = useState(false);
  const [runningIssue, setRunningIssue] = useState(0);
  const [triggeredIssue, setTriggeredIssue] = useState(0);
  const [commentIssue, setCommentIssue] = useState(0);
  const [comment, setComment] = useState("");

  useEffect(() => {
    api
      .githubOwners()
      .then((list) => {
        setOwners(list);
        setOwner((o) => (o && list.includes(o) ? o : (list[0] ?? "")));
      })
      .catch(() => {});
    api
      .getConfig()
      .then((cfg) => setAgentModel(cfg.agent_model))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!owner) return;
    setRepos([]);
    api
      .githubListRepos(owner)
      .then((list) => {
        setRepos(list);
        setName((n) => (n && list.includes(n) ? n : (list[0] ?? "")));
      })
      .catch((e) => setLoadError(String(e)));
  }, [owner]);

  useEffect(() => {
    if (repo) localStorage.setItem(TASKS_REPO_KEY, repo);
  }, [repo]);

  const refresh = useCallback((r: string) => {
    if (!r) return;
    setLoadError("");
    Promise.all([api.githubListTaskIssues(r), api.githubListWorkflowRuns(r)])
      .then(([i, w]) => {
        setIssues(i);
        setRuns(w);
      })
      .catch((e) => setLoadError(String(e)));
  }, []);

  useEffect(() => {
    setIssues([]);
    setRuns([]);
    setCreatedUrl("");
    setCreateError("");
    setInstalled(false);
    setTriggeredIssue(0);
    setCommentIssue(0);
    refresh(repo);
  }, [repo, refresh]);

  const openComment = (number: number) => {
    setCommentIssue(number);
    setComment("@opentask Can you work on this?");
    setTriggeredIssue(0);
  };

  const runTask = async (number: number) => {
    setRunningIssue(number);
    try {
      await api.githubRunTaskIssue(repo, number, comment);
      setTriggeredIssue(number);
      setCommentIssue(0);
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setRunningIssue(0);
    }
  };

  const createTask = async () => {
    setCreating(true);
    setCreateError("");
    setCreatedUrl("");
    try {
      const url = await api.githubCreateTaskIssue(repo, title, body);
      setCreatedUrl(url);
      setTitle("");
      setBody("");
      refresh(repo);
    } catch (e) {
      setCreateError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <>
      <p className="mb-3 text-[0.8rem] text-muted-foreground">
        Tasks are GitHub issues the installed infer-action workflow picks up.
        Pick a repository, install the workflow, then create tasks below.
      </p>
      <div className="mb-2 flex items-center gap-2">
        <select
          id="tasks-owner"
          aria-label="Tasks repository owner"
          value={owner}
          onChange={(e) => setOwner(e.target.value)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {owners.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
        <span className="text-muted-foreground">/</span>
        <select
          id="tasks-repo"
          aria-label="Tasks repository"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="min-w-0 flex-1 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {repos.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <Button
          variant="ghost"
          size="icon-sm"
          title="Refresh tasks"
          aria-label="Refresh tasks"
          onClick={() => refresh(repo)}
          className="shrink-0 text-muted-foreground"
        >
          <RotateCw size={16} />
        </Button>
      </div>

      {repo && (
        <InferActionInstall
          repository={repo}
          model={agentModel}
          onStatus={(s) => setInstalled(s.installed)}
        />
      )}

      {repo && (
        <>
          <div className="mb-6 mt-5 flex flex-col gap-2">
            <label
              htmlFor="task-title"
              className="text-[0.8rem] text-muted-foreground"
            >
              New task
            </label>
            <input
              id="task-title"
              aria-label="Task title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Task title"
              className="rounded-md border border-border bg-background px-3 py-2 text-[0.85rem]"
            />
            <textarea
              id="task-body"
              aria-label="Task description"
              rows={4}
              value={body}
              onChange={(e) => setBody(e.target.value)}
              placeholder="Describe the task for the agent…"
              className="rounded-md border border-border bg-background px-3 py-2 text-[0.85rem]"
            />
            <div className="flex items-center gap-3">
              <Button onClick={createTask} disabled={creating || !title.trim()}>
                {creating ? "Creating..." : "Create task"}
              </Button>
              {createdUrl && (
                <button
                  onClick={() => api.openUrl(createdUrl)}
                  className="inline-flex items-center gap-1 text-[0.8rem] text-primary underline"
                >
                  Issue created <ExternalLink size={12} />
                </button>
              )}
              {createError && (
                <span role="status" className="text-[0.8rem] text-err">
                  {createError}
                </span>
              )}
            </div>
          </div>

          {loadError && (
            <p role="status" className="mb-3 text-[0.8rem] text-err">
              {loadError}
            </p>
          )}

          <section className="mb-6">
            <h3 className="mb-2 text-[0.9rem] font-semibold">Tasks</h3>
            {issues.length === 0 && (
              <p className="text-[0.8rem] text-muted-foreground">
                No task issues yet.
              </p>
            )}
            <ul className="flex flex-col gap-1">
              {issues.map((i) => (
                <Fragment key={i.number}>
                  <li className="group flex w-full items-center gap-2 rounded-md px-2 py-[0.4rem] text-[0.85rem] hover:bg-primary/10">
                    <span
                      className={
                        "h-2 w-2 shrink-0 rounded-full " +
                        (i.state === "open"
                          ? "bg-emerald-500"
                          : "bg-muted-foreground")
                      }
                    />
                    <span className="text-muted-foreground">#{i.number}</span>
                    <span className="truncate">{i.title}</span>
                    <span className="ml-auto flex shrink-0 items-center gap-1">
                      {triggeredIssue === i.number && (
                        <span
                          role="status"
                          className="text-[0.7rem] text-muted-foreground"
                        >
                          Triggered
                        </span>
                      )}
                      {installed && (
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title="Run task"
                          aria-label="Run task"
                          onClick={() =>
                            commentIssue === i.number
                              ? setCommentIssue(0)
                              : openComment(i.number)
                          }
                          className="text-muted-foreground hover:text-foreground"
                        >
                          <Play size={14} />
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Open task"
                        aria-label="Open task"
                        onClick={() => api.openUrl(i.html_url)}
                        className="text-muted-foreground hover:text-foreground"
                      >
                        <ExternalLink size={14} />
                      </Button>
                    </span>
                  </li>
                  {commentIssue === i.number && (
                    <li className="ml-6 flex flex-col gap-2 pb-2">
                      <textarea
                        aria-label="Task comment"
                        rows={2}
                        value={comment}
                        onChange={(e) => setComment(e.target.value)}
                        className="rounded-md border border-border bg-background px-3 py-2 text-[0.85rem]"
                      />
                      <div className="flex items-center gap-2">
                        <Button
                          size="xs"
                          disabled={
                            runningIssue === i.number || !comment.trim()
                          }
                          onClick={() => runTask(i.number)}
                        >
                          {runningIssue === i.number ? "Sending..." : "Send"}
                        </Button>
                        <Button
                          size="xs"
                          variant="outline"
                          onClick={() => setCommentIssue(0)}
                        >
                          Cancel
                        </Button>
                      </div>
                    </li>
                  )}
                </Fragment>
              ))}
            </ul>
          </section>
          <section>
            <h3 className="mb-2 text-[0.9rem] font-semibold">Runs</h3>
            {runs.length === 0 && (
              <p className="text-[0.8rem] text-muted-foreground">
                No workflow runs yet.
              </p>
            )}
            <ul className="flex flex-col gap-1">
              {runs.map((r) => (
                <li key={r.id}>
                  <button
                    onClick={() => api.openUrl(r.html_url)}
                    className="flex w-full items-center gap-2 rounded-md px-2 py-[0.4rem] text-left text-[0.85rem] hover:bg-primary/10"
                  >
                    <span
                      className={
                        "h-2 w-2 shrink-0 rounded-full " +
                        (r.conclusion === "success"
                          ? "bg-emerald-500"
                          : r.conclusion === "failure"
                            ? "bg-red-500"
                            : "bg-amber-500")
                      }
                    />
                    <span className="truncate">{r.name}</span>
                    <span className="ml-auto shrink-0 text-[0.75rem] text-muted-foreground">
                      {r.conclusion ?? r.status}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </>
  );
}

// Checks whether the infer-action task workflow exists in the repository and
// installs/updates it via a pull request (branch + commit + PR), like the
// opentask extension.
function InferActionInstall({
  repository,
  model,
  onStatus,
}: {
  repository: string;
  model: string;
  onStatus?: (s: WorkflowStatus) => void;
}) {
  const [status, setStatus] = useState<WorkflowStatus | null>(null);
  const [checkError, setCheckError] = useState("");
  const [installing, setInstalling] = useState(false);
  const [bumping, setBumping] = useState(false);
  const [prUrl, setPrUrl] = useState("");
  const [installError, setInstallError] = useState("");
  const [showOptions, setShowOptions] = useState(false);
  const [apt, setApt] = useState("");
  const [visionModel, setVisionModel] = useState(
    "anthropic/claude-haiku-4-5-20251001",
  );
  const [imageModel, setImageModel] = useState("");

  useEffect(() => {
    setStatus(null);
    setCheckError("");
    setPrUrl("");
    setInstallError("");
    if (!repository.includes("/")) return;
    const t = setTimeout(() => {
      api
        .githubCheckWorkflow(repository)
        .then((s) => {
          setStatus(s);
          onStatus?.(s);
        })
        .catch((e) => setCheckError(String(e)));
    }, 500);
    return () => clearTimeout(t);
  }, [repository]);

  const install = async () => {
    setInstalling(true);
    setInstallError("");
    setPrUrl("");
    try {
      setPrUrl(
        await api.githubInstallWorkflow(
          repository,
          model,
          apt,
          visionModel,
          imageModel,
        ),
      );
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  };

  const bump = async () => {
    setBumping(true);
    setInstallError("");
    setPrUrl("");
    try {
      setPrUrl(await api.githubBumpWorkflow(repository));
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setBumping(false);
    }
  };

  const outdated =
    status?.installed &&
    status.version &&
    status.latest &&
    status.version !== status.latest;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2 text-[0.75rem]">
        <span
          className={
            "h-2 w-2 shrink-0 rounded-full " +
            (status?.installed
              ? outdated
                ? "bg-amber-500"
                : "bg-emerald-500"
              : "bg-amber-500")
          }
        />
        <span className="text-muted-foreground">
          {status === null &&
            !checkError &&
            "Checking infer-action workflow..."}
          {status?.installed &&
            !outdated &&
            `infer-action workflow installed${status.version ? ` (${status.version})` : ""}.`}
          {status?.installed &&
            outdated &&
            `infer-action ${status.version} installed - ${status.latest} available.`}
          {status !== null &&
            !status.installed &&
            "infer-action workflow not installed."}
          {checkError && `Couldn't check workflow: ${checkError}`}
        </span>
        {outdated && (
          <Button variant="outline" size="xs" disabled={bumping} onClick={bump}>
            {bumping ? "Opening PR..." : `Bump to ${status.latest}`}
          </Button>
        )}
        {status?.installed && status.url && (
          <Button
            variant="outline"
            size="xs"
            onClick={() => api.openUrl(status.url!)}
          >
            View workflow
          </Button>
        )}
        {status !== null && (
          <Button
            variant="outline"
            size="xs"
            disabled={installing}
            onClick={install}
          >
            {installing
              ? "Installing..."
              : status.installed
                ? "Re-install infer-action"
                : "Install infer-action"}
          </Button>
        )}
        {status !== null && (
          <button
            onClick={() => setShowOptions((s) => !s)}
            className="text-muted-foreground underline hover:text-foreground"
          >
            {showOptions ? "Hide options" : "Options"}
          </button>
        )}
        {prUrl && (
          <Button
            variant="outline"
            size="xs"
            onClick={() => api.openUrl(prUrl)}
          >
            View PR
          </Button>
        )}
        {installError && (
          <span role="status" className="text-err">
            {installError}
          </span>
        )}
      </div>
      {showOptions && (
        <div className="flex flex-col gap-2 rounded-md border border-border p-3">
          <p className="text-[0.7rem] text-muted-foreground">
            Written into the workflow on install/re-install. Leave empty to
            omit.
          </p>
          <label
            htmlFor="install-apt"
            className="text-[0.75rem] text-muted-foreground"
          >
            Extra apt packages (space-separated)
          </label>
          <input
            id="install-apt"
            aria-label="Extra apt packages"
            value={apt}
            onChange={(e) => setApt(e.target.value)}
            placeholder="libwebkit2gtk-4.1-dev ffmpeg"
            className="rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem]"
          />
          <label
            htmlFor="install-vision-model"
            className="text-[0.75rem] text-muted-foreground"
          >
            Vision model (image analysis)
          </label>
          <input
            id="install-vision-model"
            aria-label="Vision model"
            value={visionModel}
            onChange={(e) => setVisionModel(e.target.value)}
            placeholder="anthropic/claude-haiku-4-5-20251001"
            className="rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem]"
          />
          <label
            htmlFor="install-image-model"
            className="text-[0.75rem] text-muted-foreground"
          >
            Image generation model
          </label>
          <input
            id="install-image-model"
            aria-label="Image generation model"
            value={imageModel}
            onChange={(e) => setImageModel(e.target.value)}
            placeholder="openai/gpt-image-2"
            className="rounded-md border border-border bg-background px-2 py-1.5 text-[0.8rem]"
          />
        </div>
      )}
    </div>
  );
}
