import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  api,
  Channel,
  type AgentEvent,
  type Conversation,
  type DesktopConfig,
  type ProgressEvent,
  type UpdateInfo,
} from "@/lib/tauri";
import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { chatReducer, initialChatState, COMPUTER_USE_TOOLS, type ChatAction, type ChatState } from "@/lib/transcript";
import { autoGrow } from "@/lib/textarea";
import { loadSnippets, saveSnippets, defaultForId, DEFAULT_SNIPPETS, type Snippet } from "@/lib/snippets";

const STORAGE_KEY = "selectedModel";
const AUTO_MODE_KEY = "autoMode";
const MAX_SESSIONS_KEY = "maxConcurrentSessions";
const DEFAULT_MAX_SESSIONS = 5;
const UPDATE_CACHE_KEY = "updateCheck";
const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
const MAX_RETRIES = 10;

function setMonitorVisible(visible: boolean) {
  WebviewWindow.getByLabel("monitor")
    .then((w) => (visible ? w?.show() : w?.hide()))
    .catch(() => {});
}

async function notifyApproval(toolName: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title: "Approval needed", body: toolName });
  } catch {
    /* notifications are best-effort */
  }
}

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
  const [transcripts, setTranscripts] = useState<Record<string, ChatState>>({});
  const [runningIds, setRunningIds] = useState<Set<string>>(() => new Set());
  const [activeId, setActiveId] = useState<string | null>(null);
  const activeIdRef = useRef<string | null>(null);
  const [statusText, setStatusText] = useState("");
  const [statusError, setStatusErr] = useState(false);
  const [ready, setReady] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModelState] = useState<string>(() => localStorage.getItem(STORAGE_KEY) || "");
  const [autoMode, setAutoModeState] = useState(() => localStorage.getItem(AUTO_MODE_KEY) === "true");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [maxSessions, setMaxSessionsState] = useState<number>(() => {
    const n = parseInt(localStorage.getItem(MAX_SESSIONS_KEY) || "", 10);
    return Number.isFinite(n) && n >= 1 ? n : DEFAULT_MAX_SESSIONS;
  });
  const [updates, setUpdates] = useState<UpdateInfo[]>([]);
  const [currentView, setCurrentView] = useState<"chat" | "settings" | "observability">("chat");
  const [history, setHistory] = useState<string[]>([]);
  const [snippets, setSnippetsState] = useState<Snippet[]>(() => loadSnippets());
  const [tokenUsage, setTokenUsage] = useState({ input: 0, output: 0, cached_read: 0, total_tool_calls: 0 });
  const [projects, setProjects] = useState<Record<string, string>>(() => ({}));
  const [projectNames, setProjectNames] = useState<string[]>(() => []);
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => new Set());
  const [activeProject, setActiveProject] = useState<string | null>(null);
  const [projectContexts, setProjectContexts] = useState<Record<string, string>>(() => ({}));
  const [projectsLoaded, setProjectsLoaded] = useState(false);
  const [initialSettingsTab, setInitialSettingsTab] = useState("general");

  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const monitorShown = useRef(false);
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
    api.setDefaultModel(m).catch(() => {});
  }, []);

  const setAutoMode = useCallback((enabled: boolean) => {
    setAutoModeState(enabled);
    localStorage.setItem(AUTO_MODE_KEY, String(enabled));
  }, []);

  const setMaxSessions = useCallback((n: number) => {
    const v = Number.isFinite(n) && n >= 1 ? Math.floor(n) : DEFAULT_MAX_SESSIONS;
    setMaxSessionsState(v);
    localStorage.setItem(MAX_SESSIONS_KEY, String(v));
  }, []);

  const dispatchTo = useCallback((id: string, action: ChatAction) => {
    setTranscripts((prev) => ({ ...prev, [id]: chatReducer(prev[id] ?? initialChatState, action) }));
  }, []);

  useEffect(() => {
    const unlisten = listen<{ sessionId: string; callId: string; status: "approved" | "denied" }>(
      "approval-resolved",
      (e) =>
        dispatchTo(e.payload.sessionId, {
          type: "setApproval",
          callId: e.payload.callId,
          status: e.payload.status,
        })
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [dispatchTo]);

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

  useEffect(() => {
    window.addEventListener("focus", refreshConversations);
    return () => window.removeEventListener("focus", refreshConversations);
  }, [refreshConversations]);

  const loadProjects = useCallback(async () => {
    try {
      const raw = await api.readProjects();
      const parsed = JSON.parse(raw);
      const map: Record<string, string> = {};
      const names = new Set<string>();
      if (Array.isArray(parsed?.names)) {
        for (const n of parsed.names) {
          if (typeof n === "string" && n.trim()) names.add(n);
        }
      }
      for (const [id, name] of Object.entries(parsed?.assignments ?? {})) {
        if (typeof name === "string" && name.trim()) {
          map[id] = name;
          names.add(name);
        }
      }
      const contexts: Record<string, string> = {};
      for (const [name, ctx] of Object.entries(parsed?.contexts ?? {})) {
        if (typeof ctx === "string" && ctx.trim()) contexts[name] = ctx;
      }
      setProjects(map);
      setProjectNames(Array.from(names));
      setProjectContexts(contexts);
    } catch (e) {
      console.error("Failed to load projects:", e);
    } finally {
      setProjectsLoaded(true);
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
      for (const id of runningIds) {
        try {
          await api.cancelAgent(id);
        } catch (e) {
          console.error("Cancel failed:", e);
        }
      }
      setRunningIds(new Set());
      setStatus(force ? "Updating..." : "Restarting CLI...");
      setReady(false);
      await initBackend(force);
    },
    [runningIds, setStatus, initBackend]
  );

  useEffect(() => {
    if (initRan.current) return;
    initRan.current = true;

    const staleRaw = localStorage.getItem(UPDATE_CACHE_KEY);
    if (staleRaw) {
      try {
        const stale = JSON.parse(staleRaw);
        localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ ...stale, checkedAt: 0 }));
      } catch {
        localStorage.removeItem(UPDATE_CACHE_KEY);
      }
    }

    if (!localStorage.getItem(STORAGE_KEY)) {
      api
        .getConfig()
        .then((cfg) => {
          if (cfg.default_model && !localStorage.getItem(STORAGE_KEY)) {
            localStorage.setItem(STORAGE_KEY, cfg.default_model);
          }
        })
        .catch(() => {});
    }
    initBackend();
    api.readHistory().then(setHistory).catch(() => {});
    loadProjects();
    const t = setInterval(() => checkForUpdates(true), UPDATE_INTERVAL_MS);
    return () => clearInterval(t);
  }, [initBackend, checkForUpdates]);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    lastClickedIndex.current = -1;
  }, []);

  // Running sessions not yet persisted to disk still need a sidebar entry so the
  // user can switch back to them; show them first, titled by their first prompt.
  const displayConversations = useMemo(() => {
    const known = new Set(conversations.map((c) => c.id));
    const pending: Conversation[] = [];
    for (const id of runningIds) {
      if (known.has(id)) continue;
      const first = transcripts[id]?.items.find((it) => it.kind === "user");
      pending.push({ id, title: first && first.kind === "user" ? first.text : "New chat" });
    }
    return [...pending, ...conversations];
  }, [conversations, runningIds, transcripts]);

  const isRunning = useCallback((id: string) => runningIds.has(id), [runningIds]);

  const isAwaitingApproval = useCallback(
    (id: string) =>
      transcripts[id]?.items.some((it) => it.kind === "approval" && it.status === "pending") ?? false,
    [transcripts]
  );

  const openConversation = useCallback(
    async (id: string) => {
      setActiveId(id);
      activeIdRef.current = id;
      setActiveProject(projects[id] ?? null);
      if (transcripts[id]) return;
      try {
        const ndjson = await api.getConversation(id);
        dispatchTo(id, { type: "loadHistory", ndjson });
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Failed to load conversation: ${err}` });
      }
    },
    [transcripts, projects, dispatchTo]
  );

  const newChat = useCallback(() => {
    setActiveId(null);
    activeIdRef.current = null;
    clearSelection();
    composerRef.current?.focus();
  }, [clearSelection, composerRef]);

  const deleteConversation = useCallback(
    async (id: string) => {
      if (runningIds.has(id)) {
        try {
          await api.cancelAgent(id);
        } catch (e) {
          console.error("Cancel failed:", e);
        }
        setRunningIds((prev) => {
          const next = new Set(prev);
          next.delete(id);
          return next;
        });
      }
      try {
        await api.deleteConversation(id);
        setTranscripts((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });
        if (id === activeId) newChat();
        await refreshConversations();
      } catch (err) {
        setError(`Failed to delete conversation: ${err}`);
      }
    },
    [runningIds, activeId, newChat, refreshConversations, setError]
  );

  const onChatClick = useCallback(
    (index: number, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => {
      const list = displayConversations;
      if (e.shiftKey) {
        if (lastClickedIndex.current < 0) lastClickedIndex.current = index;
        const order: number[] = [];
        for (const name of projectNames) {
          list.forEach((conv, i) => {
            if (projects[conv.id] === name) order.push(i);
          });
        }
        list.forEach((conv, i) => {
          if (!projectNames.includes(projects[conv.id])) order.push(i);
        });
        const start = order.indexOf(lastClickedIndex.current);
        const end = order.indexOf(index);
        setSelected((prev) => {
          const next = new Set(prev);
          for (let i = Math.min(start, end); i <= Math.max(start, end); i++) {
            const id = list[order[i]]?.id;
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
    [displayConversations, projects, projectNames, selected, clearSelection, openConversation]
  );

  const bulkDelete = useCallback(() => {
    const ids = Array.from(selected);
    clearSelection();
    Promise.all(ids.map((id) => deleteConversation(id)));
  }, [selected, clearSelection, deleteConversation]);

  useEffect(() => {
    if (!projectsLoaded) return;
    api
      .writeProjects(JSON.stringify({ assignments: projects, names: projectNames, contexts: projectContexts }))
      .catch(() => {});
  }, [projectsLoaded, projects, projectNames, projectContexts]);

  const assignProject = useCallback((sessionId: string, projectName: string) => {
    setProjects((prev) => ({ ...prev, [sessionId]: projectName }));
    setProjectNames((prev) => (prev.includes(projectName) ? prev : [...prev, projectName]));
  }, []);

  const sendPrompt = useCallback(async (runId: string, text: string, projectName?: string) => {
    if (runningIds.has(runId)) return;
    if (!model) {
      setError("Please select a model first");
      return;
    }
    setStatus("Running...");
    dispatchTo(runId, { type: "userSend", text });
    setRunningIds((prev) => new Set(prev).add(runId));
    try {
      const ch = new Channel<AgentEvent>();
      ch.onmessage = (event) => {
        dispatchTo(runId, { type: "event", event });
        if (
          event.kind === "AssistantMessage" &&
          !monitorShown.current &&
          event.tool_calls.some((tc) => COMPUTER_USE_TOOLS.has(tc.name))
        ) {
          monitorShown.current = true;
          setMonitorVisible(true);
        }
        if (event.kind === "TokenUsage") {
          setTokenUsage((prev) => ({
            input: prev.input + event.input,
            output: prev.output + event.output,
            cached_read: prev.cached_read + event.cached_read,
            total_tool_calls: prev.total_tool_calls + event.total_tool_calls,
          }));
        }
        switch (event.kind) {
          case "ApprovalRequest":
            if (runId === activeIdRef.current) setStatus("Awaiting approval...");
            if (!document.hasFocus()) notifyApproval(event.tool_name);
            break;
          case "Done":
            setRunningIds((prev) => {
              const next = new Set(prev);
              next.delete(runId);
              if (next.size === 0) monitorShown.current = false;
              return next;
            });
            if (runId === activeIdRef.current)
              setStatus(event.exit_code === 0 ? "Done" : `Exited with code ${event.exit_code}`);
            break;
          case "Cancelled":
            setRunningIds((prev) => {
              const next = new Set(prev);
              next.delete(runId);
              if (next.size === 0) monitorShown.current = false;
              return next;
            });
            if (runId === activeIdRef.current) setStatus("Cancelled");
            break;
        }
      };
      const cfg = await api.getConfig();
      const projectContext = projectName ? projectContexts[projectName] : undefined;
      await api.sendMessage({
        prompt: text,
        model,
        sessionId: runId,
        onEvent: ch,
        systemPrompt: cfg.system_prompt || undefined,
        extraInstructions:
          [cfg.extra_instructions, projectContext].filter(Boolean).join("\n\n") || undefined,
        autoMode,
      });
      refreshConversations();
    } catch (err) {
      dispatchTo(runId, { type: "error", text: `Error: ${err}` });
      setRunningIds((prev) => {
        const next = new Set(prev);
        next.delete(runId);
        if (next.size === 0) monitorShown.current = false;
        return next;
      });
      if (runId === activeIdRef.current) setStatus("Error");
    }
  }, [runningIds, model, autoMode, projectContexts, setStatus, setError, refreshConversations, dispatchTo]);

  useEffect(() => {
    const unlisten = listen<{ sessionId: string; text: string }>("monitor-send", (e) =>
      sendPrompt(e.payload.sessionId, e.payload.text, projects[e.payload.sessionId])
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [sendPrompt, projects]);

  const send = useCallback(async () => {
    const el = composerRef.current;
    const text = el?.value.trim() ?? "";
    if (!text) return;
    if (activeId && runningIds.has(activeId)) return;
    if (!model) {
      setError("Please select a model first");
      return;
    }
    if (!activeId && runningIds.size >= maxSessions) {
      setError(`Max ${maxSessions} concurrent sessions reached - stop one to start another`);
      return;
    }
    const runId = activeId ?? crypto.randomUUID();
    if (!activeId && activeProject) assignProject(runId, activeProject);
    setActiveId(runId);
    activeIdRef.current = runId;
    api.appendHistory(text).catch(() => {});
    setHistory((h) => [...h, text]);
    if (el) {
      el.value = "";
      autoGrow(el);
    }
    await sendPrompt(runId, text, (activeId ? projects[activeId] : activeProject) ?? undefined);
  }, [activeId, runningIds, model, maxSessions, activeProject, assignProject, projects, sendPrompt, setError]);

  const cancel = useCallback(async () => {
    if (!activeId || !runningIds.has(activeId)) return;
    try {
      await api.cancelAgent(activeId);
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }, [activeId, runningIds]);

  const approve = useCallback(
    async (callId: string, approved: boolean) => {
      const id = activeIdRef.current;
      if (!id) return;
      try {
        await api.sendApproval(id, callId, approved);
        const status = approved ? "approved" : "denied";
        dispatchTo(id, { type: "setApproval", callId, status });
        emit("approval-resolved", { sessionId: id, callId, status }).catch(() => {});
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Approval failed: ${err}` });
      }
    },
    [dispatchTo]
  );

  const insertSnippet = useCallback((prompt: string) => {
    const el = composerRef.current;
    if (!el) return;
    el.value = prompt;
    autoGrow(el);
    el.focus();
  }, []);

  const resetSnippet = useCallback((id: string) => {
    const def = defaultForId(id);
    if (!def) return;
    setSnippetsState((prev) => {
      const next = prev.map((s) => (s.id === id ? { ...def } : s));
      saveSnippets(next);
      return next;
    });
  }, []);

  const updateSnippet = useCallback(
    (id: string, patch: { label?: string; prompt?: string }) => {
      setSnippetsState((prev) => {
        const next = prev.map((s) => (s.id === id ? { ...s, ...patch } : s));
        saveSnippets(next);
        return next;
      });
    },
    [],
  );

  const resetAllSnippets = useCallback(() => {
    setSnippetsState(DEFAULT_SNIPPETS);
    saveSnippets(DEFAULT_SNIPPETS);
  }, []);

  const openSettings = useCallback(() => {
    checkForUpdates();
    setCurrentView("settings");
  }, [checkForUpdates]);

  const openObservability = useCallback(() => {
    setCurrentView("observability");
  }, []);

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
        setError(`App update failed: ${err}`);
      }
    }
  }, [restartBackend, updates, setStatus, setError]);

  const outdated = updates.filter((u) => u.outdated);
  const versionBadge = updates.map((u) => `${u.name} ${u.current}`).join(" · ");
  const updateBannerText = outdated.length
    ? `${outdated.map((u) => `${u.name} ${u.latest}`).join(", ")} available - restart to update`
    : "";

  const unassignProject = useCallback((sessionId: string) => {
    setProjects((prev) => {
      const next = { ...prev };
      delete next[sessionId];
      return next;
    });
  }, []);

  const createProject = useCallback((name: string) => {
    setProjectNames((prev) => (prev.includes(name) ? prev : [...prev, name]));
  }, []);

  const deleteProject = useCallback((name: string) => {
    setProjects((prev) =>
      Object.fromEntries(Object.entries(prev).filter(([, p]) => p !== name))
    );
    setProjectNames((prev) => prev.filter((n) => n !== name));
    setProjectContexts((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });
    setActiveProject((p) => (p === name ? null : p));
  }, []);

  const renameProject = useCallback((oldName: string, newName: string) => {
    setProjects((prev) =>
      Object.fromEntries(Object.entries(prev).map(([id, p]) => [id, p === oldName ? newName : p]))
    );
    setProjectNames((prev) => prev.map((n) => (n === oldName ? newName : n)));
    setProjectContexts((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setActiveProject((p) => (p === oldName ? newName : p));
  }, []);

  const setProjectContext = useCallback((name: string, context: string) => {
    setProjectContexts((prev) => ({ ...prev, [name]: context }));
  }, []);

  const toggleCollapseProject = useCallback((name: string) => {
        setCollapsedProjects((prev) => {
          const next = new Set(prev);
          if (next.has(name)) next.delete(name);
          else next.add(name);
          return next;
        });
  }, []);

  const currentProject = (activeId ? projects[activeId] : null) ?? activeProject;

  const active = (activeId && transcripts[activeId]) || initialChatState;
  const running = activeId != null && runningIds.has(activeId);

  return {
    items: active.items,
    typing: active.typing,
    statusText,
    statusError,
    ready,
    running,
    enabled: ready && !running,
    models,
    model,
    setModel,
    autoMode,
    setAutoMode,
    maxSessions,
    setMaxSessions,
    conversations: displayConversations,
    selected,
    sessionId: activeId,
    isRunning,
    isAwaitingApproval,
    runningCount: runningIds.size,
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
    openObservability,
    setCurrentView,
    saveSettings,
    getConfig: () => api.getConfig(),
    getAuth: () => api.getAuth(),
    saveConfig: (cfg: DesktopConfig) => api.setConfig(cfg),
    updates,
    versionBadge,
    updateBannerText,
    showUpdateBanner: outdated.length > 0,
    checkForUpdates,
    applyUpdates,
    composerRef,
    history,
    snippets,
    insertSnippet,
    updateSnippet,
    resetSnippet,
    resetAllSnippets,
    setStatus,
    setError,
    tokenUsage,
    projects,
    projectNames,
    collapsedProjects,
    activeProject,
    setActiveProject,
    currentProject,
    projectContexts,
    setProjectContext,
    initialSettingsTab,
    setInitialSettingsTab,
    assignProject,
    unassignProject,
    createProject,
    deleteProject,
    renameProject,
    toggleCollapseProject,
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
