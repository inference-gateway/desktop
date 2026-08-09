import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  api,
  Channel,
  type AgentEvent,
  type Conversation,
  type ProgressEvent,
  type UpdateInfo,
} from "@/lib/tauri";
import { chatReducer, initialChatState } from "@/lib/transcript";
import { autoGrow } from "@/lib/textarea";

const STORAGE_KEY = "selectedModel";
const UPDATE_CACHE_KEY = "updateCheck";
const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
const MAX_RETRIES = 10;

export const PROVIDERS = [
  { label: "OpenAI", env: "OPENAI_API_KEY" },
  { label: "Anthropic", env: "ANTHROPIC_API_KEY" },
  { label: "DeepSeek", env: "DEEPSEEK_API_KEY" },
  { label: "Google", env: "GOOGLE_API_KEY" },
  { label: "Groq", env: "GROQ_API_KEY" },
  { label: "Mistral", env: "MISTRAL_API_KEY" },
  { label: "Cohere", env: "COHERE_API_KEY" },
  { label: "Cloudflare", env: "CLOUDFLARE_API_KEY" },
  { label: "NVIDIA", env: "NVIDIA_API_KEY" },
  { label: "Moonshot", env: "MOONSHOT_API_KEY" },
  { label: "MiniMax", env: "MINIMAX_API_KEY" },
  { label: "Ollama Cloud", env: "OLLAMA_CLOUD_API_KEY" },
] as const;

function useDesktopStore() {
  const [chat, dispatch] = useReducer(chatReducer, initialChatState);
  const [statusText, setStatusText] = useState("");
  const [statusError, setStatusErr] = useState(false);
  const [ready, setReady] = useState(false);
  const [running, setRunning] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModelState] = useState<string>(() => localStorage.getItem(STORAGE_KEY) || "");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [updates, setUpdates] = useState<UpdateInfo[]>([]);
  const [currentView, setCurrentView] = useState<"chat" | "settings">("chat");

  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const initRan = useRef(false);
  const lastClickedIndex = useRef(-1);

  const setStatus = useCallback((t: string) => {
    setStatusText(t);
    setStatusErr(false);
  }, []);
  const setError = useCallback((t: string) => {
    setStatusText(t);
    setStatusErr(true);
  }, []);

  const setModel = useCallback((m: string) => {
    setModelState(m);
    localStorage.setItem(STORAGE_KEY, m);
  }, []);

  const populateModels = useCallback((list: string[]) => {
    setModels(list);
    setModelState((cur) => {
      if (cur && list.includes(cur)) return cur;
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved && list.includes(saved)) return saved;
      return list[0] ?? "";
    });
  }, []);

  const fetchModelsWithRetry = useCallback(
    async (attempt = 0) => {
      try {
        populateModels(await api.listModels());
      } catch {
        if (attempt < MAX_RETRIES) setTimeout(() => fetchModelsWithRetry(attempt + 1), 1500);
      }
    },
    [populateModels]
  );

  const startGatewayThenModels = useCallback(
    async (force = false) => {
      setStatus("Starting gateway...");
      try {
        await api.startGateway(force);
      } catch (e) {
        console.error("start_gateway failed:", e);
      }
      setStatus("Ready");
      fetchModelsWithRetry();
    },
    [setStatus, fetchModelsWithRetry]
  );

  const refreshConversations = useCallback(async () => {
    try {
      const data = JSON.parse(await api.listConversations());
      setConversations(data.conversations || []);
    } catch (e) {
      console.error("Failed to load conversations:", e);
    }
  }, []);

  const checkForUpdates = useCallback(async (force = false) => {
    const cached = JSON.parse(localStorage.getItem(UPDATE_CACHE_KEY) || "null");
    if (!force && cached && Date.now() - cached.checkedAt < UPDATE_INTERVAL_MS) {
      setUpdates(cached.updates);
      return;
    }
    try {
      const list = await api.checkUpdates();
      localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ checkedAt: Date.now(), updates: list }));
      setUpdates(list);
    } catch (e) {
      console.error("check_updates failed:", e);
      if (cached) setUpdates(cached.updates);
    }
  }, []);

  const initBackend = useCallback(
    async (force = false) => {
      try {
        const ch = new Channel<ProgressEvent>();
        ch.onmessage = (event) => {
          switch (event.kind) {
            case "Checking":
              setStatus("Checking for infer binary...");
              break;
            case "Downloading":
              setStatus(
                event.total > 0
                  ? `Downloading infer... ${Math.round((event.received / event.total) * 100)}%`
                  : "Downloading infer..."
              );
              break;
            case "Verifying":
              setStatus("Verifying download...");
              break;
            case "Installing":
              setStatus("Installing infer...");
              break;
            case "Initializing":
              setStatus("Running initial setup...");
              break;
            case "Ready":
              setStatus("Ready");
              setReady(true);
              startGatewayThenModels(force).then(() => checkForUpdates(force));
              refreshConversations();
              break;
            case "Error":
              setError(`Error: ${event.message}`);
              break;
          }
        };
        await api.checkAndInstallCli(ch, force);
      } catch (err) {
        setError(`Setup failed: ${err}`);
      }
    },
    [setStatus, setError, startGatewayThenModels, checkForUpdates, refreshConversations]
  );

  const restartBackend = useCallback(
    async (force: boolean) => {
      if (running) {
        try {
          await api.cancelAgent();
        } catch (e) {
          console.error("Cancel failed:", e);
        }
      }
      setStatus(force ? "Updating..." : "Restarting CLI...");
      setReady(false);
      await initBackend(force);
    },
    [running, setStatus, initBackend]
  );

  useEffect(() => {
    if (initRan.current) return;
    initRan.current = true;
    // Invalidate the cached update check on launch so a new version is detected,
    // keeping the stale data as an offline fallback.
    const staleRaw = localStorage.getItem(UPDATE_CACHE_KEY);
    if (staleRaw) {
      try {
        const stale = JSON.parse(staleRaw);
        localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ ...stale, checkedAt: 0 }));
      } catch {
        localStorage.removeItem(UPDATE_CACHE_KEY);
      }
    }
    initBackend();
    const t = setInterval(() => checkForUpdates(true), UPDATE_INTERVAL_MS);
    return () => clearInterval(t);
  }, [initBackend, checkForUpdates]);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    lastClickedIndex.current = -1;
  }, []);

  const openConversation = useCallback(
    async (id: string) => {
      if (running) return;
      try {
        const ndjson = await api.getConversation(id);
        setSessionId(id);
        dispatch({ type: "loadHistory", ndjson });
      } catch (err) {
        dispatch({ type: "error", text: `Failed to load conversation: ${err}` });
      }
    },
    [running]
  );

  const newChat = useCallback(() => {
    if (running) return;
    setSessionId(null);
    dispatch({ type: "newChat" });
    clearSelection();
    composerRef.current?.focus();
  }, [running, clearSelection]);

  const deleteConversation = useCallback(
    async (id: string) => {
      if (running) return;
      try {
        await api.deleteConversation(id);
        if (id === sessionId) newChat();
        await refreshConversations();
      } catch (err) {
        dispatch({ type: "error", text: `Failed to delete conversation: ${err}` });
      }
    },
    [running, sessionId, newChat, refreshConversations]
  );

  const onChatClick = useCallback(
    (index: number, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => {
      const list = conversations;
      if (e.shiftKey) {
        if (lastClickedIndex.current < 0) lastClickedIndex.current = index;
        const start = Math.min(lastClickedIndex.current, index);
        const end = Math.max(lastClickedIndex.current, index);
        setSelected((prev) => {
          const next = new Set(prev);
          for (let i = start; i <= end; i++) {
            const id = list[i]?.id;
            if (id) next.add(id);
          }
          return next;
        });
        lastClickedIndex.current = index;
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        const id = list[index]?.id;
        if (!id) return;
        setSelected((prev) => {
          const next = new Set(prev);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        });
        return;
      }
      if (selected.size > 0) clearSelection();
      lastClickedIndex.current = index;
      openConversation(list[index].id);
    },
    [conversations, selected, clearSelection, openConversation]
  );

  const bulkDelete = useCallback(() => {
    const ids = Array.from(selected);
    clearSelection();
    Promise.all(ids.map((id) => deleteConversation(id)));
  }, [selected, clearSelection, deleteConversation]);

  const send = useCallback(async () => {
    const el = composerRef.current;
    const text = el?.value.trim() ?? "";
    if (!text || running) return;
    if (!model) {
      setError("Please select a model first");
      return;
    }
    setRunning(true);
    setStatus("Running...");
    dispatch({ type: "userSend", text });
    if (el) {
      el.value = "";
      autoGrow(el);
    }
    try {
      const ch = new Channel<AgentEvent>();
      ch.onmessage = (event) => {
        dispatch({ type: "event", event });
        switch (event.kind) {
          case "SessionId":
            setSessionId(event.session_id);
            break;
          case "ApprovalRequest":
            setStatus("Awaiting approval...");
            break;
          case "Done":
            setRunning(false);
            setStatus(event.exit_code === 0 ? "Done" : `Exited with code ${event.exit_code}`);
            break;
          case "Cancelled":
            setRunning(false);
            setStatus("Cancelled");
            break;
        }
      };
      const newSessionId = await api.sendMessage({ prompt: text, model, sessionId, onEvent: ch });
      if (newSessionId) setSessionId(newSessionId);
      refreshConversations();
    } catch (err) {
      dispatch({ type: "error", text: `Error: ${err}` });
      setRunning(false);
      setStatus("Error");
    }
  }, [running, model, sessionId, setStatus, setError, refreshConversations]);

  const cancel = useCallback(async () => {
    if (!running) return;
    try {
      await api.cancelAgent();
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }, [running]);

  const approve = useCallback(async (callId: string, approved: boolean) => {
    try {
      await api.sendApproval(callId, approved);
      dispatch({ type: "setApproval", callId, status: approved ? "approved" : "denied" });
    } catch (err) {
      dispatch({ type: "error", text: `Approval failed: ${err}` });
    }
  }, []);

  const openSettings = useCallback(() => {
    checkForUpdates();
    setCurrentView("settings");
  }, [checkForUpdates]);

  const saveSettings = useCallback(
    async (keys: Record<string, string>) => {
      try {
        await api.setAuth(keys);
        setCurrentView("chat");
        startGatewayThenModels();
      } catch (err) {
        setError(`Failed to save settings: ${err}`);
      }
    },
    [startGatewayThenModels, setError]
  );

  const applyUpdates = useCallback(async () => {
    await restartBackend(true);
    if (updates.some((u) => u.outdated && u.name === "Desktop")) {
      setStatus("Updating app...");
      try {
        await api.installDesktopUpdate();
      } catch (err) {
        dispatch({ type: "error", text: `App update failed: ${err}` });
      }
    }
  }, [restartBackend, updates, setStatus]);

  const outdated = updates.filter((u) => u.outdated);
  const versionBadge = updates.map((u) => `${u.name} ${u.current}`).join(" · ");
  const updateBannerText = outdated.length
    ? `${outdated.map((u) => `${u.name} ${u.latest}`).join(", ")} available - restart to update`
    : "";

  return {
    items: chat.items,
    typing: chat.typing,
    statusText,
    statusError,
    ready,
    running,
    enabled: ready && !running,
    models,
    model,
    setModel,
    conversations,
    selected,
    sessionId,
    onChatClick,
    clearSelection,
    deleteConversation,
    bulkDelete,
    send,
    cancel,
    approve,
    openConversation,
    newChat,
    restartBackend,
    currentView,
    openSettings,
    setCurrentView,
    saveSettings,
    getAuth: () => api.getAuth(),
    updates,
    versionBadge,
    updateBannerText,
    showUpdateBanner: outdated.length > 0,
    checkForUpdates,
    applyUpdates,
    composerRef,
    setStatus,
    setError,
  };
}

export type DesktopStore = ReturnType<typeof useDesktopStore>;

const DesktopContext = createContext<DesktopStore | null>(null);

export function DesktopProvider({ children }: { children: ReactNode }) {
  const store = useDesktopStore();
  return <DesktopContext.Provider value={store}>{children}</DesktopContext.Provider>;
}

export function useDesktop(): DesktopStore {
  const ctx = useContext(DesktopContext);
  if (!ctx) throw new Error("useDesktop must be used within DesktopProvider");
  return ctx;
}
