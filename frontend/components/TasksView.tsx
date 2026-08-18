import { useCallback, useEffect, useState } from "react";
import { ExternalLink, RotateCw } from "lucide-react";
import { api, type TaskIssue, type WorkflowRun } from "@/lib/tauri";
import { Button } from "@/components/ui/button";

// Long-horizon tasks panel (Settings -> GitHub -> Tasks): create GitHub
// issues the installed infer-action workflow picks up, and watch the
// resulting issues and workflow runs.
export function TasksPanel({ onConfigure }: { onConfigure: () => void }) {
  const [repo, setRepo] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [issues, setIssues] = useState<TaskIssue[]>([]);
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [loadError, setLoadError] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdUrl, setCreatedUrl] = useState("");
  const [createError, setCreateError] = useState("");

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
    api
      .getConfig()
      .then((cfg) => {
        setRepo(cfg.scheduler_github_repository);
        setLoaded(true);
        refresh(cfg.scheduler_github_repository);
      })
      .catch(() => setLoaded(true));
  }, [refresh]);

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

  if (loaded && !repo) {
    return (
      <p className="text-[0.85rem] text-muted-foreground">
        No task repository configured.{" "}
        <button onClick={onConfigure} className="text-primary underline">
          Pick one in the Repository tab
        </button>{" "}
        and install the infer-action workflow.
      </p>
    );
  }

  return (
    <>
      <div className="mb-4 flex items-center gap-3">
        <p className="text-[0.8rem] text-muted-foreground">
          Tasks are GitHub issues in{" "}
          <code className="rounded bg-secondary px-1">{repo}</code> picked up by the installed
          infer-action workflow.
        </p>
        <Button
          variant="ghost"
          size="icon-sm"
          title="Refresh tasks"
          aria-label="Refresh tasks"
          onClick={() => refresh(repo)}
          className="ml-auto shrink-0 text-muted-foreground"
        >
          <RotateCw size={16} />
        </Button>
      </div>

      <div className="mb-6 flex flex-col gap-2">
        <label htmlFor="task-title" className="text-[0.8rem] text-muted-foreground">
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
          <p className="text-[0.8rem] text-muted-foreground">No task issues yet.</p>
        )}
        <ul className="flex flex-col gap-1">
          {issues.map((i) => (
            <li key={i.number}>
              <button
                onClick={() => api.openUrl(i.html_url)}
                className="flex w-full items-center gap-2 rounded-md px-2 py-[0.4rem] text-left text-[0.85rem] hover:bg-primary/10"
              >
                <span
                  className={
                    "h-2 w-2 shrink-0 rounded-full " +
                    (i.state === "open" ? "bg-emerald-500" : "bg-muted-foreground")
                  }
                />
                <span className="text-muted-foreground">#{i.number}</span>
                <span className="truncate">{i.title}</span>
              </button>
            </li>
          ))}
        </ul>
      </section>
      <section>
        <h3 className="mb-2 text-[0.9rem] font-semibold">Runs</h3>
        {runs.length === 0 && (
          <p className="text-[0.8rem] text-muted-foreground">No workflow runs yet.</p>
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
  );
}
