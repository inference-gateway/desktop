import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, ArrowLeft, CheckCircle2, CircleMinus, Eye, EyeOff, GitBranch, Paperclip } from "lucide-react";
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
import {
  api,
  type A2aAgent,
  type ComputerUsePermissionStatus,
  type DesktopConfig,
  type GitRepo,
  type OsPermissionState,
  type ProjectFile,
  type RepoEntry,
} from "@/lib/tauri";
import { TasksPanel } from "./TasksView";
import { fetchAgentCatalog, type CatalogAgent } from "@/lib/registry";
import { PROVIDERS, useDesktop } from "@/store";
import { DEFAULT_SNIPPETS } from "@/lib/snippets";
import {
  DEFAULT_REGISTRY_URL,
  fetchSkillsCatalog,
  getRegistryUrl,
  setRegistryUrl,
  type SkillsCatalog,
} from "@/lib/skills";

type Tab =
  | "general"
  | "keys"
  | "prompt"
  | "updates"
  | "agents"
  | "snippets"
  | "projects"
  | "github"
  | "skills";

type GithubSubTab = "repository" | "scheduling" | "tasks";

const GITHUB_SUBTABS: { id: GithubSubTab; label: string }[] = [
  { id: "repository", label: "General" },
  { id: "scheduling", label: "Scheduling" },
  { id: "tasks", label: "Tasks" },
];

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "keys", label: "API Keys" },
  { id: "prompt", label: "System Prompt" },
  { id: "agents", label: "Agents" },
  { id: "projects", label: "Projects" },
  { id: "snippets", label: "Snippets" },
  { id: "github", label: "GitHub" },
  { id: "skills", label: "Skills" },
  { id: "updates", label: "Updates" },
];

export function SettingsView() {
  const {
    setCurrentView,
    saveSettings,
    getAuth,
    updates,
    checkForUpdates,
    applyUpdates,
    showUpdateBanner,
    statusText,
    statusError,
    initialSettingsTab,
    setInitialSettingsTab,
    setInitialProjectFilter,
  } = useDesktop();
  const [tab, setTab] = useState<Tab>(
    TABS.some((t) => t.id === initialSettingsTab) ? (initialSettingsTab as Tab) : "general"
  );
  const [githubSub, setGithubSub] = useState<GithubSubTab>("repository");

  useEffect(() => {
    setInitialSettingsTab("general");
    setInitialProjectFilter("");
  }, [setInitialSettingsTab, setInitialProjectFilter]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    getAuth()
      .then((auth) => {
        const next: Record<string, string> = {};
        for (const p of PROVIDERS) next[p.env] = (auth && auth[p.env]) || "";
        setValues(next);
      })
      .catch(() => {});
  }, [getAuth]);

  const onSave = () => {
    const keys: Record<string, string> = {};
    for (const p of PROVIDERS) keys[p.env] = (values[p.env] || "").trim();
    saveSettings(keys);
  };

  return (
    <div id="settings-view" className="flex min-h-0 flex-1">
      <nav className="flex w-[200px] shrink-0 flex-col gap-1 border-r border-border bg-secondary p-3">
        <button
          onClick={() => setCurrentView("chat")}
          className="mb-2 inline-flex items-center gap-1.5 rounded-md px-2 py-[0.45rem] text-[0.85rem] font-medium text-muted-foreground hover:bg-primary/10 hover:text-foreground"
        >
          <ArrowLeft size={15} /> Back
        </button>
        <span className="mb-1 px-2 text-[0.7rem] font-semibold uppercase tracking-wide text-muted-foreground">
          Settings
        </span>
        <div className="flex min-h-0 flex-1 flex-col gap-1">
          {TABS.map((t) => (
            <div key={t.id} className={cn("flex flex-col gap-1", t.id === "updates" && "mt-auto")}>
              <button
                aria-pressed={tab === t.id}
                onClick={() => setTab(t.id)}
                className={cn(
                  "relative rounded-md px-3 py-[0.5rem] text-left text-[0.85rem] font-medium",
                  tab === t.id
                    ? "bg-primary/15 text-primary before:absolute before:left-0 before:top-1/2 before:h-4 before:w-[3px] before:-translate-y-1/2 before:rounded-full before:bg-primary"
                    : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
                )}
              >
                {t.label}
              </button>
              {t.id === "github" && (
                <div className="ml-4 flex flex-col border-l border-border/60 pl-1">
                  {GITHUB_SUBTABS.map((s) => {
                    const active = tab === "github" && githubSub === s.id;
                    return (
                      <button
                        key={s.id}
                        aria-pressed={active}
                        onClick={() => {
                          setTab("github");
                          setGithubSub(s.id);
                        }}
                        className={cn(
                          "relative rounded-md px-3 py-[0.3rem] text-left text-[0.8rem]",
                          active
                            ? "font-medium text-primary before:absolute before:-left-[5px] before:top-1/2 before:h-4 before:w-[2px] before:-translate-y-1/2 before:rounded-full before:bg-primary"
                            : "text-muted-foreground hover:text-foreground",
                        )}
                      >
                        {s.label}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          ))}
        </div>
      </nav>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
        <div className="mx-auto w-full max-w-[640px]">
          {statusError && <div role="status" className="mb-3 text-[0.8rem] text-err">{statusText}</div>}
          {tab === "general" && <GeneralTab />}

          {tab === "keys" && (
            <>
              <h2 className="text-[1.05rem] font-semibold">API Keys</h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">
                Stored in ~/.infer/auth.json and passed to the agent as environment variables.
              </p>
              <div className="flex flex-col gap-[0.7rem]">
                {PROVIDERS.map((p) => (
                  <div key={p.env} className="flex flex-col gap-1">
                    <Label htmlFor={p.env} className="text-[0.8rem] text-muted-foreground">
                      {p.label}
                    </Label>
                    <div className="relative">
                      <Input
                        id={p.env}
                        type={shown[p.env] ? "text" : "password"}
                        autoComplete="off"
                        placeholder={p.env}
                        value={values[p.env] ?? ""}
                        onChange={(e) => setValues((v) => ({ ...v, [p.env]: e.target.value }))}
                        className="pr-9"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label={shown[p.env] ? "Hide API key" : "Show API key"}
                        onClick={() => setShown((s) => ({ ...s, [p.env]: !s[p.env] }))}
                        className="absolute right-0.5 top-1/2 -translate-y-1/2 text-muted-foreground"
                      >
                        {shown[p.env] ? <EyeOff /> : <Eye />}
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-6 flex items-center gap-2 border-t border-border pt-4">
                <Button onClick={onSave}>Save</Button>
              </div>
            </>
          )}

          {tab === "updates" && (
            <>
              <h2 className="text-[1.05rem] font-semibold">Updates</h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">Checked automatically every 6 hours.</p>
              <div>
                {updates.map((u) => {
                  const latest = u.latest ? (u.outdated ? `→ ${u.latest}` : "up to date") : "unknown";
                  return (
                    <div key={u.name} className="mb-[0.3rem] text-[0.8rem] text-muted-foreground">
                      {`${u.name} ${u.current} ${latest}`}
                    </div>
                  );
                })}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={checking}
                  onClick={async () => {
                    setChecking(true);
                    await checkForUpdates(true);
                    setChecking(false);
                  }}
                >
                  {checking ? "Checking..." : "Check for updates"}
                </Button>
                {showUpdateBanner && (
                  <Button
                    size="sm"
                    onClick={() => {
                      setCurrentView("chat");
                      applyUpdates();
                    }}
                  >
                    Install updates
                  </Button>
                )}
              </div>
            </>
          )}

          {tab === "agents" && <AgentsTab />}
          {tab === "github" && <GithubTab sub={githubSub} />}
          {tab === "projects" && <ProjectsTab />}
          {tab === "snippets" && <SnippetsTab />}
          {tab === "prompt" && <SystemPromptTab />}
          {tab === "skills" && <SkillsTab />}
        </div>
      </div>
    </div>
  );
}

type StorageField = {
  key: keyof DesktopConfig;
  label: string;
  ph?: string;
  secret?: boolean;
  options?: readonly string[];
};

const STORAGE_BACKENDS = ["jsonl", "sqlite", "postgres", "redis", "d1"] as const;

// Fields per backend, mapping 1:1 to infer's storage.<type>.* schema.
const STORAGE_FIELDS: Record<string, readonly StorageField[]> = {
  jsonl: [{ key: "storage_directory", label: "Conversations directory", ph: "~/.infer/conversations" }],
  sqlite: [{ key: "sqlite_path", label: "Database file", ph: ".infer/conversations.db" }],
  postgres: [
    { key: "postgres_host", label: "Host", ph: "localhost" },
    { key: "postgres_port", label: "Port", ph: "5432" },
    { key: "postgres_database", label: "Database", ph: "infer_conversations" },
    { key: "postgres_username", label: "Username" },
    { key: "postgres_password", label: "Password", secret: true },
    {
      key: "postgres_ssl_mode",
      label: "SSL mode",
      options: ["disable", "allow", "prefer", "require", "verify-ca", "verify-full"],
    },
  ],
  redis: [
    { key: "redis_host", label: "Host", ph: "localhost" },
    { key: "redis_port", label: "Port", ph: "6379" },
    { key: "redis_password", label: "Password", secret: true },
    { key: "redis_db", label: "Database number", ph: "0" },
  ],
  d1: [
    { key: "d1_account_id", label: "Account ID" },
    { key: "d1_database_id", label: "Database ID" },
    { key: "d1_api_token", label: "API token", secret: true },
    { key: "d1_base_url", label: "Base URL", ph: "https://api.cloudflare.com/client/v4" },
  ],
};

const DEFAULT_CONFIG: DesktopConfig = {
  storage_backend: "jsonl",
  storage_directory: "",
  gateway_url: "http://localhost:8080",
  default_model: "",
  sqlite_path: ".infer/conversations.db",
  postgres_host: "localhost",
  postgres_port: "5432",
  postgres_database: "infer_conversations",
  postgres_username: "",
  postgres_password: "",
  postgres_ssl_mode: "prefer",
  redis_host: "localhost",
  redis_port: "6379",
  redis_password: "",
  redis_db: "0",
  d1_account_id: "",
  d1_database_id: "",
  d1_api_token: "",
  d1_base_url: "https://api.cloudflare.com/client/v4",
  extra_instructions: "",
  system_prompt: "",
  schedule_enabled: false,
  agent_model: "",
  scheduler_backend: "local",
  scheduler_github_repository: "",
  scheduler_github_app_client_id_secret: "APP_CLIENT_ID",
  scheduler_github_app_private_key_secret: "APP_PRIVATE_KEY",
  scheduler_github_bot_name: "infer",
  scheduler_github_bot_email: "infer@users.noreply.github.com",
  scheduler_github_pull_requests: false,
  scheduler_github_artifacts_enabled: true,
  scheduler_github_artifacts_poll_interval: "10m",
  scheduler_github_artifacts_initial_delay: "1m",
  scheduler_github_artifacts_max_attempts: "3",
  scheduler_github_artifacts_rate_limit_backoff: "1h",
  projects_root: "",
  projects_backend: "local",
  projects_github_repository: ".projects",
  projects_max_file_size_mb: "10",
  projects_allowed_mimes: "pdf,png,jpg,jpeg,gif,webp,mp4,mov,txt,md,csv",
};

// Text inputs for the github scheduling backend; the repository picker and
// checkboxes are hand-rendered.
const SCHEDULER_GITHUB_FIELDS: readonly StorageField[] = [
  { key: "scheduler_github_artifacts_poll_interval", label: "Artifact poll interval", ph: "10m" },
  { key: "scheduler_github_artifacts_initial_delay", label: "Artifact poll initial delay", ph: "1m" },
  { key: "scheduler_github_artifacts_max_attempts", label: "Max download attempts per artifact", ph: "3" },
  { key: "scheduler_github_artifacts_rate_limit_backoff", label: "Rate-limit backoff", ph: "1h" },
];

function GeneralTab() {
  const { maxSessions, setMaxSessions, models, model, setModel } = useDesktop();
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [computerUsePermissions, setComputerUsePermissions] =
    useState<ComputerUsePermissionStatus | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api.getConfig().then(setConfigs).catch(() => {});
  }, []);

  const refreshComputerUsePermissions = useCallback(() => {
    api.computerUsePermissionStatus().then(setComputerUsePermissions).catch(() => {});
  }, []);

  useEffect(() => {
    refreshComputerUsePermissions();
    window.addEventListener("focus", refreshComputerUsePermissions);
    return () => window.removeEventListener("focus", refreshComputerUsePermissions);
  }, [refreshComputerUsePermissions]);

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

  const storageFields = STORAGE_FIELDS[config.storage_backend];

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">General</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Agent sessions, storage backend, default model, and gateway settings. Changes take effect on next agent spawn.
      </p>

      <h3 className="text-[0.9rem] font-semibold">Computer Use</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Lets the agent see the screen and control other apps. Off by default; takes effect on
        the next agent spawn.
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="computer-use-enabled"
          checked={!!computerUsePermissions?.computer_use_enabled}
          onChange={(e) => {
            api
              .setComputerUseEnabled(e.target.checked)
              .catch(() => {})
              .then(refreshComputerUsePermissions);
          }}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="computer-use-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Computer Use
        </Label>
      </div>

      {computerUsePermissions?.computer_use_enabled && (
        <div className="mb-5 flex flex-col gap-3">
          <ComputerUsePermissionSection
            title="Accessibility"
            state={computerUsePermissions.accessibility}
            warning="Accessibility access is not granted. Computer Use cannot inspect or control other apps. Click Grant, then enable Inference Gateway Desktop in the prompt."
            onGrant={() => {
              api
                .requestAccessibilityPermission()
                .catch(() => {})
                .then(refreshComputerUsePermissions);
            }}
          />
          <ComputerUsePermissionSection
            title="Screen Recording"
            state={computerUsePermissions.screen_recording}
            warning="Screen Recording access is not granted. Computer Use cannot capture the screen. Click Grant to request access - macOS applies Screen Recording after the app restarts."
            onGrant={() => {
              api
                .requestScreenRecordingPermission()
                .catch(() => {})
                .then(refreshComputerUsePermissions);
            }}
          />
        </div>
      )}

      {/* Agent sessions */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Sessions</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Run multiple agent sessions at once. Each session is a separate infer agent process.
      </p>
      <div className="mb-5 flex flex-col gap-1">
        <Label htmlFor="max-sessions" className="text-[0.8rem] text-muted-foreground">
          Max concurrent sessions
        </Label>
        <Input
          id="max-sessions"
          type="number"
          min={1}
          value={maxSessions}
          onChange={(e) => setMaxSessions(parseInt(e.target.value, 10))}
          className="w-24"
        />
      </div>

      {/* Storage */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Storage</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Where conversations are stored. Pick a backend and fill in its connection details.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="storage-backend" className="text-[0.8rem] text-muted-foreground">
          Backend
        </Label>
        <select
          id="storage-backend"
          value={config.storage_backend}
          onChange={(e) => set("storage_backend", e.target.value)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {STORAGE_BACKENDS.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
      </div>
      {storageFields ? (
        <div className="mb-3 flex flex-col gap-3">
          {storageFields.map((f) => (
            <div key={f.key} className="flex flex-col gap-1">
              <Label htmlFor={f.key} className="text-[0.8rem] text-muted-foreground">
                {f.label}
              </Label>
              {f.options ? (
                <select
                  id={f.key}
                  value={config[f.key] as string}
                  onChange={(e) => set(f.key, e.target.value)}
                  className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
                >
                  {f.options.map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
              ) : (
                <Input
                  id={f.key}
                  type={f.secret ? "password" : undefined}
                  value={config[f.key] as string}
                  onChange={(e) => set(f.key, e.target.value)}
                  placeholder={f.ph}
                />
              )}
            </div>
          ))}
        </div>
      ) : (
        <p className="mb-3 text-[0.75rem] text-muted-foreground">
          The {config.storage_backend} backend is not editable here yet.
        </p>
      )}

      {/* Default model */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Default Model</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Model selected by default for new chats. Persisted server-side so it survives a cache clear.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="default-model" className="text-[0.8rem] text-muted-foreground">
          Model
        </Label>
        <select
          id="default-model"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          className="w-full max-w-xs rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {(models.includes(model) ? models : [model, ...models].filter(Boolean)).map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </div>

      {/* Gateway */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Gateway</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        URL of the inference gateway. Changes take effect on the next gateway restart.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="gateway-url" className="text-[0.8rem] text-muted-foreground">
          Gateway URL
        </Label>
        <Input
          id="gateway-url"
          value={config.gateway_url}
          onChange={(e) => set("gateway_url", e.target.value)}
          placeholder="http://localhost:8080"
        />
      </div>

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !error && (
          <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>
        )}
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

function ComputerUsePermissionSection({
  title,
  state,
  warning,
  onGrant,
}: {
  title: string;
  state: OsPermissionState;
  warning: string;
  onGrant?: () => void;
}) {
  const isWarning = state === "not_granted" || state === "unavailable";
  const status = {
    granted: "Granted",
    not_granted: "Not granted",
    unavailable: "Unavailable in development",
    not_applicable: "Not required on this OS",
  }[state];
  const message =
    state === "unavailable"
      ? `${title} status cannot be verified from Desktop dev because macOS grants access to app bundle identities. Build and launch Inference Gateway Desktop.app to check this permission.`
      : state === "not_granted"
        ? warning
        : null;

  return (
    <section className="rounded-lg border border-border bg-secondary/40 p-3">
      <div className="flex items-center gap-2">
        {isWarning ? (
          <AlertTriangle className="text-amber-500" size={17} aria-hidden="true" />
        ) : state === "granted" ? (
          <CheckCircle2 className="text-emerald-500" size={17} aria-hidden="true" />
        ) : (
          <CircleMinus className="text-muted-foreground" size={17} aria-hidden="true" />
        )}
        <h3 className="text-[0.9rem] font-semibold">{title}</h3>
        <span className="ml-auto text-[0.75rem] text-muted-foreground">{status}</span>
      </div>
      {message && (
        <p role="alert" className="mt-2 text-[0.75rem] text-amber-600 dark:text-amber-400">
          {message}
        </p>
      )}
      {state === "not_granted" && onGrant && (
        <Button
          size="sm"
          variant="outline"
          className="mt-2"
          aria-label={`Grant ${title}`}
          onClick={onGrant}
        >
          Grant
        </Button>
      )}
    </section>
  );
}

function SchedulingTab() {
  const { models } = useDesktop();
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api.getConfig().then(setConfigs).catch(() => {});
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
        Run <code className="rounded bg-secondary px-1">infer daemon</code> locally to fire
        scheduled agent jobs (and listen on configured channels like Telegram). Job runs are
        recorded to your configured storage. The daemon starts on Save, runs while the app is
        open, and stops on quit.
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
          Routines repository for the github backend: schedules are deployed there as GitHub
          Actions workflows by the daemon (auto-created when missing).
        </p>
        <RepositoryPicker
          value={config.scheduler_github_repository}
          onChange={(v) => set("scheduler_github_repository", v)}
        />
      </div>
      {config.scheduler_backend === "github" && (
        <>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            Jobs are deployed as GitHub Actions workflows in your routines repository and run
            there via infer-action. Schedules fire in UTC with a minimum interval of 5 minutes.
            Deploy outcomes (push or pull request URL) and cron validation errors appear in the
            chat when you schedule a job.
          </p>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            GitHub CLI status and the Actions secrets the workflows need are managed in the
            General tab.
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
            <Label
              htmlFor="scheduler-github-pull-requests"
              className="cursor-pointer text-[0.8rem] font-medium"
            >
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
            <Label
              htmlFor="scheduler-github-artifacts-enabled"
              className="cursor-pointer text-[0.8rem] font-medium"
            >
              Pull run conversations back into local storage
            </Label>
          </div>
        </>
      )}
      <div className="mb-3 flex items-center justify-between gap-3">
        <Label htmlFor="schedule-model" className="text-[0.8rem] font-medium">
          Default model for scheduled jobs
        </Label>
        <select
          id="schedule-model"
          value={config.agent_model}
          onChange={(e) => set("agent_model", e.target.value)}
          className="w-64 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          <option value="">Not set - each job must specify one</option>
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </div>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Used when a scheduled job has no model of its own (writes{" "}
        <code className="rounded bg-secondary px-1">agent.model</code>). A model chosen when
        scheduling a job always takes precedence.
      </p>
      <SchedulerLogView />

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !error && (
          <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>
        )}
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

function GithubTab({ sub }: { sub: GithubSubTab }) {
  return (
    <>
      <h2 className="mb-4 text-[1.05rem] font-semibold">
        GitHub · {GITHUB_SUBTABS.find((t) => t.id === sub)?.label}
      </h2>
      {sub === "repository" && <GithubRepositoryPanel />}
      {sub === "scheduling" && <SchedulingTab />}
      {sub === "tasks" && <TasksPanel />}
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
    api.getConfig().then(setConfigs).catch(() => {});
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
        GitHub CLI status, the Actions secrets the workflows need, and the bot identity. The
        scheduling repository lives in the Scheduling tab; task repositories in the Tasks tab.
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
        Git author of the deploy commits pushed to the routines repository when schedules
        change. To attribute them to your GitHub App bot, use{" "}
        <code className="rounded bg-secondary px-1">{"<app-slug>[bot]"}</code> and{" "}
        <code className="rounded bg-secondary px-1">
          {"<user-id>+<app-slug>[bot]@users.noreply.github.com"}
        </code>
        .
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
        {dirty && !error && (
          <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>
        )}
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
    api.githubOwners().then(setOwners).catch(() => {});
  }, []);

  useEffect(() => {
    setRepoExists(null);
    if (!value.includes("/")) return;
    const t = setTimeout(() => {
      api.githubRepoExists(value).then(setRepoExists).catch(() => {});
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
          Repository <code className="rounded bg-secondary px-1">{value}</code> doesn't exist
          yet.
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

function AgentsTab() {
  const { models } = useDesktop();
  const [agents, setAgents] = useState<A2aAgent[]>([]);
  const [catalog, setCatalog] = useState<CatalogAgent[]>([]);
  const [catalogError, setCatalogError] = useState(false);
  const [loadingCatalog, setLoadingCatalog] = useState(true);
  const [newAgentUrl, setNewAgentUrl] = useState("");

  const loadAgents = async () => {
    try {
      setAgents(await api.listA2aAgents());
    } catch (e) {
      console.error("Failed to load A2A agents:", e);
    }
  };

  useEffect(() => {
    loadAgents();
    fetchAgentCatalog()
      .then(setCatalog)
      .catch((e) => {
        console.error("Failed to load agent catalog:", e);
        setCatalogError(true);
      })
      .finally(() => setLoadingCatalog(false));
  }, []);

  const configured = useMemo(() => new Map(agents.map((a) => [a.name, a])), [agents]);
  const catalogNames = useMemo(() => new Set(catalog.map((c) => c.name)), [catalog]);
  const remoteAgents = useMemo(() => agents.filter((a) => !catalogNames.has(a.name)), [agents, catalogNames]);

  const toggleLocal = async (c: CatalogAgent) => {
    try {
      if (configured.has(c.name)) await api.removeA2aAgent(c.name);
      else await api.addA2aAgent(c.name, "");
      await loadAgents();
    } catch (e) {
      console.error("Failed to toggle A2A agent:", e);
    }
  };

  const setModel = async (name: string, model: string) => {
    try {
      await api.setA2aAgentModel(name, model);
      await loadAgents();
    } catch (e) {
      console.error("Failed to set agent model:", e);
    }
  };

  const addRemote = async () => {
    const url = newAgentUrl.trim();
    if (!url) return;
    try {
      await api.addA2aAgent(url, url);
      setNewAgentUrl("");
      await loadAgents();
    } catch (e) {
      console.error("Failed to add A2A agent:", e);
    }
  };

  const removeRemote = async (name: string) => {
    try {
      await api.removeA2aAgent(name);
      await loadAgents();
    } catch (e) {
      console.error("Failed to remove A2A agent:", e);
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Agents</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        A2A agents your local infer agent can delegate to. Toggle the local agents you want, or add a remote agent by
        URL. Selections are persisted and loaded on startup.
      </p>

      <h3 className="text-[0.9rem] font-semibold">Local A2A Agents (containers)</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Toggle to enable. Run each agent's container locally so infer can reach it.
      </p>
      {loadingCatalog ? (
        <p className="text-[0.8rem] text-muted-foreground">Loading registry...</p>
      ) : catalogError ? (
        <p className="text-[0.8rem] text-muted-foreground">Couldn't load the agent registry.</p>
      ) : (
        <div className="grid gap-2 grid-cols-[repeat(auto-fill,minmax(240px,1fr))]">
          {catalog.map((c) => {
            const agent = configured.get(c.name);
            const on = !!agent;
            return (
              <div
                key={c.name}
                className={cn(
                  "flex flex-col gap-1 rounded-md border bg-card p-3",
                  on ? "border-primary" : "border-border",
                )}
              >
                <label className="flex cursor-pointer flex-col gap-1">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={() => toggleLocal(c)}
                      className="h-4 w-4 shrink-0 accent-primary"
                    />
                    <span className="flex-1 truncate text-[0.85rem] font-medium">{c.name}</span>
                    {c.version && (
                      <span className="shrink-0 text-[0.7rem] font-normal text-muted-foreground">v{c.version}</span>
                    )}
                  </div>
                  {c.description && (
                    <p className="line-clamp-2 text-[0.75rem] text-muted-foreground">{c.description}</p>
                  )}
                  {c.skills.length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {c.skills.slice(0, 4).map((s) => (
                        <span key={s} className="rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground">
                          {s}
                        </span>
                      ))}
                    </div>
                  )}
                </label>
                {agent?.model && (
                  <select
                    aria-label={`Model for ${c.name}`}
                    value={agent.model}
                    onChange={(e) => setModel(c.name, e.target.value)}
                    className="mt-1 rounded border border-border bg-secondary px-1.5 py-1 text-[0.7rem] text-foreground"
                  >
                    {(models.includes(agent.model) ? models : [agent.model, ...models]).map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                )}
              </div>
            );
          })}
        </div>
      )}

      <h3 className="mt-6 text-[0.9rem] font-semibold">Remote agents</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">Agents reachable at a URL you host or run elsewhere.</p>
      {remoteAgents.length > 0 && (
        <ul className="mb-3 flex flex-col gap-2">
          {remoteAgents.map((a) => (
            <li
              key={a.name}
              className="flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-[0.85rem]"
            >
              <span className="flex-1 truncate">{a.url}</span>
              <Button variant="outline" size="xs" onClick={() => removeRemote(a.name)}>
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <Input
          placeholder="http://localhost:8085"
          value={newAgentUrl}
          onChange={(e) => setNewAgentUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addRemote();
          }}
        />
        <Button size="sm" onClick={addRemote} disabled={!newAgentUrl.trim()}>
          Add
        </Button>
      </div>
    </>
  );
}

// Files stored in a project's directory (local) or repository (github),
// rendered as attachment-style chips under the project's context.
function ProjectFiles({ project }: { project: string }) {
  const [files, setFiles] = useState<ProjectFile[] | null>(null);
  useEffect(() => {
    api.listProjectFiles(project).then(setFiles).catch(() => setFiles([]));
  }, [project]);
  if (!files?.length) return null;
  const fmt = (n: number) =>
    n < 1024
      ? `${n} B`
      : n < 1024 * 1024
        ? `${Math.round(n / 1024)} KB`
        : `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return (
    <div className="flex flex-wrap gap-1.5">
      {files.map((f) => (
        <span
          key={f.name}
          className="inline-flex h-7 max-w-full items-center gap-1.5 rounded-md border border-border bg-secondary px-2 text-[0.72rem] text-muted-foreground"
        >
          <Paperclip size={11} className="shrink-0" />
          <span className="min-w-0 truncate">{f.name}</span>
          <span className="shrink-0 tabular-nums">{fmt(f.size)}</span>
        </span>
      ))}
    </div>
  );
}

function ProjectsTab() {
  const { projectNames, projectContexts, setProjectContext, projectPaths, setProjectPath, importProjects, gitProjects, dirtyProjects, deleteProjects, projectGroups, initialProjectFilter } = useDesktop();
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
    api.getConfig().then(setConfigs).catch(() => {});
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
        Every project gets its own directory under the root below, pre-authorized in the agent sandbox. Applies to newly created projects.
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
            placeholder="pdf,png,jpg,jpeg,gif,webp,mp4,mov,txt,md,csv"
            onChange={(e) => set("projects_allowed_mimes", e.target.value)}
          />
        </div>
        {error && <div role="status" className="text-[0.75rem] text-err">{error}</div>}
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
                      done ? "text-muted-foreground/60" : "cursor-pointer hover:bg-secondary"
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
                            done ? "text-muted-foreground/60" : "cursor-pointer hover:bg-secondary"
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
              {error && <div role="status" className="text-[0.75rem] text-err">{error}</div>}
            </>
          )}
          <DialogFooter showCloseButton>
            {!ghHint && (
              <Button
                size="sm"
                disabled={ghLoading || ghSelected.size === 0 || cloning !== ""}
                onClick={importGithub}
              >
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
          {visible.length === 0 && (
            <p className="text-[0.8rem] text-muted-foreground">No projects match "{filter}".</p>
          )}
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
                </div>
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

function SnippetsTab() {
  const { snippets, updateSnippet, resetSnippet, resetAllSnippets } = useDesktop();
  const hasModifications = snippets.some((s) => {
    const def = DEFAULT_SNIPPETS.find((d) => d.id === s.id);
    return def && def.prompt !== s.prompt;
  });

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Snippets</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Quick-prompt templates shown below the composer. Changes save automatically.
      </p>
      <div className="flex flex-col gap-4">
        {snippets.map((s) => (
          <div
            key={s.id}
            className="rounded-lg border border-border bg-card p-4"
          >
            <div className="mb-3 flex items-center justify-between">
              <Label htmlFor={`snippet-label-${s.id}`} className="text-[0.8rem] font-medium">
                {s.id}
              </Label>
              <Button
                variant="outline"
                size="xs"
                onClick={() => resetSnippet(s.id)}
              >
                Reset
              </Button>
            </div>
            <div className="mb-2 flex flex-col gap-1">
              <Label
                htmlFor={`snippet-label-${s.id}`}
                className="text-[0.75rem] text-muted-foreground"
              >
                Label
              </Label>
              <Input
                id={`snippet-label-${s.id}`}
                value={s.label}
                onChange={(e) => updateSnippet(s.id, { label: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1">
              <Label
                htmlFor={`snippet-prompt-${s.id}`}
                className="text-[0.75rem] text-muted-foreground"
              >
                Prompt
              </Label>
              <textarea
                id={`snippet-prompt-${s.id}`}
                rows={3}
                value={s.prompt}
                onChange={(e) => updateSnippet(s.id, { prompt: e.target.value })}
                className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              />
            </div>
          </div>
        ))}
      </div>
      {hasModifications && (
        <div className="mt-4">
          <Button variant="outline" size="sm" onClick={resetAllSnippets}>
            Reset all to defaults
          </Button>
        </div>
      )}
    </>
  );
}

function SystemPromptTab() {
  const [config, setConfig] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [overrideEnabled, setOverrideEnabled] = useState(false);
  const [showOverrideWarning, setShowOverrideWarning] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => {
    api.getConfig().then((cfg) => {
      setConfig(cfg);
    }).catch(() => {});
  }, []);

  const update = (k: keyof DesktopConfig, v: string) => {
    setConfig((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setErr("");
  };

  const toggleOverride = () => {
    if (overrideEnabled) {
      setOverrideEnabled(false);
      update("system_prompt", "");
    } else {
      setShowOverrideWarning(true);
    }
  };

  const apply = async () => {
    setSaving(true);
    try {
      await api.setConfig(config);
      setDirty(false);
      setSaved(true);
      setErr("");
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">System Prompt</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Customize the default system prompt sent to the agent. Extra instructions are appended to the default; override
        replaces it entirely. When both are set, the override replaces the default and extra instructions are appended.
      </p>

      <div className="mb-5 flex flex-col gap-2">
        <Label htmlFor="extra-instructions" className="text-[0.8rem] font-medium">
          Extra instructions (append mode, always active)
        </Label>
        <p className="mb-1 text-[0.75rem] text-muted-foreground">
          Appended to the default system prompt on every agent session. Safe for project conventions, tool usage
          preferences, or security constraints.
        </p>
        <textarea
          id="extra-instructions"
          rows={4}
          value={config.extra_instructions}
          onChange={(e) => update("extra_instructions", e.target.value)}
          placeholder="e.g. Always cite sources when providing information."
          className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
        />
      </div>

      <div className="mb-5 flex items-start gap-3 rounded-md border border-border bg-card p-4">
        <input
          type="checkbox"
          id="override-toggle"
          checked={overrideEnabled}
          onChange={toggleOverride}
          className="mt-1 h-4 w-4 shrink-0 accent-primary"
        />
        <div className="flex flex-col gap-1">
          <Label htmlFor="override-toggle" className="cursor-pointer text-[0.8rem] font-medium">
            Override system prompt
          </Label>
          <p className="text-[0.75rem] text-muted-foreground">
            Replaces the entire default system prompt, discarding built-in instructions the agent relies on, such as how
            to use its tools and workspace context like the current directory, file tree, and git branch. Prefer extra
            instructions above. When enabled, extra instructions are still appended after the override.
          </p>
        </div>
      </div>

      {overrideEnabled && (
        <div className="mb-5 flex flex-col gap-2">
          <Label htmlFor="override-prompt" className="text-[0.8rem] font-medium">
            Override prompt
          </Label>
          <p className="mb-1 text-[0.75rem] text-muted-foreground">
            Replaces the default system prompt. Extra instructions from above are still appended after this text.
          </p>
          <textarea
            id="override-prompt"
            rows={8}
            value={config.system_prompt}
            onChange={(e) => update("system_prompt", e.target.value)}
            placeholder="Write your custom system prompt here..."
            className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          />
        </div>
      )}

      <Dialog open={showOverrideWarning} onOpenChange={setShowOverrideWarning}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="text-amber-500" size={18} />
              Override system prompt?
            </DialogTitle>
            <DialogDescription>
              You are overriding the default system prompt, which provides useful context for the orchestrator about the current
              project, installed plugins, skills, and memory. Prefer using "Extra instructions" (append mode) instead to
              keep this context.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowOverrideWarning(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => { setOverrideEnabled(true); setShowOverrideWarning(false); }}>
              Enable override
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !err && <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>}
        {saved && !dirty && (
          <span role="status" className="text-[0.8rem] text-muted-foreground">Saved</span>
        )}
        {err && <span role="status" className="text-[0.8rem] text-err">Couldn't save: {err}</span>}
      </div>
    </>
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
    api.githubAuthStatus().then(setGh).catch(() => {});
  }, []);

  const hasRepo = repository.includes("/");
  const refreshSecrets = useCallback(() => {
    if (!repository.includes("/")) return;
    api.githubListSecrets(repository).then(setExisting).catch(() => setExisting([]));
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
        Workflows run as a GitHub App bot and need these Actions secrets in the routines
        repository (or as org secrets under the same names). Values are sent straight to
        GitHub and never stored by the app. The App credential secret names are written
        into the generated workflows - rename them to match your convention, then Save
        config.
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
          <Button
            variant="outline"
            size="sm"
            onClick={() => api.openUrl(`${repoUrl}/actions`).catch(() => {})}
          >
            Actions runs
          </Button>
        </div>
      )}
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

function SkillsTab() {
  const [catalog, setCatalog] = useState<SkillsCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState("");
  const [search, setSearch] = useState("");
  const [registryUrl, setRegistryUrlState] = useState(getRegistryUrl());
  const [urlDirty, setUrlDirty] = useState(false);
  const [installed, setInstalled] = useState<Set<string>>(new Set());
  const [installingName, setInstallingName] = useState("");
  const [installErr, setInstallErr] = useState<{ name: string; msg: string } | null>(null);

  useEffect(() => {
fetchSkillsCatalog()
  .then(setCatalog)
  .catch((e) => setErr(String(e)))
  .finally(() => setLoading(false));
api.listInstalledSkills().then((s) => setInstalled(new Set(s))).catch(() => {});
  }, []);

  const runSkillAction = async (name: string, action: (name: string) => Promise<void>) => {
setInstallingName(name);
setInstallErr(null);
try {
  await action(name);
  setInstalled(new Set(await api.listInstalledSkills()));
} catch (e) {
  setInstallErr({ name, msg: String(e) });
} finally {
  setInstallingName("");
}
  };

  const filtered = useMemo(() => {
if (!catalog) return [];
const q = search.toLowerCase();
if (!q) return catalog.skills;
return catalog.skills.filter(
  (s) => s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q),
);
  }, [catalog, search]);

  return (
<>
  <h2 className="text-[1.05rem] font-semibold">Skills</h2>
  <p className="mb-5 text-[0.8rem] text-muted-foreground">
    Remote skills registry. Type / in the composer to discover and invoke skills.
    Remote skills are downloaded on first use with your approval.
  </p>

  <h3 className="text-[0.9rem] font-semibold">Registry URL</h3>
  <div className="mb-4 flex items-center gap-2">
    <Input
      value={registryUrl}
      onChange={(e) => {
        setRegistryUrlState(e.target.value);
        setUrlDirty(true);
      }}
      placeholder={DEFAULT_REGISTRY_URL}
      className="flex-1"
    />
    {urlDirty && (
      <Button
        size="sm"
        onClick={() => {
          setRegistryUrl(registryUrl);
          setUrlDirty(false);
          setLoading(true);
          setErr("");
          fetchSkillsCatalog(registryUrl)
            .then(setCatalog)
            .catch((e) => setErr(String(e)))
            .finally(() => setLoading(false));
        }}
      >
        Apply
      </Button>
    )}
  </div>

  <h3 className="text-[0.9rem] font-semibold">Catalog</h3>
  {loading ? (
    <p className="text-[0.8rem] text-muted-foreground">Loading catalog...</p>
  ) : err ? (
    <p className="text-[0.8rem] text-err">Couldn't load catalog: {err}</p>
  ) : catalog ? (
    <>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Version <code className="rounded bg-secondary px-1">{catalog.version}</code> — {catalog.skills.length}{" "}
        skills available.
      </p>

      <Input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search skills by name or description..."
        className="mb-3"
      />

      <div className="flex flex-col gap-1">
        {filtered.map((s) => {
          const isInstalled = installed.has(s.name);
          return (
            <div
              key={s.name}
              className={cn(
                    "flex items-center gap-2 rounded-md border bg-card px-3 py-2 text-[0.82rem]",
                    isInstalled ? "border-primary" : "border-border",
              )}
            >
              <span className="min-w-0 flex-1">
                    <span className="font-medium">{s.name}</span>
                    {s.version && (
                      <span className="ml-1 text-[0.7rem] text-muted-foreground">v{s.version}</span>
                    )}
                    {s.description && (
                      <p className="line-clamp-1 text-[0.73rem] text-muted-foreground">{s.description}</p>
                    )}
                    {installErr?.name === s.name && (
                      <p className="text-[0.73rem] text-err">{installErr.msg}</p>
                    )}
              </span>
              {isInstalled ? (
                    <>
                      <span className="shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground">
                        installed
                      </span>
                      <Button
                        size="sm"
                        variant="outline"
                        className="shrink-0"
                        disabled={installingName !== ""}
                        onClick={() => runSkillAction(s.name, api.uninstallSkill)}
                      >
                        {installingName === s.name ? "Uninstalling..." : "Uninstall"}
                      </Button>
                    </>
              ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="shrink-0"
                      disabled={installingName !== ""}
                      onClick={() => runSkillAction(s.name, api.installSkill)}
                    >
                      {installingName === s.name ? "Installing..." : "Install"}
                    </Button>
              )}
            </div>
          );
        })}
        {filtered.length === 0 && (
          <p className="text-[0.8rem] text-muted-foreground">No skills match your search.</p>
        )}
      </div>
    </>
  ) : (
    <p className="text-[0.8rem] text-muted-foreground">No catalog loaded.</p>
  )}
</>
  );
}
