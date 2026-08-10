import { useEffect, useMemo, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { api, type A2aAgent } from "@/lib/tauri";
import { fetchAgentCatalog, type CatalogAgent } from "@/lib/registry";
import { PROVIDERS, useDesktop } from "@/store";

type Tab = "general" | "keys" | "updates" | "agents";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "keys", label: "API Keys" },
  { id: "agents", label: "Agents" },
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
    maxSessions,
    setMaxSessions,
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
          {tab === "general" && (
            <>
              <h2 className="text-[1.05rem] font-semibold">General</h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">
                Run multiple agent sessions at once. Each session is a separate infer agent process.
              </p>
              <div className="flex flex-col gap-1">
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
            </>
          )}

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
        </div>
      </div>
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
    // ponytail: fetch catalog on tab open — small CDN-cached file; cache only if it drags.
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

  // ponytail: register by name only — the CLI (agent_defaults.go) assigns each known agent a
  // collision-free host port; passing a URL would override that onto the gateway's own :8080.
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
