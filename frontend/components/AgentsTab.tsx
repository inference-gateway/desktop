import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api, type A2aAgent } from "@/lib/tauri";
import { fetchAgentCatalog, type CatalogAgent } from "@/lib/registry";
import { AgentAvatar } from "@/components/AgentAvatar";
import { useDesktop } from "@/store";
import { useAgents } from "@/stores/agents";
import { useBackend } from "@/stores/backend";
import { useStatusActions } from "@/stores/status";

export function AgentsTab() {
  const { models, startServices } = useBackend();
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
      startServices();
    } catch (e) {
      console.error("Failed to toggle A2A agent:", e);
    }
  };

  const setModel = async (name: string, model: string) => {
    try {
      await api.setA2aAgentModel(name, model);
      await loadAgents();
      startServices();
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
        Your Markdown agents, plus the A2A agents your local infer agent can delegate to. Toggle the local A2A agents
        you want, or add a remote agent by URL. Selections are persisted and loaded on startup.
      </p>
      <MarkdownAgentsCard />

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
                        <span
                          key={s}
                          className="rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground"
                        >
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

// The Markdown agents the sidebar lists, from the dirs the CLI reads them
// from: .infer/agents and .agents/agents, then ~/.infer/agents and
// ~/.agents/agents. A new agent is saved to ~/.infer/agents.
function MarkdownAgentsCard() {
  const { setError } = useStatusActions();
  const { agents, deleteAgent } = useAgents();
  const { openAgentEditor } = useDesktop();
  return (
    <>
      <h3 className="text-[0.9rem] font-semibold">Your agents</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Markdown agents you chat with from the sidebar's Agents filter, one <code>&lt;name&gt;.md</code> per agent
        (Claude Code format), read from <code>.infer/agents</code>, <code>.agents/agents</code>,{" "}
        <code>~/.infer/agents</code> and <code>~/.agents/agents</code> - the same places the CLI looks. New agents go to{" "}
        <code>~/.infer/agents</code>. Each gets a robot face drawn from its name until you pick an animated emoji or a
        picture, and agents can delegate to each other through the Agent tool.
      </p>
      <div className="mb-6 flex flex-col gap-1.5">
        {agents.map((agent) => (
          <div key={agent.name} className="flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2">
            <AgentAvatar agent={agent} size={28} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[0.85rem] font-medium">{agent.name}</div>
              <div className="truncate text-[0.72rem] text-muted-foreground">{agent.description}</div>
            </div>
            <Button size="sm" variant="outline" onClick={() => openAgentEditor(agent)}>
              Edit
            </Button>
            <Button size="sm" variant="ghost" onClick={() => deleteAgent(agent.name).catch((e) => setError(String(e)))}>
              Delete
            </Button>
          </div>
        ))}
        <Button size="sm" variant="outline" className="self-start" onClick={() => openAgentEditor(null)}>
          New agent
        </Button>
      </div>
    </>
  );
}
