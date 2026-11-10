import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, type MarkdownAgent } from "@/lib/tauri";
import { useSidebar } from "@/stores/sidebar";

function useAgentsStore() {
  const { setAgentOf } = useSidebar();
  const [agents, setAgents] = useState<MarkdownAgent[]>(() => []);
  const [activeAgent, setActiveAgent] = useState<string | null>(null);
  const [agentEditor, setAgentEditor] = useState<{ agent: MarkdownAgent | null } | null>(null);
  const loadRan = useRef(false);

  const loadAgents = useCallback(async () => {
    try {
      setAgents(await api.listMarkdownAgents());
    } catch (e) {
      console.error("Failed to load agents:", e);
    }
  }, []);

  useEffect(() => {
    if (loadRan.current) return;
    loadRan.current = true;
    loadAgents();
  }, [loadAgents]);

  const assignAgent = useCallback(
    (sessionId: string, agentName: string) => {
      setAgentOf((prev) => ({ ...prev, [sessionId]: agentName }));
    },
    [setAgentOf],
  );

  const showAgentEditor = useCallback((agent: MarkdownAgent | null) => setAgentEditor({ agent }), []);
  const closeAgentEditor = useCallback(() => setAgentEditor(null), []);

  /** Writes the agent file and its avatar, keeps the chats of a renamed agent
   * with it, and closes the editor. */
  const storeAgent = useCallback(
    async (agent: MarkdownAgent, previousName?: string, avatarUrl?: string) => {
      await api.saveMarkdownAgent(agent, previousName);
      if (avatarUrl) await api.setAgentAvatarFromUrl(agent.name, avatarUrl);
      await loadAgents();
      if (previousName && previousName !== agent.name) {
        setAgentOf((prev) =>
          Object.fromEntries(Object.entries(prev).map(([id, n]) => [id, n === previousName ? agent.name : n])),
        );
      }
      setAgentEditor(null);
    },
    [loadAgents, setAgentOf],
  );

  const deleteAgent = useCallback(
    async (name: string) => {
      await api.deleteMarkdownAgent(name);
      setAgentOf((prev) => Object.fromEntries(Object.entries(prev).filter(([, n]) => n !== name)));
      setActiveAgent((prev) => (prev === name ? null : prev));
      setAgentEditor((prev) => (prev?.agent?.name === name ? null : prev));
      await loadAgents();
    },
    [loadAgents, setAgentOf],
  );

  return useMemo(
    () => ({
      agents,
      loadAgents,
      activeAgent,
      setActiveAgent,
      assignAgent,
      agentEditor,
      showAgentEditor,
      closeAgentEditor,
      storeAgent,
      deleteAgent,
    }),
    [
      agents,
      loadAgents,
      activeAgent,
      assignAgent,
      agentEditor,
      showAgentEditor,
      closeAgentEditor,
      storeAgent,
      deleteAgent,
    ],
  );
}

type AgentsStore = ReturnType<typeof useAgentsStore>;

const AgentsContext = createContext<AgentsStore | null>(null);

export function AgentsProvider({ children }: { children: ReactNode }) {
  return <AgentsContext.Provider value={useAgentsStore()}>{children}</AgentsContext.Provider>;
}

/** The agents the person has defined, the one picked for the next chat, and
 * the editor that creates or changes one. */
export function useAgents(): AgentsStore {
  const ctx = useContext(AgentsContext);
  if (!ctx) throw new Error("useAgents must be used within AgentsProvider");
  return ctx;
}
