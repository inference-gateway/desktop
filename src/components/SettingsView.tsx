import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, type A2aAgent } from "@/lib/tauri";
import { PROVIDERS, useDesktop } from "@/store";

export function SettingsView() {
  const { setCurrentView, saveSettings, getAuth, updates, checkForUpdates, applyUpdates, showUpdateBanner } =
    useDesktop();
  const [values, setValues] = useState<Record<string, string>>({});
  const [checking, setChecking] = useState(false);
  const [agents, setAgents] = useState<A2aAgent[]>([]);
  const [newAgentUrl, setNewAgentUrl] = useState("");

  const loadAgents = async () => {
    try {
      setAgents(await api.listA2aAgents());
    } catch (e) {
      console.error("Failed to load A2A agents:", e);
    }
  };

  useEffect(() => {
    getAuth()
      .then((auth) => {
        const next: Record<string, string> = {};
        for (const p of PROVIDERS) next[p.env] = (auth && auth[p.env]) || "";
        setValues(next);
      })
      .catch(() => {});
    loadAgents();
  }, [getAuth]);

  const onSave = () => {
    const keys: Record<string, string> = {};
    for (const p of PROVIDERS) keys[p.env] = (values[p.env] || "").trim();
    saveSettings(keys);
  };

  const addAgent = async () => {
    const url = newAgentUrl.trim();
    if (!url) return;  
    try {
      await api.addA2aAgent(url);
      setNewAgentUrl("");
      await loadAgents();
    } catch (e) {
      console.error("Failed to add A2A agent:", e);
    }
  };

  const removeAgent = async (url: string) => {
    try {
      await api.removeA2aAgent(url);
      await loadAgents();
    } catch (e) {
      console.error("Failed to remove A2A agent:", e);
    }
  };

  return (
    <div id="settings-view" className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
      <div className="mx-auto w-full max-w-[540px]">
        <h1 className="mb-6 text-xl font-semibold">Settings</h1>

        {/* API Keys */}
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

        {/* Updates */}
        <h2 className="mt-5 text-[1.05rem] font-semibold">Updates</h2>
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

        {/* A2A Agents */}
        <h2 className="mt-5 text-[1.05rem] font-semibold">A2A Agents</h2>
        <p className="mb-4 text-[0.8rem] text-muted-foreground">
          Remote A2A agents your local infer agent can delegate to. Agents are persisted and loaded on startup.
        </p>
        {agents.length === 0 ? (
          <p className="mb-3 text-[0.8rem] text-muted-foreground">No A2A agents configured.</p>
        ) : (
          <ul className="mb-3 flex flex-col gap-2">
            {agents.map((a) => (
              <li key={a.url} className="flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-[0.85rem]">
                <span className="flex-1 truncate">{a.url}</span>
                <span className="text-[0.75rem] text-muted-foreground">{a.status || "configured"}</span>
                <Button variant="outline" size="xs" onClick={() => removeAgent(a.url)}>
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
            onKeyDown={(e) => { if (e.key === "Enter") addAgent(); }}
          />
          <Button size="sm" onClick={addAgent} disabled={!newAgentUrl.trim()}>
            Add
          </Button>
        </div>

        {/* Actions */}
        <div className="mt-6 flex items-center gap-2 border-t border-border pt-4">
          <Button variant="outline" onClick={() => setCurrentView("chat")}>
            Back to Chat
          </Button>
          <Button onClick={onSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
