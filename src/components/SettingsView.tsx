import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, ArrowLeft } from "lucide-react";
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
import { api, type A2aAgent, type DesktopConfig } from "@/lib/tauri";
import { fetchAgentCatalog, type CatalogAgent } from "@/lib/registry";
import { PROVIDERS, useDesktop } from "@/store";
import { DEFAULT_SNIPPETS } from "@/lib/snippets";

type Tab = "general" | "keys" | "prompt" | "updates" | "agents" | "snippets";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "keys", label: "API Keys" },
  { id: "prompt", label: "System Prompt" },
  { id: "agents", label: "Agents" },
  { id: "snippets", label: "Snippets" },
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
  } = useDesktop();
  const [tab, setTab] = useState<Tab>("general");
  const [values, setValues] = useState<Record<string, string>>({});
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
            <button
              key={t.id}
              aria-pressed={tab === t.id}
              onClick={() => setTab(t.id)}
              className={cn(
                "rounded-md px-3 py-[0.5rem] text-left text-[0.85rem] font-medium",
                t.id === "updates" && "mt-auto",
                tab === t.id
                  ? "bg-primary/15 text-foreground"
                  : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
              )}
            >
              {t.label}
            </button>
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
                    <Input
                      id={p.env}
                      type="password"
                      autoComplete="off"
                      placeholder={p.env}
                      value={values[p.env] ?? ""}
                      onChange={(e) => setValues((v) => ({ ...v, [p.env]: e.target.value }))}
                    />
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
          {tab === "snippets" && <SnippetsTab />}
          {tab === "prompt" && <SystemPromptTab />}
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

function GeneralTab() {
  const { maxSessions, setMaxSessions, models, model, setModel } = useDesktop();
  const [config, setConfigs] = useState<DesktopConfig>({
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
  });
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

  const set = (k: keyof DesktopConfig, v: string) => {
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
                  value={config[f.key]}
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
                  value={config[f.key]}
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
  const [config, setConfig] = useState<DesktopConfig>({
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
  });
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
              You are overriding the default system prompt, which provides useful context for the agent about the current
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
