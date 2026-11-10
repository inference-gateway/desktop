import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { api, type DesktopConfig } from "@/lib/tauri";
import { TasksPanel } from "./TasksView";
import { DEFAULT_CONFIG, type StorageField } from "@/lib/desktop-config";

export type GithubSubTab = "repository" | "scheduling" | "tasks";

export const GITHUB_SUBTABS: { id: GithubSubTab; label: string }[] = [
  { id: "repository", label: "General" },
  { id: "scheduling", label: "Scheduling" },
  { id: "tasks", label: "Tasks" },
];

// Text inputs for the github scheduling backend; the repository picker and
// checkboxes are hand-rendered.
const SCHEDULER_GITHUB_FIELDS: readonly StorageField[] = [
  { key: "scheduler_github_artifacts_poll_interval", label: "Artifact poll interval", ph: "10m" },
  { key: "scheduler_github_artifacts_initial_delay", label: "Artifact poll initial delay", ph: "1m" },
  { key: "scheduler_github_artifacts_max_attempts", label: "Max download attempts per artifact", ph: "3" },
  { key: "scheduler_github_artifacts_rate_limit_backoff", label: "Rate-limit backoff", ph: "1h" },
];

export function GithubTab({ sub }: { sub: GithubSubTab }) {
  return (
    <>
      <h2 className="mb-4 text-[1.05rem] font-semibold">GitHub · {GITHUB_SUBTABS.find((t) => t.id === sub)?.label}</h2>
      {sub === "repository" && <GithubRepositoryPanel />}
      {sub === "scheduling" && <SchedulingTab />}
      {sub === "tasks" && <TasksPanel />}
    </>
  );
}

function SchedulingTab() {
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .getConfig()
      .then(setConfigs)
      .catch(() => {});
  }, []);

  const apply = async () => {
    setSaving(true);
    try {
      await api.setConfig(config);
      if (config.schedule_enabled) {
        await api.startScheduler();
      } else {
        await api.stopScheduler();
      }
      setDirty(false);
      setSaved(true);
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const set = (k: keyof DesktopConfig, v: string | boolean) => {
    setConfigs((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setError("");
  };

  return (
    <>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Run <code className="rounded bg-secondary px-1">infer daemon</code> locally to fire scheduled agent jobs (and
        listen on configured channels like Telegram). Job runs are recorded to your configured storage. The daemon
        starts on Save, runs while the app is open, and stops on quit.
      </p>
      <div className="mb-3 flex items-center gap-3">
        <input
          type="checkbox"
          id="schedule-enabled"
          checked={config.schedule_enabled}
          onChange={(e) => set("schedule_enabled", e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="schedule-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          {config.scheduler_backend === "github"
            ? "Enable scheduling (daemon pulls run artifacts back)"
            : "Enable local scheduling"}
        </Label>
      </div>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="scheduler-backend" className="text-[0.8rem] text-muted-foreground">
          Backend
        </Label>
        <select
          id="scheduler-backend"
          value={config.scheduler_backend}
          onChange={(e) => set("scheduler_backend", e.target.value)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          <option value="local">local</option>
          <option value="github">github</option>
        </select>
      </div>
      <div className="mb-3 flex flex-col gap-1">
        <p className="text-[0.75rem] text-muted-foreground">
          Routines repository for the github backend: schedules are deployed there as GitHub Actions workflows by the
          daemon (auto-created when missing).
        </p>
        <RepositoryPicker
          value={config.scheduler_github_repository}
          onChange={(v) => set("scheduler_github_repository", v)}
        />
      </div>
      {config.scheduler_backend === "github" && (
        <>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            Jobs are deployed as GitHub Actions workflows in your routines repository and run there via infer-action.
            Schedules fire in UTC with a minimum interval of 5 minutes. Deploy outcomes (push or pull request URL) and
            cron validation errors appear in the chat when you schedule a job.
          </p>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            GitHub CLI status and the Actions secrets the workflows need are managed in the General tab.
          </p>
          <div className="mb-3 flex flex-col gap-3">
            {SCHEDULER_GITHUB_FIELDS.map((f) => (
              <div key={f.key} className="flex flex-col gap-1">
                <Label htmlFor={f.key} className="text-[0.8rem] text-muted-foreground">
                  {f.label}
                </Label>
                <Input
                  id={f.key}
                  value={config[f.key] as string}
                  onChange={(e) => set(f.key, e.target.value)}
                  placeholder={f.ph}
                />
              </div>
            ))}
          </div>
          <div className="mb-3 flex items-center gap-3">
            <input
              type="checkbox"
              id="scheduler-github-pull-requests"
              checked={config.scheduler_github_pull_requests}
              onChange={(e) => set("scheduler_github_pull_requests", e.target.checked)}
              className="h-4 w-4 accent-primary"
            />
            <Label htmlFor="scheduler-github-pull-requests" className="cursor-pointer text-[0.8rem] font-medium">
              Open a pull request per change instead of pushing to main
            </Label>
          </div>
          <div className="mb-3 flex items-center gap-3">
            <input
              type="checkbox"
              id="scheduler-github-artifacts-enabled"
              checked={config.scheduler_github_artifacts_enabled}
              onChange={(e) => set("scheduler_github_artifacts_enabled", e.target.checked)}
              className="h-4 w-4 accent-primary"
            />
            <Label htmlFor="scheduler-github-artifacts-enabled" className="cursor-pointer text-[0.8rem] font-medium">
              Pull run conversations back into local storage
            </Label>
          </div>
        </>
      )}
      <SchedulerLogView />

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !error && <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>}
        {saved && !dirty && (
          <span role="status" className="text-[0.8rem] text-muted-foreground">
            Saved
          </span>
        )}
        {error && (
          <span role="status" className="text-[0.8rem] text-err">
            Couldn't save: {error}
          </span>
        )}
      </div>
    </>
  );
}

function GithubRepositoryPanel() {
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .getConfig()
      .then(setConfigs)
      .catch(() => {});
  }, []);

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

  const set = (k: keyof DesktopConfig, v: string | boolean) => {
    setConfigs((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setError("");
  };

  return (
    <>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        GitHub CLI status, the Actions secrets the workflows need, and the bot identity. The scheduling repository lives
        in the Scheduling tab; task repositories in the Tasks tab.
      </p>
      <GithubPrereqs
        repository={config.scheduler_github_repository}
        clientIdSecret={config.scheduler_github_app_client_id_secret}
        privateKeySecret={config.scheduler_github_app_private_key_secret}
        onClientIdSecret={(v) => set("scheduler_github_app_client_id_secret", v)}
        onPrivateKeySecret={(v) => set("scheduler_github_app_private_key_secret", v)}
      />

      <h3 className="mt-5 text-[0.9rem] font-semibold">Bot identity</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Git author of the deploy commits pushed to the routines repository when schedules change. To attribute them to
        your GitHub App bot, use <code className="rounded bg-secondary px-1">{"<app-slug>[bot]"}</code> and{" "}
        <code className="rounded bg-secondary px-1">{"<user-id>+<app-slug>[bot]@users.noreply.github.com"}</code>.
      </p>
      <div className="mb-3 flex flex-col gap-3">
        <div className="flex flex-col gap-1">
          <Label htmlFor="scheduler-github-bot-name" className="text-[0.8rem] text-muted-foreground">
            Bot name
          </Label>
          <Input
            id="scheduler-github-bot-name"
            value={config.scheduler_github_bot_name}
            onChange={(e) => set("scheduler_github_bot_name", e.target.value)}
            placeholder="infer"
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="scheduler-github-bot-email" className="text-[0.8rem] text-muted-foreground">
            Bot email
          </Label>
          <Input
            id="scheduler-github-bot-email"
            value={config.scheduler_github_bot_email}
            onChange={(e) => set("scheduler_github_bot_email", e.target.value)}
            placeholder="infer@users.noreply.github.com"
          />
        </div>
      </div>

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !error && <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>}
        {saved && !dirty && (
          <span role="status" className="text-[0.8rem] text-muted-foreground">
            Saved
          </span>
        )}
        {error && (
          <span role="status" className="text-[0.8rem] text-err">
            Couldn't save: {error}
          </span>
        )}
      </div>
    </>
  );
}

// Owner dropdown (user + orgs via gh) composed with a repo-name input into the
// single owner/repo config string; falls back to a plain text input until gh
// responds (or when it can't).
function RepositoryPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [owners, setOwners] = useState<string[]>([]);
  const [owner, setOwner] = useState("");
  const [name, setName] = useState("");
  const [repoExists, setRepoExists] = useState<boolean | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");

  useEffect(() => {
    api
      .githubOwners()
      .then(setOwners)
      .catch(() => {});
  }, []);

  useEffect(() => {
    setRepoExists(null);
    if (!value.includes("/")) return;
    const t = setTimeout(() => {
      api
        .githubRepoExists(value)
        .then(setRepoExists)
        .catch(() => {});
    }, 500);
    return () => clearTimeout(t);
  }, [value]);

  const createRepo = async () => {
    setCreating(true);
    try {
      await api.githubCreateRepo(value);
      setCreateError("");
      setRepoExists(true);
    } catch (e) {
      setCreateError(String(e));
    } finally {
      setCreating(false);
    }
  };

  const existsLine =
    repoExists === false ? (
      <div className="mt-1 flex items-center gap-2 text-[0.75rem]">
        <span className="h-2 w-2 shrink-0 rounded-full bg-amber-500" />
        <span className="text-muted-foreground">
          Repository <code className="rounded bg-secondary px-1">{value}</code> doesn't exist yet.
        </span>
        <Button variant="outline" size="xs" disabled={creating} onClick={createRepo}>
          {creating ? "Creating..." : "Create repository"}
        </Button>
        {createError && (
          <span role="status" className="text-err">
            {createError}
          </span>
        )}
      </div>
    ) : null;

  useEffect(() => {
    const slash = value.indexOf("/");
    if (slash >= 0) {
      setOwner(value.slice(0, slash));
      setName(value.slice(slash + 1));
    }
  }, [value]);

  const canonical = owners.find((o) => o.toLowerCase() === owner.toLowerCase());

  useEffect(() => {
    if (canonical && canonical !== owner) {
      setOwner(canonical);
      if (name) onChange(`${canonical}/${name}`);
    }
  }, [canonical, owner, name, onChange]);

  if (owners.length === 0) {
    return (
      <div className="flex flex-col gap-1">
        <Label htmlFor="scheduler_github_repository" className="text-[0.8rem] text-muted-foreground">
          Repository
        </Label>
        <Input
          id="scheduler_github_repository"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="<login>/.routines (auto-created when empty)"
        />
        {existsLine}
      </div>
    );
  }

  const ownerOptions = owner && !canonical ? [owner, ...owners] : owners;
  const update = (o: string, n: string) => {
    setOwner(o);
    setName(n);
    onChange(n ? `${o}/${n}` : "");
  };

  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor="scheduler-github-repo-name" className="text-[0.8rem] text-muted-foreground">
        Repository
      </Label>
      <div className="flex items-center gap-2">
        <select
          id="scheduler-github-owner"
          aria-label="Repository owner"
          value={owner || owners[0]}
          onChange={(e) => update(e.target.value, name)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {ownerOptions.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
        <span className="text-muted-foreground">/</span>
        <Input
          id="scheduler-github-repo-name"
          aria-label="Repository name"
          value={name}
          onChange={(e) => update(owner || owners[0], e.target.value)}
          placeholder=".routines (auto-created when empty)"
        />
      </div>
      {existsLine}
    </div>
  );
}

function GithubPrereqs({
  repository,
  clientIdSecret,
  privateKeySecret,
  onClientIdSecret,
  onPrivateKeySecret,
}: {
  repository: string;
  clientIdSecret: string;
  privateKeySecret: string;
  onClientIdSecret: (v: string) => void;
  onPrivateKeySecret: (v: string) => void;
}) {
  const [gh, setGh] = useState<{ installed: boolean; authenticated: boolean } | null>(null);
  const [existing, setExisting] = useState<string[]>([]);
  const [secretError, setSecretError] = useState("");
  const [providerSecret, setProviderSecret] = useState("OPENAI_API_KEY");

  useEffect(() => {
    api
      .githubAuthStatus()
      .then(setGh)
      .catch(() => {});
  }, []);

  const hasRepo = repository.includes("/");
  const refreshSecrets = useCallback(() => {
    if (!repository.includes("/")) return;
    api
      .githubListSecrets(repository)
      .then(setExisting)
      .catch(() => setExisting([]));
  }, [repository]);

  useEffect(() => {
    if (gh?.authenticated) refreshSecrets();
  }, [gh, refreshSecrets]);

  const repoUrl = hasRepo ? `https://github.com/${repository}` : "";

  return (
    <div className="mb-3 rounded-md border border-border bg-card px-3 py-2">
      <div className="mb-1 text-[0.8rem] font-medium">Prerequisites</div>
      <div className="mb-2 flex items-center gap-2 text-[0.75rem]">
        {gh?.authenticated ? (
          <>
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
            <span className="text-emerald-500">GitHub CLI authenticated</span>
          </>
        ) : (
          <>
            <span className="h-2 w-2 rounded-full bg-amber-500" />
            <span className="text-muted-foreground">
              {gh?.installed
                ? "GitHub CLI not authenticated - run "
                : "GitHub CLI (gh) not found - install it, then run "}
              <code className="rounded bg-secondary px-1">gh auth login</code>
            </span>
          </>
        )}
      </div>
      <p className="mb-2 text-[0.75rem] text-muted-foreground">
        Workflows run as a GitHub App bot and need these Actions secrets in the routines repository (or as org secrets
        under the same names). Values are sent straight to GitHub and never stored by the app. The App credential secret
        names are written into the generated workflows - rename them to match your convention, then Save config.
      </p>
      {gh?.authenticated && hasRepo ? (
        <div className="mb-2">
          <SecretRow
            repo={repository}
            name={clientIdSecret}
            nameLabel="App client ID"
            onNameChange={onClientIdSecret}
            multiline={false}
            existing={existing}
            onDone={() => {
              setSecretError("");
              refreshSecrets();
            }}
            onError={setSecretError}
          />
          <SecretRow
            repo={repository}
            name={privateKeySecret}
            nameLabel="App private key"
            onNameChange={onPrivateKeySecret}
            multiline
            existing={existing}
            onDone={() => {
              setSecretError("");
              refreshSecrets();
            }}
            onError={setSecretError}
          />
          <SecretRow
            repo={repository}
            name={providerSecret}
            nameLabel="Provider API key"
            onNameChange={setProviderSecret}
            multiline={false}
            existing={existing}
            onDone={() => {
              setSecretError("");
              refreshSecrets();
            }}
            onError={setSecretError}
          />
        </div>
      ) : (
        <p className="mb-2 text-[0.75rem] text-muted-foreground">
          {gh?.authenticated
            ? "Configure a repository in the Scheduling tab to manage its secrets here."
            : "Authenticate the GitHub CLI to manage secrets from here."}
        </p>
      )}
      {secretError && (
        <div role="status" className="mb-2 text-[0.75rem] text-err">
          {secretError}
        </div>
      )}
      {repoUrl && (
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => api.openUrl(`${repoUrl}/settings/secrets/actions`).catch(() => {})}
          >
            Repository secrets
          </Button>
          <Button variant="outline" size="sm" onClick={() => api.openUrl(`${repoUrl}/actions`).catch(() => {})}>
            Actions runs
          </Button>
        </div>
      )}
    </div>
  );
}

// Secret values are pushed straight to GitHub via `gh secret set` and never
// stored by the app. The App credential secret NAMES are config (they end up
// in the generated workflow), so renaming them here persists via Save config.
function SecretRow({
  repo,
  name,
  nameLabel,
  onNameChange,
  multiline,
  existing,
  onDone,
  onError,
}: {
  repo: string;
  name: string;
  nameLabel: string;
  onNameChange: (v: string) => void;
  multiline: boolean;
  existing: string[];
  onDone: () => void;
  onError: (msg: string) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const isSet = existing.includes(name.trim());

  const submit = async () => {
    setBusy(true);
    try {
      await api.githubSetSecret(repo, name.trim(), value);
      setValue("");
      onDone();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const inputClass =
    "flex-1 rounded border border-input bg-transparent px-2 py-1 text-[0.75rem] text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-ring";

  return (
    <div className="mb-1.5 flex items-center gap-2">
      <span className={cn("h-2 w-2 shrink-0 rounded-full", isSet ? "bg-emerald-500" : "bg-amber-500")} />
      <Input
        aria-label={`${nameLabel} secret name`}
        value={name}
        onChange={(e) => onNameChange(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, ""))}
        className="h-7 w-64 font-mono text-[0.7rem]"
      />
      {multiline ? (
        <textarea
          aria-label={`Value for ${nameLabel}`}
          rows={1}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={isSet ? "set - paste to replace" : "paste PEM key"}
          className={cn(inputClass, "resize-y font-mono")}
        />
      ) : (
        <input
          aria-label={`Value for ${nameLabel}`}
          type="password"
          autoComplete="off"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={isSet ? "set - type to replace" : "not set"}
          className={inputClass}
        />
      )}
      <Button
        variant="outline"
        size="xs"
        aria-label={`Set ${nameLabel}`}
        disabled={busy || !value || !name.trim()}
        onClick={submit}
      >
        {busy ? "Setting..." : "Set"}
      </Button>
    </div>
  );
}

function SchedulerLogView() {
  const [log, setLog] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [running, setRunning] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setRunning(await api.getSchedulerStatus());
      setLog(await api.getSchedulerLog());
    } catch {}
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, [refresh]);

  return (
    <div className="mb-3 rounded-md border border-border bg-card">
      <button
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-[0.8rem] font-medium text-muted-foreground hover:text-foreground"
      >
        <span className="flex-1">Scheduler daemon</span>
        {running ? (
          <span className="flex items-center gap-1 text-emerald-500">
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
            Running
          </span>
        ) : (
          <span className="text-muted-foreground">Stopped</span>
        )}
      </button>
      {open && (
        <div className="border-t border-border px-3 pb-3 pt-2">
          <div className="max-h-[200px] overflow-y-auto rounded bg-secondary p-2 font-mono text-[0.7rem] leading-relaxed text-muted-foreground">
            {log.length === 0 ? (
              <span className="italic">No log entries yet.</span>
            ) : (
              log.map((line, i) => <div key={i}>{line}</div>)
            )}
          </div>
        </div>
      )}
    </div>
  );
}
