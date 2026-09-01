import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import {
  chatReducer,
  delegationsFrom,
  initialChatState,
  COMPUTER_USE_TOOLS,
  type ChatAction,
  type ChatState,
  type Delegation,
} from "@/lib/transcript";
import { autoGrow } from "@/lib/textarea";
import { matchShortcut } from "@/lib/shortcuts";
import {
  loadSnippets,
  saveSnippets,
  defaultForId,
  DEFAULT_SNIPPETS,
  mergeSnippets,
  type Snippet,
} from "@/lib/snippets";
import { hydrateRegistry } from "@/lib/skills";

const STORAGE_KEY = "selectedModel";
const AUTO_MODE_KEY = "autoMode";
const MAX_SESSIONS_KEY = "maxConcurrentSessions";
const DEFAULT_MAX_SESSIONS = 5;
const UPDATE_CACHE_KEY = "updateCheck";
const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
const MAX_RETRIES = 10;
const LAST_RUN_TTL_MS = 10_000;

async function setMonitorVisible(visible: boolean) {
  const win = await WebviewWindow.getByLabel("monitor");
  if (!win) return;
  await win.setIgnoreCursorEvents(!visible);
  if (visible) await win.show();
  else await win.hide();
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

const INIT_PR_INSTRUCTION =
  "After /init completes, open a pull request containing the AGENTS.md changes for human review - do not merge.";

function useDesktopStore() {
  const [transcripts, setTranscripts] = useState<Record<string, ChatState>>({});
  const [runningIds, setRunningIds] = useState<Set<string>>(() => new Set());
  const [initAllRunning, setInitAllRunning] = useState(false);
  const [initSelecting, setInitSelecting] = useState(false);
  const [initSelection, setInitSelection] = useState<Set<string>>(() => new Set());
  const [activeId, setActiveId] = useState<string | null>(null);
  const activeIdRef = useRef<string | null>(null);
  const [statusText, setStatusText] = useState("");
  const [statusError, setStatusErr] = useState(false);
  const [lastRun, setLastRun] = useState<Record<string, { label: string; error: boolean }>>({});
  const lastRunTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const [ready, setReady] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModelState] = useState<string>(() => localStorage.getItem(STORAGE_KEY) || "");
  const [autoMode, setAutoModeState] = useState(() => localStorage.getItem(AUTO_MODE_KEY) === "true");
  const [autoModes, setAutoModes] = useState<Record<string, boolean>>({});
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
  const [projectPaths, setProjectPaths] = useState<Record<string, string>>(() => ({}));
  const [projectGroups, setProjectGroups] = useState<Record<string, string>>(() => ({}));
  const [projectsLoaded, setProjectsLoaded] = useState(false);
  const [gitProjects, setGitProjects] = useState<Set<string>>(() => new Set());
  const [dirtyProjects, setDirtyProjects] = useState<Set<string>>(() => new Set());
  const [initialSettingsTab, setInitialSettingsTab] = useState("general");
  const [initialProjectFilter, setInitialProjectFilter] = useState("");

  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const computerApprovalsRef = useRef<Map<string, string>>(new Map());
  const initRan = useRef(false);
  const lastClickedIndex = useRef(-1);
  const gitSigRef = useRef("");
  const runningIdsRef = useRef<Set<string>>(runningIds);
  useEffect(() => {
    runningIdsRef.current = runningIds;
  }, [runningIds]);

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

  // With an open conversation this flips that session's own mode; on a new
  // chat (no active session) it sets the default new sessions are seeded with (#171).
  const setAutoMode = useCallback((enabled: boolean) => {
    const id = activeIdRef.current;
    if (id) setAutoModes((prev) => ({ ...prev, [id]: enabled }));
    else {
      setAutoModeState(enabled);
      localStorage.setItem(AUTO_MODE_KEY, String(enabled));
    }
  }, []);

  const setMaxSessions = useCallback((n: number) => {
    const v = Number.isFinite(n) && n >= 1 ? Math.floor(n) : DEFAULT_MAX_SESSIONS;
    setMaxSessionsState(v);
    localStorage.setItem(MAX_SESSIONS_KEY, String(v));
  }, []);

  const dispatchTo = useCallback((id: string, action: ChatAction) => {
    setTranscripts((prev) => ({ ...prev, [id]: chatReducer(prev[id] ?? initialChatState, action) }));
  }, []);

  const clearTerminal = useCallback((id: string) => {
    clearTimeout(lastRunTimers.current.get(id));
    lastRunTimers.current.delete(id);
    setLastRun((prev) => {
      if (!(id in prev)) return prev;
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const recordTerminal = useCallback(
    (id: string, status: { label: string; error: boolean }, keepExisting = false) => {
      setLastRun((prev) => (keepExisting && prev[id] ? prev : { ...prev, [id]: status }));
      clearTimeout(lastRunTimers.current.get(id));
      lastRunTimers.current.set(
        id,
        setTimeout(() => clearTerminal(id), LAST_RUN_TTL_MS),
      );
    },
    [clearTerminal],
  );

  useEffect(() => {
    const unlisten = listen<{ sessionId: string; callId: string; status: "approved" | "denied" }>(
      "approval-resolved",
      (e) =>
        dispatchTo(e.payload.sessionId, {
          type: "setApproval",
          callId: e.payload.callId,
          status: e.payload.status,
        }),
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
    [populateModels],
  );

  const startGatewayThenModels = useCallback(
    async (force = false, restart = false) => {
      setStatus("Starting gateway...");
      try {
        await api.startGateway(force, restart);
      } catch (e) {
        console.error("start_gateway failed:", e);
      }
      setStatus("Ready");
      fetchModelsWithRetry();
    },
    [setStatus, fetchModelsWithRetry],
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
      const paths: Record<string, string> = {};
      for (const [name, p] of Object.entries(parsed?.paths ?? {})) {
        if (typeof p === "string" && p.trim()) paths[name] = p;
      }
      const groups: Record<string, string> = {};
      for (const [name, g] of Object.entries(parsed?.groups ?? {})) {
        if (typeof g === "string" && g.trim()) groups[name] = g;
      }
      const selected = new Set<string>();
      if (Array.isArray(parsed?.selected)) {
        for (const n of parsed.selected) {
          if (typeof n === "string" && names.has(n)) selected.add(n);
        }
      }
      setProjects(map);
      setProjectNames(Array.from(names));
      setProjectContexts(contexts);
      setProjectPaths(paths);
      setProjectGroups(groups);
      setInitSelection(selected);
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
    async (force = false, restart = false) => {
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
                  : "Downloading infer...",
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
              startGatewayThenModels(force, restart).then(() => checkForUpdates(true));
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
    [setStatus, setError, startGatewayThenModels, checkForUpdates, refreshConversations],
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
      await initBackend(force, true);
    },
    [runningIds, setStatus, initBackend],
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
    api
      .readHistory()
      .then(setHistory)
      .catch(() => {});
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
    (id: string) => transcripts[id]?.items.some((it) => it.kind === "approval" && it.status === "pending") ?? false,
    [transcripts],
  );

  const openConversation = useCallback(
    async (id: string) => {
      setActiveId(id);
      activeIdRef.current = id;
      setActiveProject(projects[id] ?? null);
      if (transcripts[id]) return;
      try {
        const cwd = conversations.find((c) => c.id === id)?.project ?? undefined;
        const ndjson = await api.getConversation(id, cwd);
        dispatchTo(id, { type: "loadHistory", ndjson });
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Failed to load conversation: ${err}` });
      }
    },
    [transcripts, projects, conversations, dispatchTo],
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
        const cwd = conversations.find((c) => c.id === id)?.project ?? undefined;
        await api.deleteConversation(id, cwd);
        setTranscripts((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });
        clearTerminal(id);
        if (id === activeId) newChat();
        await refreshConversations();
      } catch (err) {
        setError(`Failed to delete conversation: ${err}`);
      }
    },
    [runningIds, activeId, conversations, newChat, refreshConversations, setError, clearTerminal],
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
    [displayConversations, projects, projectNames, selected, clearSelection, openConversation],
  );

  const bulkDelete = useCallback(() => {
    const ids = Array.from(selected);
    clearSelection();
    Promise.all(ids.map((id) => deleteConversation(id)));
  }, [selected, clearSelection, deleteConversation]);

  const fetchGitProjects = useCallback(async () => {
    const status = await api.gitProjectStatus();
    setGitProjects(new Set(status.git));
    setDirtyProjects(new Set(status.dirty));
  }, []);

  useEffect(() => {
    if (!projectsLoaded) return;
    // ponytail: only rescan git when a git-relevant field changed - a checkbox toggle persists `selected` without a full `git status` sweep.
    const sig = JSON.stringify([projectNames, projectPaths, projectGroups]);
    const gitChanged = sig !== gitSigRef.current;
    gitSigRef.current = sig;
    api
      .writeProjects(
        JSON.stringify({
          assignments: projects,
          names: projectNames,
          contexts: projectContexts,
          paths: projectPaths,
          groups: projectGroups,
          selected: Array.from(initSelection),
        }),
      )
      .then(() => (gitChanged ? fetchGitProjects() : undefined))
      .catch(() => {});
  }, [
    projectsLoaded,
    projects,
    projectNames,
    projectContexts,
    projectPaths,
    projectGroups,
    initSelection,
    fetchGitProjects,
  ]);

  const refreshGitProjects = useCallback(() => fetchGitProjects().catch(() => {}), [fetchGitProjects]);

  const assignProject = useCallback((sessionId: string, projectName: string) => {
    setProjects((prev) => ({ ...prev, [sessionId]: projectName }));
    setProjectNames((prev) => (prev.includes(projectName) ? prev : [...prev, projectName]));
  }, []);

  const sendPrompt = useCallback(
    async (runId: string, text: string, projectName?: string, extraInstruction?: string) => {
      if (runningIds.has(runId)) return;
      if (!model) {
        setError("Please select a model first");
        return;
      }
      setStatusErr(false);
      dispatchTo(runId, { type: "userSend", text });
      clearTerminal(runId);
      setRunningIds((prev) => new Set(prev).add(runId));
      try {
        const ch = new Channel<AgentEvent>();
        ch.onmessage = (event) => {
          dispatchTo(runId, { type: "event", event });
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
              if (COMPUTER_USE_TOOLS.has(event.tool_name)) {
                computerApprovalsRef.current.set(event.tool_call_id, runId);
              }
              if (!document.hasFocus()) notifyApproval(event.tool_name);
              break;
            case "Done":
              setRunningIds((prev) => {
                const next = new Set(prev);
                next.delete(runId);
                return next;
              });
              for (const [callId, sessionId] of computerApprovalsRef.current) {
                if (sessionId === runId) computerApprovalsRef.current.delete(callId);
              }
              recordTerminal(
                runId,
                {
                  label: event.exit_code === 0 ? "Done" : `Exited with code ${event.exit_code}`,
                  error: event.exit_code !== 0,
                },
                true,
              );
              break;
            case "Cancelled":
              setRunningIds((prev) => {
                const next = new Set(prev);
                next.delete(runId);
                return next;
              });
              for (const [callId, sessionId] of computerApprovalsRef.current) {
                if (sessionId === runId) computerApprovalsRef.current.delete(callId);
              }
              recordTerminal(runId, { label: "Stopped", error: false });
              break;
          }
        };
        const isInit = /^\/init(\s|$)/.test(text);
        if (!(runId in autoModes)) setAutoModes((p) => ({ ...p, [runId]: isInit || autoMode }));
        const cfg = await api.getConfig();
        const projectContext = projectName ? projectContexts[projectName] : undefined;
        const projectGroup =
          projectName && projectGroups[projectName]
            ? `This chat's project "${projectName}" belongs to the project group "${projectGroups[projectName]}".`
            : undefined;
        await api.sendMessage({
          prompt: text,
          model,
          sessionId: runId,
          onEvent: ch,
          systemPrompt: cfg.system_prompt || undefined,
          extraInstructions:
            [cfg.extra_instructions, projectGroup, projectContext, extraInstruction].filter(Boolean).join("\n\n") ||
            undefined,
          autoMode: isInit || (autoModes[runId] ?? autoMode),
          project: projectName,
        });
        refreshConversations();
        loadProjects();
      } catch (err) {
        dispatchTo(runId, { type: "error", text: `Error: ${err}` });
        dispatchTo(runId, { type: "event", event: { kind: "Done", exit_code: -1, stderr: "" } });
        setRunningIds((prev) => {
          const next = new Set(prev);
          next.delete(runId);
          return next;
        });
        recordTerminal(runId, { label: "Error", error: true });
      }
    },
    [
      runningIds,
      model,
      autoMode,
      autoModes,
      projectContexts,
      projectGroups,
      setError,
      refreshConversations,
      loadProjects,
      dispatchTo,
      clearTerminal,
      recordTerminal,
    ],
  );

  const runOnProjects = useCallback(
    async (names: string[], runOne: (name: string) => Promise<void>) => {
      setInitAllRunning(true);
      try {
        const skipped: string[] = [];
        const inFlight = new Set<Promise<void>>();
        for (const name of names) {
          const dirOk = await api.projectDirExists(name).catch(() => false);
          const busy = Object.entries(projects).some(([id, p]) => p === name && runningIdsRef.current.has(id));
          if (!dirOk || busy) {
            skipped.push(busy ? `${name} (busy)` : `${name} (missing folder)`);
            continue;
          }
          while (inFlight.size >= maxSessions) await Promise.race(inFlight);
          const run = runOne(name).catch(() => {});
          inFlight.add(run);
          void run.finally(() => inFlight.delete(run));
        }
        await Promise.all(inFlight);
        if (skipped.length) setStatus(`Skipped: ${skipped.join(", ")}`);
      } finally {
        setInitAllRunning(false);
      }
    },
    [projects, maxSessions, setStatus],
  );

  const broadcastPrompt = useCallback(
    async (names: string[], text: string) => {
      const isInit = /^\/init(\s|$)/.test(text);
      let first = true;
      await runOnProjects(names, async (name) => {
        const runId = crypto.randomUUID();
        assignProject(runId, name);
        if (first) {
          // Surface the first spawned session so its progress/approvals are visible; the rest run in the sidebar.
          first = false;
          setActiveProject(name);
          setActiveId(runId);
          activeIdRef.current = runId;
        }
        const extra = isInit && gitProjects.has(name) ? INIT_PR_INSTRUCTION : undefined;
        await sendPrompt(runId, text, name, extra);
      });
    },
    [runOnProjects, assignProject, gitProjects, sendPrompt],
  );

  useEffect(() => {
    const unlisten = listen<{ sessionId: string; text: string }>("monitor-send", (e) =>
      sendPrompt(e.payload.sessionId, e.payload.text, projects[e.payload.sessionId]),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [sendPrompt, projects]);

  const send = useCallback(async () => {
    const el = composerRef.current;
    const text = el?.value.trim() ?? "";
    if (!text) return;
    if (initSelecting) {
      if (initSelection.size === 0) {
        setError("Select at least one project to broadcast to");
        return;
      }
      if (!model) {
        setError("Please select a model first");
        return;
      }
      const names = Array.from(initSelection);
      if (el) {
        el.value = "";
        autoGrow(el);
      }
      api.appendHistory(text).catch(() => {});
      setHistory((h) => [...h, text]);
      setInitSelecting(false);
      setInitSelection(new Set());
      await broadcastPrompt(names, text);
      return;
    }
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
  }, [
    activeId,
    runningIds,
    model,
    maxSessions,
    activeProject,
    assignProject,
    projects,
    sendPrompt,
    setError,
    initSelecting,
    initSelection,
    broadcastPrompt,
  ]);

  const cancel = useCallback(async () => {
    if (!activeId || !runningIds.has(activeId)) return;
    try {
      await api.cancelAgent(activeId);
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  }, [activeId, runningIds]);

  const approve = useCallback(
    async (callId: string, approved: boolean, scope?: "always") => {
      const id = activeIdRef.current;
      if (!id) return;
      try {
        const computerApproval = computerApprovalsRef.current.get(callId) === id;
        if (computerApproval) {
          computerApprovalsRef.current.delete(callId);
          if (approved) await setMonitorVisible(false).catch(() => {});
        }
        await api.sendApproval(id, callId, approved, scope);
        const status = approved ? "approved" : "denied";
        dispatchTo(id, { type: "setApproval", callId, status });
        emit("approval-resolved", { sessionId: id, callId, status }).catch(() => {});
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Approval failed: ${err}` });
      }
    },
    [dispatchTo],
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

  const updateSnippet = useCallback((id: string, patch: { label?: string; prompt?: string }) => {
    setSnippetsState((prev) => {
      const next = prev.map((s) => (s.id === id ? { ...s, ...patch } : s));
      saveSnippets(next);
      return next;
    });
  }, []);

  const resetAllSnippets = useCallback(() => {
    setSnippetsState(DEFAULT_SNIPPETS);
    saveSnippets(DEFAULT_SNIPPETS);
  }, []);

  const reloadDesktopData = useCallback(() => {
    return api.readDesktopData().then((data) => {
      if (data.snippets.length > 0) {
        const next = mergeSnippets(data.snippets);
        setSnippetsState(next);
        saveSnippets(next);
      }
      hydrateRegistry(data.skills_registry_url);
      return data;
    });
  }, []);

  useEffect(() => {
    void reloadDesktopData().catch(() => {});
  }, [reloadDesktopData]);

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
        startGatewayThenModels(false, true);
      } catch (err) {
        setError(`Failed to save settings: ${err}`);
      }
    },
    [startGatewayThenModels, setError],
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
    api.createProjectDir(name).catch((e) => console.error("Failed to create project directory:", e));
  }, []);

  const importProjects = useCallback(
    (repos: { name: string; path: string; group?: string; context?: string | null }[]) => {
      setProjectNames((prev) => {
        const fresh = repos.map((r) => r.name).filter((n) => !prev.includes(n));
        return fresh.length ? [...prev, ...fresh] : prev;
      });
      setProjectPaths((prev) => {
        const next = { ...prev };
        for (const r of repos) next[r.name] ??= r.path;
        return next;
      });
      setProjectContexts((prev) => {
        const next = { ...prev };
        for (const r of repos) if (r.context) next[r.name] ??= r.context;
        return next;
      });
      setProjectGroups((prev) => {
        const next = { ...prev };
        for (const r of repos) if (r.group) next[r.name] ??= r.group;
        return next;
      });
    },
    [],
  );

  const deleteProjects = useCallback((names: string[]) => {
    const gone = new Set(names);
    setProjects((prev) => Object.fromEntries(Object.entries(prev).filter(([, p]) => !gone.has(p))));
    setProjectNames((prev) => prev.filter((n) => !gone.has(n)));
    setProjectContexts((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setProjectPaths((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setProjectGroups((prev) => {
      const next = { ...prev };
      for (const name of gone) delete next[name];
      return next;
    });
    setActiveProject((p) => (p && gone.has(p) ? null : p));
  }, []);

  const deleteProject = useCallback((name: string) => deleteProjects([name]), [deleteProjects]);

  const renameProject = useCallback((oldName: string, newName: string) => {
    setProjects((prev) => Object.fromEntries(Object.entries(prev).map(([id, p]) => [id, p === oldName ? newName : p])));
    setProjectNames((prev) => prev.map((n) => (n === oldName ? newName : n)));
    setProjectContexts((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setProjectPaths((prev) => {
      if (!(oldName in prev)) return prev;
      const next = { ...prev };
      next[newName] = next[oldName];
      delete next[oldName];
      return next;
    });
    setProjectGroups((prev) => {
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

  const initProject = useCallback(
    async (name: string) => {
      if (runningIdsRef.current.size >= maxSessions) {
        setError(`Max ${maxSessions} concurrent sessions reached - stop one to start another`);
        return;
      }
      const runId = crypto.randomUUID();
      assignProject(runId, name);
      setActiveProject(name);
      setActiveId(runId);
      activeIdRef.current = runId;
      const pr = gitProjects.has(name) ? INIT_PR_INSTRUCTION : undefined;
      await sendPrompt(runId, "/init", name, pr);
      await loadProjects();
      const ctx = await api.refreshProjectContext(name).catch(() => null);
      if (ctx) setProjectContext(name, ctx);
    },
    [gitProjects, maxSessions, assignProject, sendPrompt, loadProjects, setProjectContext, setError],
  );

  const startInitSelection = useCallback(() => {
    setInitSelecting(true);
    setInitSelection((prev) => (prev.size === 0 ? new Set(projectNames) : prev));
  }, [projectNames]);

  const cancelInitSelection = useCallback(() => {
    setInitSelecting(false);
    setInitSelection(new Set());
  }, []);

  const toggleInitSelection = useCallback((name: string) => {
    setInitSelection((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const selectAllProjects = useCallback(() => setInitSelection(new Set(projectNames)), [projectNames]);

  const clearProjectSelection = useCallback(() => setInitSelection(new Set()), []);

  const selectProjectsInGroup = useCallback(
    (group: string) => {
      setInitSelection((prev) => {
        const next = new Set(prev);
        const inGroup = projectNames.filter((n) => projectGroups[n] === group);
        const allIn = inGroup.length > 0 && inGroup.every((n) => next.has(n));
        for (const n of inGroup) {
          if (allIn) next.delete(n);
          else next.add(n);
        }
        return next;
      });
    },
    [projectNames, projectGroups],
  );

  const initAllProjects = useCallback(
    (names: string[]) => runOnProjects(names, initProject),
    [runOnProjects, initProject],
  );

  const setProjectPath = useCallback((name: string, path: string) => {
    setProjectPaths((prev) => {
      if (!path.trim()) {
        const next = { ...prev };
        delete next[name];
        return next;
      }
      return { ...prev, [name]: path };
    });
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
  const activeAutoMode = (activeId != null ? autoModes[activeId] : undefined) ?? autoMode;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const shortcut = matchShortcut({
        key: e.key,
        metaKey: e.metaKey,
        ctrlKey: e.ctrlKey,
        shiftKey: e.shiftKey,
        altKey: e.altKey,
        repeat: e.repeat,
        defaultPrevented: e.defaultPrevented,
        inComposer: e.target === composerRef.current,
      });
      if (shortcut === "newChat") {
        e.preventDefault();
        newChat();
      } else if (shortcut === "cancel" && running) {
        e.preventDefault();
        void cancel();
      } else if (shortcut === "autoModeToggle") {
        e.preventDefault();
        setAutoMode(!activeAutoMode);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [running, cancel, newChat, activeAutoMode, setAutoMode]);

  const runLabel = useCallback(
    (id: string): { label: string; error: boolean } | null => {
      const chat = transcripts[id];
      if (chat?.items.some((it) => it.kind === "approval" && it.status === "pending")) {
        return { label: "Awaiting approval...", error: false };
      }
      if (runningIds.has(id)) {
        if (chat?.currentReasoningId) return { label: "Thinking...", error: false };
        const tool = [...(chat?.items ?? [])].reverse().find((it) => it.kind === "tool" && it.state === "running");
        return {
          label: tool?.kind === "tool" ? `Running ${tool.name}...` : "Running...",
          error: false,
        };
      }
      return lastRun[id] ?? null;
    },
    [transcripts, runningIds, lastRun],
  );

  const delegations = useCallback(
    (id: string): Delegation[] => delegationsFrom(transcripts[id]?.items ?? []),
    [transcripts],
  );

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
    autoMode: activeAutoMode,
    setAutoMode,
    maxSessions,
    setMaxSessions,
    conversations: displayConversations,
    selected,
    sessionId: activeId,
    isRunning,
    isAwaitingApproval,
    runLabel,
    delegations,
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
    reloadDesktopData,
    loadProjects,
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
    projectPaths,
    setProjectPath,
    projectGroups,
    initialSettingsTab,
    setInitialSettingsTab,
    initialProjectFilter,
    setInitialProjectFilter,
    assignProject,
    unassignProject,
    createProject,
    importProjects,
    gitProjects,
    dirtyProjects,
    refreshGitProjects,
    deleteProject,
    deleteProjects,
    renameProject,
    toggleCollapseProject,
    initProject,
    initAllProjects,
    broadcastPrompt,
    initAllRunning,
    initSelecting,
    initSelection,
    startInitSelection,
    cancelInitSelection,
    toggleInitSelection,
    selectAllProjects,
    clearProjectSelection,
    selectProjectsInGroup,
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
