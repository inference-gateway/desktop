// Pure transcript state machine. Maps agent stream events (and loaded history)
// to a flat list of render items. A faithful port of the imperative DOM logic
// in the old main.js. Self-check: `bun test src/lib/transcript.test.ts`.
import type {
  AgentEvent,
  BackgroundJob,
  SnapshotMessage,
  InputEvent,
  RecordedArea,
  UserQuestion,
  UserQuestionAnswer,
  PromptRef,
} from "./tauri";
import { imageFilename, parseToolResult, safeAudioSrc, safeImageSrc } from "./tools";
import type { PlanAction } from "./agent-mode";

export type ToolState = "running" | "done" | "failed" | "cancelled";

export type TranscriptItem =
  | { kind: "user"; id: string; text: string }
  | { kind: "assistant"; id: string; chunks: string[] }
  | { kind: "reasoning"; id: string; paragraphs: string[] }
  | {
      kind: "tool";
      id: string;
      callId: string | null;
      name: string;
      args: string;
      output: string | null;
      state: ToolState;
      skeleton: boolean;
    }
  | {
      kind: "approval";
      id: string;
      callId: string;
      toolName: string;
      toolArgs: string;
      status: "pending" | "approved" | "denied" | "expired";
    }
  | {
      kind: "question";
      id: string;
      callId: string;
      questions: UserQuestion[];
      answers: UserQuestionAnswer[];
      status: "pending" | "answered" | "skipped" | "expired";
    }
  | {
      kind: "plan";
      id: string;
      planId: string;
      callId: string | null;
      plan: string;
      status: "pending" | PlanAction | "expired";
    }
  | { kind: "image"; id: string; src: string; filename: string; path: string }
  | { kind: "audio"; id: string; src: string; filename: string; path: string }
  | { kind: "error"; id: string; text: string }
  | { kind: "cancelled"; id: string }
  | { kind: "info"; id: string; text: string }
  | { kind: "task_result"; id: string; text: string };

type ToolItem = Extract<TranscriptItem, { kind: "tool" }>;

export type TokenUsage = {
  input: number;
  output: number;
  cached_read: number;
  total_tool_calls: number;
  last_input: number;
  context_window: number;
  cost: number;
};

export type ChatState = {
  items: TranscriptItem[];
  usage: TokenUsage;
  typing: boolean;
  seq: number;
  currentAssistantId: string | null;
  currentAssistantMessageId: string | null;
  currentReasoningId: string | null;
  currentReasoningMessageId: string | null;
  seenImages: string[];
  paused?: boolean;
  backgroundJobs: BackgroundJob[];
  agentStartup: Record<string, { message: string; done: number; total: number }>;
  recording?: AgentRecording;
  suggestion: string;
  turnStartedAt: number | null;
  lastTurnMs: number | null;
};

function startTurn(state: ChatState, now: number): ChatState {
  return state.turnStartedAt === null ? { ...state, turnStartedAt: now } : state;
}

function endTurn(state: ChatState, now: number): ChatState {
  return state.turnStartedAt === null
    ? state
    : { ...state, turnStartedAt: null, lastTurnMs: now - state.turnStartedAt };
}

/** The agent recording this session's process owns, from RecordingStarted until
 * it stops or the process exits. */
export type AgentRecording = { area?: RecordedArea; path?: string; inputs: InputEvent[] };

type DecisionItem = Extract<TranscriptItem, { kind: "approval" | "question" | "plan" }>;

/** An item that waits on the user: a tool approval, a question form or a plan. */
function isDecision(it: TranscriptItem): it is DecisionItem {
  return it.kind === "approval" || it.kind === "question" || it.kind === "plan";
}

/** The run is blocked on the user: a pending approval, question or plan card. */
export function pendingInput(items: TranscriptItem[]): "approval" | "question" | "plan" | null {
  const it = items.find((i) => isDecision(i) && i.status === "pending");
  return it ? (it.kind as "approval" | "question" | "plan") : null;
}

export const COMPUTER_USE_TOOLS = new Set(["Computer", "GetLatestFrame"]);

export const SANDBOX_ACCESS_TOOL = "SandboxAccess";

export const PLAN_APPROVAL_TOOL = "RequestPlanApproval";

/** The call id of the latest RequestPlanApproval row, the call a new plan card answers. */
function lastPlanCallId(items: TranscriptItem[]): string | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind === "tool" && it.name === PLAN_APPROVAL_TOOL) return it.callId;
  }
  return null;
}

/** The items to render: a plan card stands in for the RequestPlanApproval call that submitted it,
 * so that call's row is left out. A reopened chat has no plan cards and keeps its rows. */
export function visibleItems(items: TranscriptItem[]): TranscriptItem[] {
  const answered = new Set(items.flatMap((it) => (it.kind === "plan" && it.callId ? [it.callId] : [])));
  if (answered.size === 0) return items;
  return items.filter((it) => !(it.kind === "tool" && it.callId !== null && answered.has(it.callId)));
}

/** Auto mode answers approvals for the user, except a path outside the sandbox, which the user decides. */
export function autoApproves(toolName: string): boolean {
  return toolName !== SANDBOX_ACCESS_TOOL;
}

export type Delegation = { id: string; label: string; kind: "subagent" | "a2a" | "shell" | "recording" };

// Subagent sessions are persisted by the CLI as "subagent-<parentId>-<childId>".
// ponytail: correlation parsed from the id convention - replace with a real
// parent_session_id field once the CLI exposes one in `conversations list`.
const SUBAGENT_SESSION = /^subagent-([0-9a-fA-F-]{36})-[0-9a-fA-F-]{36}$/;

export function subagentParentId(sessionId: string): string | null {
  return SUBAGENT_SESSION.exec(sessionId)?.[1] ?? null;
}

/** The stored session of a subagent delegation under `parentId`: the parent's
 * only child session, or the one whose title matches the delegation label.
 * A2A, shell and recording delegations have no session of their own. */
export function delegationSessionId(
  parentId: string,
  delegation: Delegation,
  conversations: { id: string; title?: string | null }[],
): string | null {
  if (delegation.kind !== "subagent") return null;
  const children = conversations.filter((c) => subagentParentId(c.id) === parentId);
  if (children.length === 1) return children[0].id;
  const label = trimEllipsis(delegation.label);
  return children.find((c) => c.title && sameCut(trimEllipsis(c.title), label))?.id ?? null;
}

// A title and a label are both cuts of the same prompt, at different lengths.
const sameCut = (a: string, b: string): boolean => a.startsWith(b) || b.startsWith(a);

const trimEllipsis = (text: string): string => text.replace(/(…|\.\.\.)$/, "");

const DELEGATION_LABEL_MAX = 48;

function delegationLabel(text: unknown): string | null {
  if (typeof text !== "string" || !text.trim()) return null;
  const t = text.trim();
  return t.length > DELEGATION_LABEL_MAX ? `${t.slice(0, DELEGATION_LABEL_MAX)}…` : t;
}

// Running tool calls give the live view while a submit is still open; the
// CLI's background_tasks snapshot (jobs) takes over once the submit returns.
export function delegationsFrom(items: TranscriptItem[], jobs: BackgroundJob[] = []): Delegation[] {
  const out: Delegation[] = [];
  for (const j of jobs) {
    if (j.status !== "running") continue;
    out.push({
      id: `job:${j.id}`,
      label: delegationLabel(j.description) ?? delegationLabel(j.label) ?? delegationLabel(j.detail) ?? j.id,
      kind: j.kind === "a2a" || j.kind === "shell" || j.kind === "recording" ? j.kind : "subagent",
    });
  }
  for (const it of items) {
    if (it.kind !== "tool" || it.state !== "running") continue;
    const key = it.callId ?? it.id;
    let args: Record<string, unknown> | null = null;
    try {
      const parsed: unknown = JSON.parse(it.args);
      if (parsed && typeof parsed === "object") args = parsed as Record<string, unknown>;
    } catch {
      args = null;
    }
    if (it.name === "Agent") {
      const tasks = Array.isArray(args?.tasks) ? (args.tasks as unknown[]) : args ? [args] : [{}];
      for (const [i, raw] of tasks.entries()) {
        const task = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
        out.push({
          id: `${key}:${i}`,
          label: delegationLabel(task.label) ?? delegationLabel(task.description) ?? "agent",
          kind: "subagent",
        });
      }
    } else if (it.name.startsWith("A2A_")) {
      out.push({
        id: `${key}:0`,
        label:
          delegationLabel(args?.agent) ?? delegationLabel(args?.name) ?? delegationLabel(args?.url) ?? it.name.slice(4),
        kind: "a2a",
      });
    }
  }
  return out;
}

export type TodoStatus = "pending" | "in_progress" | "completed";

/** A todo of the session's list. The CLI stamps the times - `started_at` while it is in progress and `elapsed_ms`
 * for the stretches that ended - and keeps them by `id`. */
export type TodoItem = { content: string; status: TodoStatus; id?: string; started_at?: string; elapsed_ms?: number };

/** The list a TodoWrite call ended up with: the one its result carries, which has the times the CLI stamped, or the
 * one the model wrote while the call has no result yet. A tool's output is the `data` of its result. */
function writtenTodos(call: { args: string; output: string | null }): unknown {
  try {
    const stamped = JSON.parse(call.output ?? "")?.todos;
    if (Array.isArray(stamped)) return stamped;
  } catch {
    // not a result yet, or one from a CLI that answers in plain text
  }
  const parsed: unknown = JSON.parse(call.args);
  return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>).todos : null;
}

/** How long a todo has been worked on by `now`, the stretch it is in progress for included. Null when the CLI
 * stamped no time on it. */
export function todoElapsedMs(todo: TodoItem, now: number): number | null {
  const running = todo.status === "in_progress" && todo.started_at ? now - Date.parse(todo.started_at) : null;
  if (running === null && todo.elapsed_ms === undefined) return null;
  return (todo.elapsed_ms ?? 0) + Math.max(0, running ?? 0);
}

/** The todo to show when the list is folded away: the one in progress, else the next one to do. */
export function currentTodo(todos: TodoItem[]): TodoItem | null {
  return todos.find((t) => t.status === "in_progress") ?? todos.find((t) => t.status === "pending") ?? null;
}

// The pinned todo panel shows the list written by the latest TodoWrite call.
// Scanning backwards covers history reloads and lets partially-streamed or
// malformed args fall through to the previous call instead of blanking out.
export function todosFrom(items: TranscriptItem[]): TodoItem[] {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind !== "tool" || it.name !== "TodoWrite") continue;
    try {
      const todos = writtenTodos(it);
      if (!Array.isArray(todos)) continue;
      const out: TodoItem[] = [];
      for (const raw of todos) {
        const t = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
        if (typeof t.content !== "string" || !t.content.trim()) continue;
        out.push({
          content: t.content,
          status: t.status === "in_progress" || t.status === "completed" ? t.status : "pending",
          ...(typeof t.id === "string" && { id: t.id }),
          ...(typeof t.started_at === "string" && { started_at: t.started_at }),
          ...(typeof t.elapsed_ms === "number" && { elapsed_ms: t.elapsed_ms }),
        });
      }
      return out;
    } catch {
      continue;
    }
  }
  return [];
}

// A background job's landed note starts with the supervisor's header line,
// e.g. "[A2A Task Completed: <label>]" (see the CLI's Supervisor.formatResult).
const BACKGROUND_NOTE = /^\[(A2A Task|Background Shell|Subagent|Background Job) (Completed|Failed): (.*)\]/;

export function backgroundNoteHeader(text: string): { kind: string; failed: boolean; label: string } | null {
  const m = BACKGROUND_NOTE.exec(text.trimStart());
  return m ? { kind: m[1], failed: m[2] === "Failed", label: m[3] } : null;
}

// Panel edits are dirty when they differ from the agent's last written list.
/** Whether the transcript keeps following its bottom after a scroll event.
 * Reaching the bottom turns it on and only scrolling up turns it off: the
 * scroll event of a programmatic pin can land after content grew under it (a
 * screenshot that finished loading) and must not stop the follow. The image's
 * own `load` then re-pins. */
export function followsBottom(following: boolean, scrolledUp: boolean, atBottom: boolean): boolean {
  return atBottom || (following && !scrolledUp);
}

export function todosDiffer(a: TodoItem[], b: TodoItem[]): boolean {
  return a.length !== b.length || a.some((t, i) => t.content !== b[i].content || t.status !== b[i].status);
}

export const initialChatState: ChatState = {
  items: [],
  usage: { input: 0, output: 0, cached_read: 0, total_tool_calls: 0, last_input: 0, context_window: 0, cost: 0 },
  typing: false,
  seq: 0,
  currentAssistantId: null,
  currentAssistantMessageId: null,
  currentReasoningId: null,
  currentReasoningMessageId: null,
  seenImages: [],
  backgroundJobs: [],
  agentStartup: {},
  suggestion: "",
  turnStartedAt: null,
  lastTurnMs: null,
};

export type ChatAction =
  | { type: "newChat" }
  | { type: "loadHistory"; messages: SnapshotMessage[] }
  | { type: "setUsage"; usage: TokenUsage }
  | { type: "userSend"; text: string }
  | { type: "rewind"; itemId: string }
  | { type: "event"; event: AgentEvent }
  | { type: "setApproval"; callId: string; status: "approved" | "denied" }
  | { type: "setQuestion"; callId: string; answers: UserQuestionAnswer[] | null }
  | { type: "setPlan"; planId: string; action: PlanAction }
  | { type: "dismissSuggestion" }
  | { type: "error"; text: string };

const IMAGE_TOOL = /^Image(Generation|Edit|Variation)$/;
// Last resort when a tool result carries no structured success flag: a CLI
// older than the one that projects entry.tool_execution, or a result string
// that is not JSON. ponytail: a substring guess over the whole result - it
// reads "error" in a commit message as a failure. Delete once the desktop can
// require a CLI that always projects tool_execution.
const FAILISH = /fail|error/i;

export function chatReducer(state: ChatState, action: ChatAction, now: number = Date.now()): ChatState {
  switch (action.type) {
    case "newChat":
      return { ...initialChatState, seq: state.seq };
    case "loadHistory":
      return loadHistory(state, action.messages);
    case "setUsage":
      return { ...state, usage: action.usage };
    case "rewind": {
      const at = state.items.findIndex((item) => item.id === action.itemId);
      return at < 0 ? state : { ...state, items: state.items.slice(0, at) };
    }
    case "userSend": {
      let seq = state.seq;
      const items = [...state.items, { kind: "user", id: String(seq++), text: action.text } as TranscriptItem];
      return {
        ...startTurn(state, now),
        items,
        seq,
        typing: true,
        currentAssistantId: null,
        currentAssistantMessageId: null,
        currentReasoningId: null,
        currentReasoningMessageId: null,
        suggestion: "",
      };
    }
    case "dismissSuggestion":
      return state.suggestion ? { ...state, suggestion: "" } : state;
    case "error": {
      let seq = state.seq;
      const items = [...state.items, { kind: "error", id: String(seq++), text: action.text } as TranscriptItem];
      return { ...state, items, seq, typing: false };
    }
    case "setApproval": {
      if (!state.items.some((it) => it.kind === "approval" && it.callId === action.callId && it.status === "pending")) {
        return state;
      }
      return {
        ...state,
        typing: true,
        items: state.items.map((it) =>
          it.kind === "approval" && it.callId === action.callId ? { ...it, status: action.status } : it,
        ),
      };
    }
    case "setQuestion": {
      if (!state.items.some((it) => it.kind === "question" && it.callId === action.callId && it.status === "pending")) {
        return state;
      }
      return {
        ...state,
        typing: true,
        items: state.items.map((it) =>
          it.kind === "question" && it.callId === action.callId
            ? { ...it, status: action.answers ? "answered" : "skipped", answers: action.answers ?? [] }
            : it,
        ),
      };
    }
    case "setPlan": {
      if (!state.items.some((it) => it.kind === "plan" && it.planId === action.planId && it.status === "pending")) {
        return state;
      }
      return {
        ...state,
        typing: true,
        items: state.items.map((it) =>
          it.kind === "plan" && it.planId === action.planId ? { ...it, status: action.action } : it,
        ),
      };
    }
    case "event":
      return applyEvent(state, action.event, now);
  }
}

// Approval and question cards land above any trailing error bubbles so the
// prompt is not buried under an error the run already emitted.
function insertBeforeErrors(state: ChatState, item: TranscriptItem): ChatState {
  let insertAt = state.items.length;
  for (let i = state.items.length - 1; i >= 0; i--) {
    if (state.items[i].kind !== "error") break;
    insertAt = i;
  }
  const items = [...state.items.slice(0, insertAt), item, ...state.items.slice(insertAt)];
  return { ...state, items, seq: state.seq + 1, typing: false };
}

function applyEvent(state: ChatState, event: AgentEvent, now: number): ChatState {
  switch (event.kind) {
    case "RunStarted":
      return startTurn(state, now);
    case "SessionId":
    case "RawLine":
    case "ComputerUseAction":
      return state;
    case "AssistantMessage":
      return applyAssistant(state, event);
    case "ToolResult":
      return applyToolResult(state, event.tool_call_id, event.content);
    case "ToolCancelled":
      return {
        ...state,
        items: state.items.map((it) =>
          it.kind === "tool" && it.callId === event.tool_call_id ? { ...it, state: "cancelled", skeleton: false } : it,
        ),
      };
    case "ConversationOptimized": {
      const text = `Conversation optimized - ${event.original_count} to ${event.optimized_count} messages`;
      return { ...state, items: [...state.items, { kind: "info", id: String(state.seq), text }], seq: state.seq + 1 };
    }
    case "ApprovalRequest":
      return insertBeforeErrors(state, {
        kind: "approval",
        id: String(state.seq),
        callId: event.tool_call_id,
        toolName: event.tool_name,
        toolArgs: event.tool_args,
        status: "pending",
      });
    case "UserQuestionRequest":
      return insertBeforeErrors(state, {
        kind: "question",
        id: String(state.seq),
        callId: event.tool_call_id,
        questions: Array.isArray(event.questions) ? event.questions : [],
        answers: [],
        status: "pending",
      });
    case "PlanApprovalRequest":
      return insertBeforeErrors(state, {
        kind: "plan",
        id: String(state.seq),
        planId: event.plan_id,
        callId: lastPlanCallId(state.items),
        plan: event.plan,
        status: "pending",
      });
    case "AgentError": {
      let seq = state.seq;
      const text =
        event.message === "max_turns_reached"
          ? 'Turn limit reached. Send a message (e.g. "continue") to keep going.'
          : event.message;
      const items = [...state.items, { kind: "error", id: String(seq++), text } as TranscriptItem];
      return { ...state, items, seq, typing: false, recording: undefined };
    }
    case "ComputerUsePaused":
      return { ...state, paused: true };
    case "ComputerUseResumed":
      return { ...state, paused: false };
    case "AgentStatus": {
      const agentStartup = { ...state.agentStartup };
      if (event.state === "Ready" || event.state === "Failed") delete agentStartup[event.name];
      else agentStartup[event.name] = { message: event.message, done: event.done, total: event.total };
      return { ...state, agentStartup };
    }
    case "UserMessage": {
      if (!event.content.trim()) return state;
      const last = state.items[state.items.length - 1];
      if (last?.kind === "user" && last.text === event.content) return state;
      let seq = state.seq;
      const items = [...state.items, { kind: "user", id: String(seq++), text: event.content } as TranscriptItem];
      return { ...state, items, seq, currentAssistantId: null, currentAssistantMessageId: null };
    }
    case "BackgroundNote": {
      if (!event.content.trim()) return state;
      let seq = state.seq;
      const items = [...state.items, { kind: "task_result", id: String(seq++), text: event.content } as TranscriptItem];
      return { ...state, items, seq, currentAssistantId: null, currentAssistantMessageId: null };
    }
    case "BackgroundTasks":
      return { ...state, backgroundJobs: Array.isArray(event.jobs) ? event.jobs : [] };
    case "NextStepSuggestion":
      return { ...state, suggestion: event.text };
    case "RecordingStarted":
      return { ...state, recording: { inputs: [], area: event.area, path: event.path } };
    case "RecordingInput":
      return state.recording
        ? { ...state, recording: { ...state.recording, inputs: [...state.recording.inputs, event.input] } }
        : state;
    case "RecordingStopped":
      return { ...state, recording: undefined };
    case "Done":
      return {
        ...finalizeTools(endTurn(state, now), "done"),
        typing: false,
        currentAssistantId: null,
        currentAssistantMessageId: null,
        currentReasoningId: null,
        currentReasoningMessageId: null,
        paused: false,
        backgroundJobs: [],
        agentStartup: {},
        recording: undefined,
      };
    case "TokenUsage": {
      const { kind: _kind, ...usage } = event;
      return {
        ...state,
        usage: {
          ...usage,
          total_tool_calls: Math.max(state.usage.total_tool_calls, usage.total_tool_calls),
          context_window: usage.context_window || state.usage.context_window,
          cost: usage.cost || state.usage.cost,
        },
      };
    }
    case "Cancelled": {
      const finalized = finalizeTools(endTurn(state, now), "cancelled");
      let seq = finalized.seq;
      const items = [...finalized.items, { kind: "cancelled", id: String(seq++) } as TranscriptItem];
      return {
        ...finalized,
        items,
        seq,
        typing: false,
        currentAssistantId: null,
        currentAssistantMessageId: null,
        currentReasoningId: null,
        currentReasoningMessageId: null,
        paused: false,
        recording: undefined,
      };
    }
  }
}

// Tool calls stream in one call at a time (TOOL_CALL_END); the CLI's
// RUN_FINISHED stats later overwrite the running count with session totals.
function applyAssistant(state: ChatState, event: Extract<AgentEvent, { kind: "AssistantMessage" }>): ChatState {
  let seq = state.seq;
  let items = state.items;
  let currentAssistantId = state.currentAssistantId;
  let currentAssistantMessageId = state.currentAssistantMessageId;
  let currentReasoningId = state.currentReasoningId;
  let currentReasoningMessageId = state.currentReasoningMessageId;
  let usage = state.usage;

  if (event.reasoning_content) {
    const text = event.reasoning_content;
    if (event.message_id && event.message_id !== currentReasoningMessageId) {
      currentReasoningId = null;
    }
    currentAssistantId = null;
    currentAssistantMessageId = null;
    if (currentReasoningId) {
      items = items.map((it) =>
        it.kind === "reasoning" && it.id === currentReasoningId ? { ...it, paragraphs: [...it.paragraphs, text] } : it,
      );
    } else {
      const id = String(seq++);
      items = [...items, { kind: "reasoning", id, paragraphs: [text] }];
      currentReasoningId = id;
    }
    if (event.message_id) currentReasoningMessageId = event.message_id;
  }

  if (event.content) {
    const text = event.content;
    if (event.message_id && event.message_id !== currentAssistantMessageId) {
      currentAssistantId = null;
    }
    currentReasoningId = null;
    currentReasoningMessageId = null;
    if (currentAssistantId) {
      items = items.map((it) =>
        it.kind === "assistant" && it.id === currentAssistantId ? { ...it, chunks: [...it.chunks, text] } : it,
      );
    } else {
      const id = String(seq++);
      items = [...items, { kind: "assistant", id, chunks: [text] }];
      currentAssistantId = id;
    }
    if (event.message_id) currentAssistantMessageId = event.message_id;
  }

  if (event.tool_calls.length) {
    usage = { ...usage, total_tool_calls: usage.total_tool_calls + event.tool_calls.length };
    const added: TranscriptItem[] = event.tool_calls.map((tc) => ({
      kind: "tool",
      id: String(seq++),
      callId: tc.id || null,
      name: tc.name,
      args: tc.args,
      output: null,
      state: "running",
      skeleton: IMAGE_TOOL.test(tc.name),
    }));
    items = [...items, ...added];
    currentAssistantId = null;
    currentAssistantMessageId = null;
    currentReasoningId = null;
    currentReasoningMessageId = null;
  }

  return {
    ...state,
    items,
    seq,
    currentAssistantId,
    currentAssistantMessageId,
    currentReasoningId,
    currentReasoningMessageId,
    usage,
  };
}

function applyToolResult(state: ChatState, callId: string, content: string): ChatState {
  let seq = state.seq;
  let seenImages = state.seenImages;
  const parsed = parseToolResult(content);
  let idx = state.items.findIndex((it) => it.kind === "tool" && it.callId === callId);
  if (idx < 0) {
    for (let i = state.items.length - 1; i >= 0; i--) {
      const it = state.items[i];
      if (it.kind === "tool" && it.output === null && (!parsed || it.name === parsed.name)) {
        idx = i;
        break;
      }
    }
  }

  let items: TranscriptItem[];
  if (idx >= 0) {
    const tool = state.items[idx] as ToolItem;
    const output = parsed ? parsed.output : content;
    const failed = parsed ? parsed.failed : FAILISH.test(content);
    items = state.items.slice();
    items[idx] = { ...tool, output, state: failed ? "failed" : "done", skeleton: false };
    const apprIdx = items.findIndex((it) => it.kind === "approval" && it.callId === callId);
    if (apprIdx > idx) {
      const [approval] = items.splice(apprIdx, 1);
      items.splice(idx, 0, approval);
    }
  } else if (parsed) {
    items = [
      ...state.items,
      {
        kind: "tool",
        id: String(seq++),
        callId: null,
        name: parsed.name,
        args: parsed.args,
        output: parsed.output,
        state: parsed.failed ? "failed" : "done",
        skeleton: false,
      },
    ];
  } else {
    items = [
      ...state.items,
      {
        kind: "tool",
        id: String(seq++),
        callId: null,
        name: "tool",
        args: content,
        output: "",
        state: FAILISH.test(content) ? "failed" : "done",
        skeleton: false,
      },
    ];
  }

  const src = parsed ? (safeImageSrc(parsed.imagePath) ?? parsed.imageData) : null;
  if (src && parsed) {
    const file = parsed.imagePath ? imageFilename(parsed.imagePath) : `${parsed.name}.jpeg`;
    const key = parsed.imagePath ? file : src;
    if (!seenImages.includes(key)) {
      seenImages = [...seenImages, key];
      items = [...items, { kind: "image", id: String(seq++), src, filename: file, path: parsed.imagePath ?? "" }];
    }
  }

  // TextToSpeech results name the generated WAV; render an inline player.
  const audioSrc = parsed ? safeAudioSrc(parsed.imagePath) : null;
  if (audioSrc && parsed?.imagePath) {
    const file = imageFilename(parsed.imagePath);
    if (!seenImages.includes(file)) {
      seenImages = [...seenImages, file];
      items = [...items, { kind: "audio", id: String(seq++), src: audioSrc, filename: file, path: parsed.imagePath }];
    }
  }

  let usage = state.usage;
  if (idx < 0) {
    usage = { ...usage, total_tool_calls: usage.total_tool_calls + 1 };
  }
  return { ...state, items, seq, seenImages, usage };
}

/** Closes a run's open items: a tool call still running takes `unfinished` -
 * done when the run ended, cancelled when the user stopped it before the call
 * got a result - and a pending approval or question expires. */
function finalizeTools(state: ChatState, unfinished: ToolState): ChatState {
  let changed = false;
  const items = state.items.map((it) => {
    if (it.kind === "tool" && (it.state === "running" || it.skeleton)) {
      changed = true;
      return { ...it, state: it.state === "running" ? unfinished : it.state, skeleton: false };
    }
    if (isDecision(it) && it.status === "pending") {
      changed = true;
      return { ...it, status: "expired" as const };
    }
    return it;
  });
  return changed ? { ...state, items } : state;
}

/** The ref of the prompt a transcript item shows, or null for any other item. */
export function promptRef(items: TranscriptItem[], itemId: string): PromptRef | null {
  const at = items.findIndex((item) => item.id === itemId);
  const prompt = items[at];
  if (prompt?.kind !== "user") return null;
  const sentAgain = items.slice(at).filter((item) => item.kind === "user" && item.text === prompt.text);
  return { text: prompt.text, occurrence: sentAgain.length };
}

/** Rebuilds the transcript from the thread's MESSAGES_SNAPSHOT: assistant
 * toolCalls become tool rows, a tool message fills the row with the same
 * toolCallId (failed when it carries error), and media paths in tool output
 * render inline. */
function loadHistory(state: ChatState, messages: SnapshotMessage[]): ChatState {
  let seq = state.seq;
  const items: TranscriptItem[] = [];
  let seenImages: string[] = [];

  const pushImage = (imagePath: string | null) => {
    const src = safeImageSrc(imagePath);
    if (!src || !imagePath) return;
    const file = imageFilename(imagePath);
    if (seenImages.includes(file)) return;
    seenImages = [...seenImages, file];
    items.push({ kind: "image", id: String(seq++), src, filename: file, path: imagePath });
  };

  const pushAudio = (audioPath: string | null) => {
    const src = safeAudioSrc(audioPath);
    if (!src || !audioPath) return;
    const file = imageFilename(audioPath);
    if (seenImages.includes(file)) return;
    seenImages = [...seenImages, file];
    items.push({ kind: "audio", id: String(seq++), src, filename: file, path: audioPath });
  };

  const toolRows = new Map<string, Extract<TranscriptItem, { kind: "tool" }>>();
  for (const m of messages) {
    if (!m || typeof m !== "object") continue;
    const content = typeof m.content === "string" ? m.content : "";
    if (m.role === "user") {
      if (content.startsWith("<system-reminder>")) continue;
      if (backgroundNoteHeader(content)) {
        items.push({ kind: "task_result", id: String(seq++), text: content });
        continue;
      }
      items.push({ kind: "user", id: String(seq++), text: content });
    } else if (m.role === "assistant") {
      if (content) items.push({ kind: "assistant", id: String(seq++), chunks: [content] });
      for (const tc of m.toolCalls ?? []) {
        const row: Extract<TranscriptItem, { kind: "tool" }> = {
          kind: "tool",
          id: String(seq++),
          callId: tc.id,
          name: tc.function.name,
          args: tc.function.arguments,
          output: "",
          state: "done",
          skeleton: false,
        };
        items.push(row);
        toolRows.set(tc.id, row);
      }
    } else if (m.role === "tool") {
      const row = m.toolCallId ? toolRows.get(m.toolCallId) : undefined;
      if (row) {
        row.output = m.error || content;
        if (m.error) row.state = "failed";
      } else {
        items.push({
          kind: "tool",
          id: String(seq++),
          callId: null,
          name: "tool",
          args: content,
          output: "",
          state: FAILISH.test(content) ? "failed" : "done",
          skeleton: false,
        });
      }
      for (const path of content.match(/\/\S+\.(?:wav|png|gif|webp|avif|jpe?g)\b/gi) ?? []) {
        pushImage(path);
        pushAudio(path);
      }
    }
  }

  const openErrors = state.items.filter((item) => item.kind === "error");
  return {
    ...initialChatState,
    items: [...items, ...openErrors],
    seq,
    seenImages,
    turnStartedAt: state.turnStartedAt,
    lastTurnMs: state.lastTurnMs,
  };
}
