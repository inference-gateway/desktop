import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  api,
  Channel,
  type AgentEvent,
  type Conversation,
  type DesktopConfig,
  type MarkdownAgent,
  type ProgressEvent,
  type UserQuestionAnswer,
} from "@/lib/tauri";
import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import {
  chatReducer,
  delegationsFrom,
  initialChatState,
  pendingInput,
  COMPUTER_USE_TOOLS,
  autoApproves,
  type ChatAction,
  type ChatState,
  type Delegation,
  type TodoItem,
} from "@/lib/transcript";
import { chatWorkspace, moveHops, sidebarConversations, THIS_COMPUTER } from "@/lib/computers";
import { workspaceModel } from "@/lib/workspace-model";
import { autoGrow } from "@/lib/textarea";
import { matchShortcut } from "@/lib/shortcuts";
import { modeAfterPlan, nextMode, parseMode, type AgentMode, type PlanAction } from "@/lib/agent-mode";
import { isBashCommand } from "@/lib/tools";
import { agentDefaultProject, agentInstructionsPreamble, agentPinsModel, agentThreadModel } from "@/lib/agents";
import { enqueue, takeAll, takeFirst, type PromptQueue } from "@/lib/queue";
import { ActivityProvider, useActivity } from "@/stores/activity";
import { SnippetsProvider } from "@/stores/snippets";
import { StatusProvider, useStatusActions } from "@/stores/status";
import { BackendProvider, MODEL_STORAGE_KEY, useBackend } from "@/stores/backend";
import { GitProvider, useGit } from "@/stores/git";
import { SidebarProvider, useSidebar } from "@/stores/sidebar";
import { AgentsProvider, useAgents } from "@/stores/agents";

const AGENT_MODE_KEY = "agentMode";
const AWAITING_LABELS = {
  approval: "Awaiting approval...",
  question: "Awaiting answer...",
  plan: "Awaiting plan review...",
} as const;
import type { ProjectType } from "@/lib/sidebar-document";
import { WINDOW_EVENTS } from "@/lib/events";
export type { ProjectType };
/** What the sidebar lists: every project, one project type, or the agents. */
export type SidebarFilter = ProjectType | "agents";

const CONTENT_GUIDANCE =
  'This is a content project, not a code repository. To add your voice to a video ("add my voice", "put a voiceover on this recording") to add captions or subtitles over it ("add captions", "caption this"), or to add title cards, lower thirds, callouts, step counters or charts over it, read and follow ~/.infer/skills/video-editing/SKILL.md, which is already installed; use only the tools and paths it names and never search the filesystem for tools or skills. The desktop renders <stem>.timeline.json in the project directory as an editable timeline, and the media/ folder as its media pool: put every recording, music file and generated voice clip there and reference it as media/<file> in the timeline. Every audio and video file (wav, mp3, m4a, mp4, mov and the like) MUST live inside media/, never in the project root or any other folder; the only exceptions are the scratch files the skill keeps in tmp/ and a voice.wav cut from the recording. Treat media/ as append-only: never overwrite or delete an existing file in it. When you need to replace a clip, write a new versioned filename (for example media/<name>-v2.<ext>) and point the timeline at the new file, leaving the old one in place.';

const MAX_SESSIONS_KEY = "maxConcurrentSessions";
const DEFAULT_MAX_SESSIONS = 5;
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
  { label: "ElevenLabs", env: "ELEVENLABS_API_KEY" },
] as const;

const INIT_PR_INSTRUCTION =
  "After /init completes, open a pull request containing the AGENTS.md changes for human review - do not merge.";

const getConfig = () => api.getConfig();
const getAuth = () => api.getAuth();
const saveConfig = (cfg: DesktopConfig) => api.setConfig(cfg);

/** The value itself while its JSON is unchanged, so a list rebuilt on every
 * streamed event keeps its identity until its content really differs. */
// ponytail: stringifies on every change of the input - fine for the sidebar list and a handful of ids; compare by key if either grows large.
/** The previous object while every field still holds the same value. A context
 * value is built fresh on each render, and this keeps its readers from
 * re-rendering unless a field they could read has changed. */
function useUnchanged<T extends Record<string, unknown>>(value: T): T {
  // The object always has the same keys, so the dependency list keeps its length.
  return useMemo(() => value, Object.values(value));
}

function useSameContent<T>(value: T): T {
  const kept = useRef(value);
  const json = JSON.stringify(value);
  const keptJson = useRef(json);
  if (json !== keptJson.current) {
    kept.current = value;
    keptJson.current = json;
  }
  return kept.current;
}

function useDesktopStore() {
  const [transcripts, setTranscripts] = useState<Record<string, ChatState>>({});
  const transcriptsRef = useRef(transcripts);
  useEffect(() => {
    transcriptsRef.current = transcripts;
  }, [transcripts]);
  const [runningIds, setRunningIds] = useState<Set<string>>(() => new Set());
  const [initAllRunning, setInitAllRunning] = useState(false);
  const [initSelecting, setInitSelecting] = useState(false);
  const [activeId, setActiveId] = useState<string | null>(null);
  const activeIdRef = useRef<string | null>(null);
  const { setStatus, setError, hideError } = useStatusActions();
  const { ready, model, refreshStatus, servicesStarting, startGatewayThenModels, checkForUpdates, sessionStopper } =
    useBackend();
  const [lastRun, setLastRun] = useState<Record<string, { label: string; error: boolean }>>({});
  const lastRunTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const { logActivity } = useActivity();
  const { agents, activeAgent, setActiveAgent, assignAgent, showAgentEditor, storeAgent } = useAgents();
  const {
    projects,
    projectNames,
    activeProject,
    projectContexts,
    projectGroups,
    projectTypes,
    agentOf,
    workspaceOf,
    workspaceTitles,
    initSelection,
    loadProjects,
    setProjects,
    setProjectNames,
    setActiveProject,
    setProjectTypes,
    setWorkspaceOf,
    setWorkspaceTitles,
    setInitSelection,
    setProjectContext,
  } = useSidebar();
  const { gitProjects, refreshGitProjects, cleanupProject } = useGit();
  const [agentMode, setAgentModeState] = useState<AgentMode>(() => parseMode(localStorage.getItem(AGENT_MODE_KEY)));
  const [agentModes, setAgentModes] = useState<Record<string, AgentMode>>({});
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [maxSessions, setMaxSessionsState] = useState<number>(() => {
    const n = parseInt(localStorage.getItem(MAX_SESSIONS_KEY) || "", 10);
    return Number.isFinite(n) && n >= 1 ? n : DEFAULT_MAX_SESSIONS;
  });
  const [currentView, setCurrentView] = useState<"chat" | "settings" | "observability" | "computers">("chat");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [chatOpen, setChatOpen] = useState(true);
  const [history, setHistory] = useState<string[]>([]);
  const [todoDrafts, setTodoDrafts] = useState<Record<string, TodoItem[]>>({});
  const [queuedPrompts, setQueuedPrompts] = useState<PromptQueue>(() => ({}));
  const queuedRef = useRef<PromptQueue>({});
  const [showStatusBar, setShowStatusBar] = useState(true);
  const [projectFilter, setProjectFilter] = useState<SidebarFilter | null>(null);
  const visibleProjectNames = useMemo(
    () =>
      projectFilter && projectFilter !== "agents"
        ? projectNames.filter((n) => (projectTypes[n] ?? "code") === projectFilter)
        : projectNames,
    [projectFilter, projectNames, projectTypes],
  );
  const workspaceOfRef = useRef(workspaceOf);
  useEffect(() => {
    workspaceOfRef.current = workspaceOf;
  }, [workspaceOf]);
  const [moving, setMoving] = useState<Record<string, string>>(() => ({}));
  const movingRef = useRef(moving);
  useEffect(() => {
    movingRef.current = moving;
  }, [moving]);
  const [initialSettingsTab, setInitialSettingsTab] = useState("general");
  const [initialProjectFilter, setInitialProjectFilter] = useState("");

  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const computerApprovalsRef = useRef<Map<string, string>>(new Map());
  const initRan = useRef(false);
  const lastClickedIndex = useRef(-1);
  const runningIdsRef = useRef<Set<string>>(runningIds);
  useEffect(() => {
    runningIdsRef.current = runningIds;
  }, [runningIds]);
  const agentModesRef = useRef<Record<string, AgentMode>>(agentModes);
  useEffect(() => {
    agentModesRef.current = agentModes;
  }, [agentModes]);

  // Todo panel drafts are keyed by session so switching sessions never
  // leaks a user-edited list into another chat (#196); null clears it.
  const setTodoDraft = useCallback((key: string, items: TodoItem[] | null) => {
    setTodoDrafts((prev) => {
      if (items === null && !(key in prev)) return prev;
      const next = { ...prev };
      if (items === null) delete next[key];
      else next[key] = items;
      return next;
    });
  }, []);

  // Mirrors the entry the CLI records from the run_agent_input (trimmed, a
  // consecutive duplicate skipped) so arrow-up is fresh without a refetch.
  const recordHistory = useCallback((text: string) => {
    const entry = text.trim();
    if (!entry) return;
    setHistory((h) => (h[h.length - 1] === entry ? h : [...h, entry]));
  }, []);

  const queuePrompt = useCallback((id: string, text: string) => {
    const next = enqueue(queuedRef.current, id, text);
    queuedRef.current = next;
    setQueuedPrompts(next);
  }, []);

  const dropQueued = useCallback((id: string) => {
    if (!(id in queuedRef.current)) return;
    const next = { ...queuedRef.current };
    delete next[id];
    queuedRef.current = next;
    setQueuedPrompts(next);
  }, []);

  const clearQueued = useCallback((id: string): string | null => {
    const { queue, text } = takeFirst(queuedRef.current, id);
    if (text === null) return null;
    queuedRef.current = queue;
    setQueuedPrompts(queue);
    return text;
  }, []);

  const popQueued = useCallback((): string | null => {
    const id = activeIdRef.current;
    if (!id) return null;
    const { queue, text } = takeAll(queuedRef.current, id);
    if (text === null) return null;
    queuedRef.current = queue;
    setQueuedPrompts(queue);
    return text;
  }, []);

  const discardQueued = useCallback(() => {
    const id = activeIdRef.current;
    if (id) dropQueued(id);
  }, [dropQueued]);

  const setAgentMode = useCallback((mode: AgentMode) => {
    const id = activeIdRef.current;
    if (id) setAgentModes((prev) => ({ ...prev, [id]: mode }));
    else {
      setAgentModeState(mode);
      localStorage.setItem(AGENT_MODE_KEY, mode);
    }
  }, []);

  const setMaxSessions = useCallback((n: number) => {
    const v = Number.isFinite(n) && n >= 1 ? Math.floor(n) : DEFAULT_MAX_SESSIONS;
    setMaxSessionsState(v);
    localStorage.setItem(MAX_SESSIONS_KEY, String(v));
  }, []);

  const dispatchTo = useCallback((id: string, action: ChatAction) => {
    const now = Date.now();
    setTranscripts((prev) => ({ ...prev, [id]: chatReducer(prev[id] ?? initialChatState, action, now) }));
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
      WINDOW_EVENTS.approvalResolved,
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

  // Cancels every running session and forgets them, before a backend the
  // chats are attached to is restarted or swapped.
  const cancelRunningAgents = useCallback(async () => {
    for (const id of runningIds) {
      try {
        await api.cancelAgent(id);
      } catch (e) {
        console.error("Cancel failed:", e);
      }
    }
    setRunningIds(new Set());
  }, [runningIds]);

  useEffect(() => {
    sessionStopper.current = cancelRunningAgents;
  }, [sessionStopper, cancelRunningAgents]);

  useEffect(() => {
    if (ready) refreshConversations();
  }, [ready, refreshConversations]);

  useEffect(() => {
    if (initRan.current) return;
    initRan.current = true;

    api
      .getConfig()
      .then((cfg) => {
        setShowStatusBar(cfg.status_bar_enabled);
        if (cfg.default_model && !localStorage.getItem(MODEL_STORAGE_KEY)) {
          localStorage.setItem(MODEL_STORAGE_KEY, cfg.default_model);
        }
      })
      .catch(() => {});
  }, []);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    lastClickedIndex.current = -1;
  }, []);

  const displayConversations = useSameContent(
    useMemo(
      () => sidebarConversations(conversations, runningIds, workspaceOf, workspaceTitles, transcripts),
      [conversations, runningIds, workspaceOf, workspaceTitles, transcripts],
    ),
  );

  const isRunning = useCallback((id: string) => runningIds.has(id), [runningIds]);

  // The CLI lets one process record at a time, so at most one session owns it.
  const recordingOwner = useMemo(
    () => Object.keys(transcripts).find((id) => transcripts[id]?.recording) ?? null,
    [transcripts],
  );

  const awaitingApproval = useSameContent(
    useMemo(
      () => Object.keys(transcripts).filter((id) => pendingInput(transcripts[id]?.items ?? []) != null),
      [transcripts],
    ),
  );
  const isAwaitingApproval = useCallback((id: string) => awaitingApproval.includes(id), [awaitingApproval]);

  // The system prompt and custom instructions a chat's thread launches with:
  // the configured ones plus the project's group, type and context.
  const threadPrompts = useCallback(
    async (projectName?: string, extraInstruction?: string, agentName?: string) => {
      const cfg = await api.getConfig();
      const agent = agentName ? agents.find((a) => a.name === agentName) : undefined;
      const agentPreamble = agent ? agentInstructionsPreamble(agent, agents) : undefined;
      const projectContext = projectName ? projectContexts[projectName] : undefined;
      const projectGroup =
        projectName && projectGroups[projectName]
          ? `This chat's project "${projectName}" belongs to the project group "${projectGroups[projectName]}".`
          : undefined;
      const projectType = projectName && projectTypes[projectName] === "content" ? CONTENT_GUIDANCE : undefined;
      return {
        systemPrompt: agent ? undefined : cfg.system_prompt || undefined,
        extraInstructions:
          [cfg.extra_instructions, agentPreamble, projectGroup, projectType, projectContext, extraInstruction]
            .filter(Boolean)
            .join("\n\n") || undefined,
      };
    },
    [agents, projectContexts, projectGroups, projectTypes],
  );

  const resolveApproval = useCallback(
    async (id: string, callId: string, approved: boolean, _scope?: "always") => {
      try {
        await api.sendApproval(id, callId, approved);
        const status = approved ? "approved" : "denied";
        dispatchTo(id, { type: "setApproval", callId, status });
        emit(WINDOW_EVENTS.approvalResolved, { sessionId: id, callId, status }).catch(() => {});
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Approval failed: ${err}` });
      }
    },
    [dispatchTo],
  );

  const chatEvents = useCallback(
    (runId: string, initProject?: string) => {
      const ch = new Channel<AgentEvent>();
      ch.onmessage = (event) => {
        if (event.kind === "NextStepSuggestion" && runId !== activeIdRef.current) return;
        dispatchTo(runId, { type: "event", event });
        switch (event.kind) {
          case "RunStarted":
            setRunningIds((prev) => (prev.has(runId) ? prev : new Set(prev).add(runId)));
            break;
          case "AssistantMessage":
            if (event.tool_calls.some((tc) => tc.name === "Agent")) setTimeout(refreshConversations, 1000);
            break;
          case "BackgroundTasks":
            refreshConversations();
            break;
          case "ApprovalRequest":
            if (agentModesRef.current[runId] === "auto" && autoApproves(event.tool_name)) {
              resolveApproval(runId, event.tool_call_id, true);
              break;
            }
            if (COMPUTER_USE_TOOLS.has(event.tool_name)) {
              computerApprovalsRef.current.set(event.tool_call_id, runId);
            }
            if (!document.hasFocus()) notifyApproval(event.tool_name);
            break;
          case "Done":
            refreshStatus();
            refreshConversations();
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
            if (initProject)
              logActivity(
                initProject,
                "Init",
                event.exit_code === 0 ? "done" : "failed",
                event.exit_code === 0 ? "AGENTS.md ready" : `Exited with code ${event.exit_code}`,
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
            if (initProject) logActivity(initProject, "Init", "skipped", "stopped");
            break;
        }
      };
      return ch;
    },
    [dispatchTo, refreshStatus, refreshConversations, resolveApproval, recordTerminal, logActivity],
  );

  const followThread = useCallback(
    async (id: string) => {
      const projectName = projects[id];
      const agentName = agentOf[id];
      return api.openThread({
        sessionId: id,
        model: agentThreadModel(
          agents.find((a) => a.name === agentName),
          model,
        ),
        onEvent: chatEvents(id),
        ...(await threadPrompts(projectName, undefined, agentName)),
        mode: agentModes[id] ?? agentMode,
        project: projectName,
        agent: agentName,
      });
    },
    [projects, agentOf, agents, model, agentModes, agentMode, threadPrompts, chatEvents],
  );

  const leaveConversation = useCallback(
    (nextId: string | null) => {
      const previous = activeIdRef.current;
      if (previous && previous !== nextId) dispatchTo(previous, { type: "dismissSuggestion" });
      if (!previous || previous === nextId || runningIdsRef.current.has(previous)) return;
      api.closeThread(previous).catch((e) => console.error("Close thread failed:", e));
    },
    [dispatchTo],
  );

  const dismissSuggestion = useCallback(() => {
    if (activeIdRef.current) dispatchTo(activeIdRef.current, { type: "dismissSuggestion" });
  }, [dispatchTo]);

  const openConversation = useCallback(
    async (id: string) => {
      leaveConversation(id);
      setActiveId(id);
      activeIdRef.current = id;
      setActiveProject(projects[id] ?? null);
      setActiveAgent(agentOf[id] ?? null);
      if (transcriptsRef.current[id]) return;
      try {
        dispatchTo(id, { type: "loadHistory", messages: await followThread(id) });
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Failed to load conversation: ${err}` });
      }
    },
    [projects, agentOf, followThread, dispatchTo, leaveConversation],
  );

  const newChat = useCallback(() => {
    leaveConversation(null);
    setActiveId(null);
    activeIdRef.current = null;
    clearSelection();
    composerRef.current?.focus();
  }, [clearSelection, composerRef, leaveConversation]);

  const selectProject = useCallback(
    (name: string | null) => {
      const current = activeIdRef.current;
      if (name && current && projects[current] !== name) newChat();
      if (name) setActiveAgent(null);
      setActiveProject(name);
    },
    [projects, newChat],
  );

  // Picking an agent works like picking a project: the next message opens a
  // chat with it, unless the open chat is already one of its own. The chat
  // opens in the agent's default project until another is picked for it.
  const selectAgent = useCallback(
    (
      name: string | null,
      project = agentDefaultProject(
        agents.find((a) => a.name === name),
        projectNames,
      ),
    ) => {
      const current = activeIdRef.current;
      const ownChatOpen = current !== null && agentOf[current] === name;
      if (name && !ownChatOpen) {
        if (current) newChat();
        setActiveProject(project);
      }
      setActiveAgent(name);
    },
    [agentOf, agents, projectNames, newChat],
  );

  const deleteConversation = useCallback(
    async (id: string) => {
      dropQueued(id);
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
        setWorkspaceOf(({ [id]: _, ...rest }) => rest);
        setWorkspaceTitles(({ [id]: _, ...rest }) => rest);
        if (id === activeId) newChat();
        await refreshConversations();
      } catch (err) {
        setError(`Failed to delete conversation: ${err}`);
      }
    },
    [runningIds, activeId, conversations, newChat, refreshConversations, setError, clearTerminal, dropQueued],
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

  const moveConversationStore = useCallback(
    (sessionId: string, projectName?: string) => {
      const fromCwd = conversations.find((c) => c.id === sessionId)?.project ?? undefined;
      api
        .moveConversation(sessionId, fromCwd, projectName)
        .then(refreshConversations)
        .catch((e) => setError(`Failed to move conversation: ${e}`));
    },
    [conversations, refreshConversations, setError],
  );

  const assignProject = useCallback(
    (sessionId: string, projectName: string) => {
      setProjects((prev) => ({ ...prev, [sessionId]: projectName }));
      setProjectNames((prev) => (prev.includes(projectName) ? prev : [...prev, projectName]));
      moveConversationStore(sessionId, projectName);
    },
    [moveConversationStore],
  );

  const openAgentEditor = useCallback(
    (agent: MarkdownAgent | null) => {
      setCurrentView("chat");
      showAgentEditor(agent);
    },
    [showAgentEditor],
  );

  // Saving lands the user in a fresh chat with the agent, Grok Bot style.
  const saveAgent = useCallback(
    async (agent: MarkdownAgent, previousName?: string, avatarUrl?: string) => {
      await storeAgent(agent, previousName, avatarUrl);
      setProjectFilter("agents");
      selectAgent(agent.name, agentDefaultProject(agent, projectNames));
      composerRef.current?.focus();
    },
    [storeAgent, selectAgent, projectNames],
  );

  // A chat keeps its agent: a send into an existing chat (a queued prompt, the
  // monitor) runs as the agent it was assigned. Only a chat whose assignment
  // has not landed in state yet needs its agent passed in.
  const sendPrompt = useCallback(
    async (runId: string, text: string, projectName?: string, extraInstruction?: string, newChatAgent?: string) => {
      const agentName = newChatAgent ?? agentOf[runId];
      if (runningIds.has(runId) || runningIdsRef.current.has(runId)) return;
      if (!model && !isBashCommand(text)) {
        setError("Please select a model first");
        return;
      }
      runningIdsRef.current = new Set(runningIdsRef.current).add(runId);
      const isInit = /^\/init(\s|$)/.test(text);
      hideError();
      dispatchTo(runId, { type: "userSend", text });
      clearTerminal(runId);
      setRunningIds((prev) => new Set(prev).add(runId));
      if (isInit && projectName) logActivity(projectName, "Init", "running");
      try {
        const ch = chatEvents(runId, isInit ? projectName : undefined);
        const mode = isInit ? "auto" : (agentModes[runId] ?? agentMode);
        if (!(runId in agentModes)) setAgentModes((p) => ({ ...p, [runId]: mode }));
        const runsOn =
          runId in workspaceOfRef.current
            ? workspaceOfRef.current[runId]
            : agents.find((a) => a.name === agentName)?.workspace || THIS_COMPUTER;
        if (runsOn && !(runId in workspaceOfRef.current)) {
          setWorkspaceOf((prev) => ({ ...prev, [runId]: runsOn }));
          setWorkspaceTitles((prev) => ({ ...prev, [runId]: text.slice(0, 200) }));
        }
        const workspaces = runsOn ? await api.workspaceConfigs().catch(() => []) : [];
        const agent = agents.find((a) => a.name === agentName);
        const onWorkspace = workspaceModel(
          agentThreadModel(agent, model),
          workspaces.find((workspace) => workspace.name === runsOn),
          agentPinsModel(agent),
        );
        if ("problem" in onWorkspace) throw onWorkspace.problem;
        if (onWorkspace.note) setStatus(onWorkspace.note);
        await api.sendMessage({
          prompt: text,
          model: onWorkspace.model,
          sessionId: runId,
          onEvent: ch,
          ...(await threadPrompts(projectName, extraInstruction, agentName)),
          mode,
          project: projectName,
          workspace: runsOn || undefined,
          agent: agentName,
        });
        refreshConversations();
        loadProjects();
      } catch (err) {
        dispatchTo(runId, { type: "error", text: `Error: ${err}` });
        if (isInit && projectName) logActivity(projectName, "Init", "failed", String(err));
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
      agents,
      agentOf,
      agentMode,
      agentModes,
      threadPrompts,
      setError,
      logActivity,
      refreshConversations,
      loadProjects,
      dispatchTo,
      clearTerminal,
      recordTerminal,
      chatEvents,
    ],
  );

  const prevRunningRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const prev = prevRunningRef.current;
    prevRunningRef.current = runningIds;
    if (prev === runningIds) return;
    for (const id of prev) {
      if (runningIds.has(id)) continue;
      const text = clearQueued(id);
      if (text) void sendPrompt(id, text, projects[id]);
    }
  }, [runningIds, sendPrompt, projects, clearQueued]);

  const runOnProjects = useCallback(
    async (names: string[], runOne: (name: string) => Promise<void>, action?: string) => {
      setInitAllRunning(true);
      try {
        const skipped: string[] = [];
        const inFlight = new Set<Promise<void>>();
        for (const name of names) {
          const dirOk = await api.projectDirExists(name).catch(() => false);
          const busy = Object.entries(projects).some(([id, p]) => p === name && runningIdsRef.current.has(id));
          if (!dirOk || busy) {
            const reason = busy ? "busy" : "missing folder";
            skipped.push(`${name} (${reason})`);
            if (action) logActivity(name, action, "skipped", reason);
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
    [projects, maxSessions, setStatus, logActivity],
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
    const unlisten = listen<{ sessionId: string; text: string }>(WINDOW_EVENTS.monitorSend, (e) =>
      sendPrompt(e.payload.sessionId, e.payload.text, projects[e.payload.sessionId]),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [sendPrompt, projects]);

  // `!!ToolName(arg="value")` from the composer: parse, execute through the
  // CLI's own tool registry (`infer tools execute`) and render the result
  // exactly like an AI-initiated tool call. No model turn involved.
  // Send text through the normal message path (composer, todo panel hand-off).
  // Returns false when the send was rejected so callers keep their text.
  const sendText = useCallback(
    async (text: string): Promise<boolean> => {
      if (activeId && movingRef.current[activeId]) {
        setError("Wait for the chat to finish moving before you send");
        return false;
      }
      if (activeId && runningIds.has(activeId)) {
        recordHistory(text);
        queuePrompt(activeId, text);
        return true;
      }
      if (!model && !isBashCommand(text)) {
        setError("Please select a model first");
        return false;
      }
      if (!activeId && runningIds.size >= maxSessions) {
        setError(`Max ${maxSessions} concurrent sessions reached - stop one to start another`);
        return false;
      }
      const runId = activeId ?? crypto.randomUUID();
      if (!activeId && activeProject) assignProject(runId, activeProject);
      if (!activeId && activeAgent) assignAgent(runId, activeAgent);
      setActiveId(runId);
      activeIdRef.current = runId;
      recordHistory(text);
      await sendPrompt(
        runId,
        text,
        (activeId ? projects[activeId] : activeProject) ?? undefined,
        undefined,
        (activeId ? agentOf[activeId] : activeAgent) ?? undefined,
      );
      return true;
    },
    [
      activeId,
      runningIds,
      model,
      maxSessions,
      activeProject,
      activeAgent,
      assignProject,
      assignAgent,
      projects,
      agentOf,
      sendPrompt,
      setError,
      recordHistory,
      queuePrompt,
    ],
  );

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
      recordHistory(text);
      setInitSelecting(false);
      setInitSelection(new Set());
      await broadcastPrompt(names, text);
      return;
    }
    if (el) {
      el.value = "";
      autoGrow(el);
    }
    if (!(await sendText(text)) && el && !el.value) {
      el.value = text;
      autoGrow(el);
    }
  }, [initSelecting, initSelection, model, setError, broadcastPrompt, sendText, recordHistory]);

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
      if (computerApprovalsRef.current.get(callId) === id) {
        computerApprovalsRef.current.delete(callId);
        if (approved) await setMonitorVisible(false).catch(() => {});
      }
      await resolveApproval(id, callId, approved, scope);
    },
    [resolveApproval],
  );

  const answerQuestions = useCallback(
    async (callId: string, answers: UserQuestionAnswer[] | null) => {
      const id = activeIdRef.current;
      if (!id) return;
      try {
        await api.sendQuestionAnswers(id, callId, answers);
        dispatchTo(id, { type: "setQuestion", callId, answers });
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Answer failed: ${err}` });
      }
    },
    [dispatchTo],
  );

  const decidePlan = useCallback(
    async (planId: string, action: PlanAction) => {
      const id = activeIdRef.current;
      if (!id) return;
      try {
        await api.sendPlanDecision(id, planId, action);
        dispatchTo(id, { type: "setPlan", planId, action });
        const mode = modeAfterPlan(action);
        if (mode) setAgentModes((prev) => ({ ...prev, [id]: mode }));
      } catch (err) {
        dispatchTo(id, { type: "error", text: `Plan decision failed: ${err}` });
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

  const openSettings = useCallback(() => {
    checkForUpdates();
    setCurrentView("settings");
  }, [checkForUpdates]);

  const openObservability = useCallback(() => {
    setCurrentView("observability");
  }, []);

  const openComputers = useCallback(() => {
    setCurrentView("computers");
  }, []);

  const [drivenId, setDrivenId] = useState<string | null>(null);

  const handBack = useCallback(async () => {
    if (!drivenId) return;
    setDrivenId(null);
    const messages = await followThread(drivenId).catch(() => null);
    if (messages && !transcriptsRef.current[drivenId]) dispatchTo(drivenId, { type: "loadHistory", messages });
    await api
      .screenControl(drivenId, projects[drivenId], false)
      .catch((e) => setError(`Failed to hand the screen back: ${e}`));
  }, [drivenId, followThread, dispatchTo, projects, setError]);

  // A chat moves between turns: an open run stops first, and a chat on one
  // workspace comes home before it goes to another.
  const moveChat = useCallback(
    async (sessionId: string, target: string) => {
      const hops = moveHops(chatWorkspace(sessionId, workspaceOf, agentOf, agents), target);
      if (hops.length === 0) return;
      try {
        if (runningIds.has(sessionId)) await api.cancelAgent(sessionId);
        if (drivenId === sessionId) await handBack();
        const home = (await api.listWorkspaces()).find((w) => w.kind === "local")?.name ?? "local";
        for (const hop of hops) {
          const label = hop || "this computer";
          setMoving((prev) => ({ ...prev, [sessionId]: `Moving to ${label}` }));
          await api.moveSession(sessionId, projects[sessionId], hop || home, hop !== THIS_COMPUTER, (stage) =>
            setMoving((prev) => ({ ...prev, [sessionId]: `Moving to ${label}: ${stage}` })),
          );
          setWorkspaceOf((prev) => ({ ...prev, [sessionId]: hop }));
        }
        setStatus(`The chat now runs on ${target || "this computer"}`);
      } catch (e) {
        setError(`Failed to move the chat: ${e}`);
      } finally {
        setMoving((prev) => Object.fromEntries(Object.entries(prev).filter(([id]) => id !== sessionId)));
      }
    },
    [workspaceOf, agentOf, agents, runningIds, drivenId, handBack, projects, setStatus, setError],
  );

  // Removes the machine of every chat on a workspace. Their transcripts go with
  // the machines, so the chats start over on their agent's workspace or here.
  const cleanMachines = useCallback(async () => {
    const remote = Object.keys(workspaceOf).filter((id) => workspaceOf[id] !== THIS_COMPUTER);
    const removed = await api.cleanMachines(remote.map((id) => ({ id, project: projects[id] })));
    setWorkspaceOf((prev) => Object.fromEntries(Object.entries(prev).filter(([id]) => !remote.includes(id))));
    return removed;
  }, [workspaceOf, projects]);

  // Removes the machine of one chat, with the transcript that lives on it. The
  // chat's next message starts it over on its agent's workspace or here.
  const deleteMachine = useCallback(
    async (sessionId: string) => {
      try {
        await api.deleteMachine({ id: sessionId, project: projects[sessionId] });
        setWorkspaceOf(({ [sessionId]: _, ...rest }) => rest);
      } catch (e) {
        setError(`Failed to delete the computer: ${e}`);
      }
    },
    [projects, setError],
  );

  const takeOver = useCallback(
    async (sessionId: string) => {
      if (drivenId && drivenId !== sessionId) await handBack();
      try {
        const held = await api.screenControl(sessionId, projects[sessionId], true);
        if (!held) throw new Error("the computer kept its screen");
        setDrivenId(sessionId);
        setCurrentView("computers");
      } catch (e) {
        setError(`Failed to take over: ${e}`);
      }
    },
    [drivenId, handBack, projects, setError],
  );

  // Start a fresh chat in a project with a given prompt (the timeline's
  // "Generate" button) and switch to it so progress and approvals are visible.
  const promptProject = useCallback(
    async (name: string, text: string) => {
      const runId = crypto.randomUUID();
      assignProject(runId, name);
      setActiveProject(name);
      setActiveId(runId);
      activeIdRef.current = runId;
      setCurrentView("chat");
      await sendPrompt(runId, text, name);
    },
    [assignProject, sendPrompt],
  );

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

  const unassignProject = useCallback(
    (sessionId: string) => {
      setProjects((prev) => {
        const next = { ...prev };
        delete next[sessionId];
        return next;
      });
      moveConversationStore(sessionId, undefined);
    },
    [moveConversationStore],
  );

  // Content projects need ffmpeg, whisper-cli and the whisper model in
  // ~/.infer/bin/tools so the agent never has to fetch tools itself.
  const prepareContentTools = useCallback(async () => {
    const ch = new Channel<ProgressEvent>();
    ch.onmessage = (e) => {
      switch (e.kind) {
        case "Checking":
          setStatus("Checking video tools...");
          break;
        case "Installing":
          setStatus("Installing tools...");
          break;
        case "Downloading":
          setStatus(
            e.total > 0
              ? `Downloading voice model... ${Math.round((e.received / e.total) * 100)}%`
              : "Downloading voice model...",
          );
          break;
        case "Ready":
          setStatus("Video tools ready");
          break;
      }
    };
    try {
      await api.prepareContentTools(ch);
    } catch (err) {
      setError(`Failed to install video tools: ${err}`);
    }
  }, [setStatus, setError]);

  const setProjectType = useCallback(
    (name: string, type: ProjectType) => {
      if (type === "content") prepareContentTools();
      setProjectTypes((prev) => {
        if (type === "code") {
          if (!(name in prev)) return prev;
          const next = { ...prev };
          delete next[name];
          return next;
        }
        return { ...prev, [name]: type };
      });
    },
    [prepareContentTools],
  );

  const initProject = useCallback(
    async (name: string) => {
      if (runningIdsRef.current.size >= maxSessions) {
        logActivity(name, "Init", "skipped", `max ${maxSessions} concurrent sessions`);
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
    [gitProjects, maxSessions, assignProject, sendPrompt, loadProjects, setProjectContext, setError, logActivity],
  );

  const startInitSelection = useCallback(() => {
    setInitSelecting(true);
    setInitSelection((prev) => {
      const kept = visibleProjectNames.filter((n) => prev.has(n));
      return new Set(kept.length > 0 ? kept : visibleProjectNames);
    });
  }, [visibleProjectNames]);

  useEffect(() => {
    if (!initSelecting) return;
    setInitSelection((prev) => {
      const kept = visibleProjectNames.filter((n) => prev.has(n));
      return kept.length === prev.size ? prev : new Set(kept);
    });
  }, [initSelecting, visibleProjectNames]);

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

  const selectAllProjects = useCallback(() => setInitSelection(new Set(visibleProjectNames)), [visibleProjectNames]);

  const clearProjectSelection = useCallback(() => setInitSelection(new Set()), []);

  const selectProjectsInGroup = useCallback(
    (group: string) => {
      setInitSelection((prev) => {
        const next = new Set(prev);
        const inGroup = visibleProjectNames.filter((n) => projectGroups[n] === group);
        const allIn = inGroup.length > 0 && inGroup.every((n) => next.has(n));
        for (const n of inGroup) {
          if (allIn) next.delete(n);
          else next.add(n);
        }
        return next;
      });
    },
    [visibleProjectNames, projectGroups],
  );

  const initAllProjects = useCallback(
    (names: string[]) => runOnProjects(names, initProject, "Init"),
    [runOnProjects, initProject],
  );

  const cleanupProjects = useCallback(
    async (names: string[]) => {
      for (const name of names.filter((n) => !gitProjects.has(n)))
        logActivity(name, "Clean up branches", "skipped", "not a git repo");
      await runOnProjects(
        names.filter((n) => gitProjects.has(n)),
        cleanupProject,
        "Clean up branches",
      );
      await refreshGitProjects();
    },
    [runOnProjects, gitProjects, refreshGitProjects, cleanupProject, logActivity],
  );

  const currentProject = (activeId ? projects[activeId] : null) ?? activeProject;
  const currentAgent = (activeId ? agentOf[activeId] : null) ?? activeAgent;

  useEffect(() => {
    let stale = false;
    api
      .listHistory(currentProject)
      .then((entries) => {
        if (!stale) setHistory(entries);
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  }, [currentProject]);

  const queuedItems = activeId != null ? queuedPrompts[activeId] : undefined;
  const queuedPrompt =
    queuedItems && queuedItems.length > 0
      ? (queuedItems[0] ?? "") + (queuedItems.length > 1 ? ` (+${queuedItems.length - 1} more)` : "")
      : null;

  const active = (activeId && transcripts[activeId]) || initialChatState;
  const running = activeId != null && runningIds.has(activeId);
  const activeAgentMode = (activeId != null ? agentModes[activeId] : undefined) ?? agentMode;

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
      } else if (shortcut === "cycleMode") {
        e.preventDefault();
        setAgentMode(nextMode(activeAgentMode));
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [running, cancel, newChat, activeAgentMode, setAgentMode]);

  const runLabel = useCallback(
    (id: string): { label: string; error: boolean } | null => {
      const chat = transcripts[id];
      const pending = pendingInput(chat?.items ?? []);
      if (pending) {
        return { label: AWAITING_LABELS[pending], error: false };
      }
      if (runningIds.has(id)) {
        const starting = Object.entries(chat?.agentStartup ?? {})[0];
        if (starting) {
          const [name, st] = starting;
          const pct = st.total > 0 ? ` ${Math.round((st.done / st.total) * 100)}%` : "";
          return { label: `Starting ${name} (${st.message}${pct})...`, error: false };
        }
        if (chat?.currentReasoningId) return { label: "Thinking...", error: false };
        const tool = [...(chat?.items ?? [])].reverse().find((it) => it.kind === "tool" && it.state === "running");
        if (tool?.kind === "tool") return { label: `Running ${tool.name}...`, error: false };
        const tasks = (chat?.backgroundJobs ?? []).filter((j) => j.status === "running").length;
        return {
          label: tasks > 0 ? `Waiting on tasks (${tasks})...` : servicesStarting ? "Starting agents..." : "Running...",
          error: false,
        };
      }
      return lastRun[id] ?? null;
    },
    [transcripts, runningIds, lastRun, servicesStarting],
  );

  useEffect(() => {
    for (const id of Object.keys(queuedPrompts)) {
      if (!runLabel(id)?.label.startsWith("Waiting on tasks")) continue;
      const text = clearQueued(id);
      if (!text) continue;
      dispatchTo(id, { type: "userSend", text });
      api.sendUserMessage(id, text).catch((err) => {
        dispatchTo(id, { type: "error", text: `Follow-up failed: ${err}` });
        queuePrompt(id, text);
      });
    }
  }, [queuedPrompts, runLabel, clearQueued, dispatchTo, queuePrompt]);

  const delegations = useCallback(
    (id: string): Delegation[] => delegationsFrom(transcripts[id]?.items ?? [], transcripts[id]?.backgroundJobs ?? []),
    [transcripts],
  );
  const runningTasks = useCallback(
    (id: string): number => (transcripts[id]?.backgroundJobs ?? []).filter((j) => j.status === "running").length,
    [transcripts],
  );
  const runningSubagents = useMemo(
    () =>
      Object.values(transcripts).reduce(
        (n, t) => n + delegationsFrom(t.items, t.backgroundJobs).filter((d) => d.kind === "subagent").length,
        0,
      ),
    [transcripts],
  );
  useEffect(() => {
    if (runningSubagents === 0) return;
    refreshConversations();
    const later = setTimeout(refreshConversations, SUBAGENT_SESSION_SETTLE_MS);
    return () => clearTimeout(later);
  }, [runningSubagents, refreshConversations]);

  const stream = {
    transcripts,
    items: active.items,
    recording: active.recording,
    tokenUsage: active.usage,
    runLabel,
    delegations,
    runningTasks,
  };
  const session = {
    typing: active.typing,
    suggestion: active.suggestion,
    dismissSuggestion,
    running,
    enabled: ready && !running,
    agentMode: activeAgentMode,
    setAgentMode,
    maxSessions,
    setMaxSessions,
    conversations: displayConversations,
    selected,
    sessionId: activeId,
    isRunning,
    isAwaitingApproval,
    recordingOwner,
    runningCount: runningIds.size,
    onChatClick,
    clearSelection,
    deleteConversation,
    bulkDelete,
    send,
    sendText,
    cancel,
    queuedPrompt,
    discardQueued,
    popQueued,
    approve,
    answerQuestions,
    decidePlan,
    openConversation,
    newChat,
    selectProject,
    selectAgent,
    currentAgent,
    openAgentEditor,
    saveAgent,
    currentView,
    sidebarOpen,
    setSidebarOpen,
    chatOpen,
    setChatOpen,
    openSettings,
    openObservability,
    openComputers,
    drivenId,
    takeOver,
    handBack,
    moving,
    moveChat,
    cleanMachines,
    deleteMachine,
    promptProject,
    runningIds,
    setCurrentView,
    saveSettings,
    getConfig,
    getAuth,
    saveConfig,
    composerRef,
    history,
    todoDrafts,
    setTodoDraft,
    insertSnippet,
    turnStartedAt: active.turnStartedAt,
    lastTurnMs: active.lastTurnMs,
    showStatusBar,
    setShowStatusBar,
    currentProject,
    setProjectType,
    initialSettingsTab,
    setInitialSettingsTab,
    initialProjectFilter,
    setInitialProjectFilter,
    assignProject,
    unassignProject,
    initProject,
    initAllProjects,
    cleanupProjects,
    broadcastPrompt,
    initAllRunning,
    initSelecting,
    startInitSelection,
    cancelInitSelection,
    toggleInitSelection,
    selectAllProjects,
    projectFilter,
    setProjectFilter,
    visibleProjectNames,
    clearProjectSelection,
    selectProjectsInGroup,
  };
  return { session: useUnchanged(session), stream: useUnchanged(stream) };
}

export type DesktopStore = ReturnType<typeof useDesktopStore>["session"];
export type ChatStream = ReturnType<typeof useDesktopStore>["stream"];

const DesktopContext = createContext<DesktopStore | null>(null);
const ChatStreamContext = createContext<ChatStream | null>(null);

// ponytail: a subagent's session file lands shortly after its Agent call starts; a
// second refresh after this delay catches it. Replace with a daemon push when one exists.
const SUBAGENT_SESSION_SETTLE_MS = 1500;

function DesktopStoreProvider({ children }: { children: ReactNode }) {
  const { session, stream } = useDesktopStore();
  return (
    <DesktopContext.Provider value={session}>
      <ChatStreamContext.Provider value={stream}>{children}</ChatStreamContext.Provider>
    </DesktopContext.Provider>
  );
}

/** Every store the app reads. The feature stores sit outside the main one,
 * which uses them, and each has its own context so its readers re-render only
 * when that feature changes. */
export function DesktopProvider({ children }: { children: ReactNode }) {
  return (
    <StatusProvider>
      <ActivityProvider>
        <SnippetsProvider>
          <BackendProvider>
            <GitProvider>
              <SidebarProvider>
                <AgentsProvider>
                  <DesktopStoreProvider>{children}</DesktopStoreProvider>
                </AgentsProvider>
              </SidebarProvider>
            </GitProvider>
          </BackendProvider>
        </SnippetsProvider>
      </ActivityProvider>
    </StatusProvider>
  );
}

/** What streams in while an agent works: the transcripts and what is read off
 * them. It changes on every agent event, so only the components that show the
 * stream read it - everything else uses `useDesktop`. */
export function useChatStream(): ChatStream {
  const ctx = useContext(ChatStreamContext);
  if (!ctx) throw new Error("useChatStream must be used within DesktopProvider");
  return ctx;
}

export function useDesktop(): DesktopStore {
  const ctx = useContext(DesktopContext);
  if (!ctx) throw new Error("useDesktop must be used within DesktopProvider");
  return ctx;
}
