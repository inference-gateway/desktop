// Pure transcript state machine. Maps agent stream events (and loaded history)
// to a flat list of render items. A faithful port of the imperative DOM logic
// in the old main.js. Self-check: `bun test src/lib/transcript.test.ts`.
import type { AgentEvent, HistoryLine } from "./tauri";
import { imageFilename, parseToolResult, safeImageSrc } from "./tools";

export type ToolState = "running" | "done" | "failed";

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
  | { kind: "image"; id: string; src: string; filename: string; path: string }
  | { kind: "error"; id: string; text: string }
  | { kind: "cancelled"; id: string };

type ToolItem = Extract<TranscriptItem, { kind: "tool" }>;

export type ChatState = {
  items: TranscriptItem[];
  typing: boolean;
  seq: number;
  currentAssistantId: string | null;
  currentAssistantMessageId: string | null;
  currentReasoningId: string | null;
  currentReasoningMessageId: string | null;
  seenImages: string[];
  paused?: boolean;
};

export const COMPUTER_USE_TOOLS = new Set(["Computer", "GetLatestFrame"]);

export type Delegation = { id: string; label: string; kind: "subagent" | "a2a" };

// Subagent sessions are persisted by the CLI as "subagent-<parentId>-<childId>".
// ponytail: correlation parsed from the id convention - replace with a real
// parent_session_id field once the CLI exposes one in `conversations list`.
const SUBAGENT_SESSION = /^subagent-([0-9a-fA-F-]{36})-[0-9a-fA-F-]{36}$/;

export function subagentParentId(sessionId: string): string | null {
  return SUBAGENT_SESSION.exec(sessionId)?.[1] ?? null;
}

const DELEGATION_LABEL_MAX = 48;

function delegationLabel(text: unknown): string | null {
  if (typeof text !== "string" || !text.trim()) return null;
  const t = text.trim();
  return t.length > DELEGATION_LABEL_MAX ? `${t.slice(0, DELEGATION_LABEL_MAX)}…` : t;
}

export function delegationsFrom(items: TranscriptItem[]): Delegation[] {
  const out: Delegation[] = [];
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
          delegationLabel(args?.agent) ??
          delegationLabel(args?.name) ??
          delegationLabel(args?.url) ??
          it.name.slice(4),
        kind: "a2a",
      });
    }
  }
  return out;
}

export const initialChatState: ChatState = {
  items: [],
  typing: false,
  seq: 0,
  currentAssistantId: null,
  currentAssistantMessageId: null,
  currentReasoningId: null,
  currentReasoningMessageId: null,
  seenImages: [],
};

export type ChatAction =
  | { type: "newChat" }
  | { type: "loadHistory"; ndjson: string }
  | { type: "userSend"; text: string }
  | { type: "event"; event: AgentEvent }
  | { type: "setApproval"; callId: string; status: "approved" | "denied" }
  | { type: "error"; text: string };

const IMAGE_TOOL = /^Image(Generation|Edit|Variation)$/;
const FAILISH = /fail|error/i;

export function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case "newChat":
      return { ...initialChatState, seq: state.seq };
    case "loadHistory":
      return loadHistory(state, action.ndjson);
    case "userSend": {
      let seq = state.seq;
      const items = [...state.items, { kind: "user", id: String(seq++), text: action.text } as TranscriptItem];
      return {
        ...state,
        items,
        seq,
        typing: true,
        currentAssistantId: null,
        currentAssistantMessageId: null,
        currentReasoningId: null,
        currentReasoningMessageId: null,
      };
    }
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
          it.kind === "approval" && it.callId === action.callId ? { ...it, status: action.status } : it
        ),
      };
    }
    case "event":
      return applyEvent(state, action.event);
  }
}

function applyEvent(state: ChatState, event: AgentEvent): ChatState {
  switch (event.kind) {
    case "SessionId":
    case "Info":
    case "RawLine":
      return state;
    case "AssistantMessage":
      return applyAssistant(state, event);
    case "ToolResult":
      return applyToolResult(state, event.tool_call_id, event.content);
    case "ApprovalRequest": {                                  
      let seq = state.seq;                                     
      let insertAt = state.items.length;                       
      for (let i = state.items.length - 1; i >= 0; i--) {     
        if (state.items[i].kind !== "error") break;           
        insertAt = i;                                          
      }                                                        
      const approval: TranscriptItem = {                       
        kind: "approval",                                      
        id: String(seq++),                                     
        callId: event.tool_call_id,                            
        toolName: event.tool_name,                             
        toolArgs: event.tool_args,                             
        status: "pending",                                     
      };                                                       
      const items = [                                          
        ...state.items.slice(0, insertAt),                     
        approval,                                              
        ...state.items.slice(insertAt),                        
      ];                                                       
      return { ...state, items, seq, typing: false };          
    }
    case "AgentError": {
      let seq = state.seq;
      const items = [...state.items, { kind: "error", id: String(seq++), text: event.message } as TranscriptItem];
      return { ...state, items, seq, typing: false };
    }
    case "ComputerUsePaused":
      return { ...state, paused: true };
    case "ComputerUseResumed":
      return { ...state, paused: false };
    case "Done":
      return {
        ...finalizeTools(state),
        typing: false,
        currentAssistantId: null,
        currentAssistantMessageId: null,
        currentReasoningId: null,
        currentReasoningMessageId: null,
        paused: false,
      };
    case "TokenUsage":
      return state;
    case "Cancelled": {
      const finalized = finalizeTools(state);
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
      };
    }
  }
}

function applyAssistant(
  state: ChatState,
  event: Extract<AgentEvent, { kind: "AssistantMessage" }>
): ChatState {
  let seq = state.seq;
  let items = state.items;
  let currentAssistantId = state.currentAssistantId;
  let currentAssistantMessageId = state.currentAssistantMessageId;
  let currentReasoningId = state.currentReasoningId;
  let currentReasoningMessageId = state.currentReasoningMessageId;

  if (event.reasoning_content) {
    const text = event.reasoning_content;
    if (event.message_id && event.message_id !== currentReasoningMessageId) {
      currentReasoningId = null;
    }
    currentAssistantId = null;
    currentAssistantMessageId = null;
    if (currentReasoningId) {
      items = items.map((it) =>
        it.kind === "reasoning" && it.id === currentReasoningId
          ? { ...it, paragraphs: [...it.paragraphs, text] }
          : it
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
        it.kind === "assistant" && it.id === currentAssistantId ? { ...it, chunks: [...it.chunks, text] } : it
      );
    } else {
      const id = String(seq++);
      items = [...items, { kind: "assistant", id, chunks: [text] }];
      currentAssistantId = id;
    }
    if (event.message_id) currentAssistantMessageId = event.message_id;
  }

  if (event.tool_calls.length) {
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

  const src = parsed ? safeImageSrc(parsed.imagePath) ?? parsed.imageData : null;
  if (src && parsed) {
    const file = parsed.imagePath ? imageFilename(parsed.imagePath) : `${parsed.name}.jpeg`;
    const key = parsed.imagePath ? file : src;
    if (!seenImages.includes(key)) {
      seenImages = [...seenImages, key];
      items = [...items, { kind: "image", id: String(seq++), src, filename: file, path: parsed.imagePath ?? "" }];
    }
  }

  return { ...state, items, seq, seenImages };
}

function finalizeTools(state: ChatState): ChatState {
  let changed = false;
  const items = state.items.map((it) => {
    if (it.kind === "tool" && (it.state === "running" || it.skeleton)) {
      changed = true;
      return { ...it, state: it.state === "running" ? "done" : it.state, skeleton: false };
    }
    if (it.kind === "approval" && it.status === "pending") {
      changed = true;
      return { ...it, status: "expired" as const };
    }
    return it;
  });
  return changed ? { ...state, items } : state;
}

function loadHistory(state: ChatState, ndjson: string): ChatState {
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

  for (const line of ndjson.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let entry: HistoryLine;
    try {
      entry = JSON.parse(trimmed);
    } catch {
      continue;
    }
    const content = entry.content || "";
    if (entry.role === "user") {
      items.push({ kind: "user", id: String(seq++), text: content });
    } else if (entry.role === "assistant") {
      if (entry.reasoning_content) items.push({ kind: "reasoning", id: String(seq++), paragraphs: [entry.reasoning_content] });
      if (content) items.push({ kind: "assistant", id: String(seq++), chunks: [content] });
    } else if (entry.role === "tool") {
      const parsed = parseToolResult(content);
      if (parsed) {
        items.push({
          kind: "tool",
          id: String(seq++),
          callId: null,
          name: parsed.name,
          args: parsed.args,
          output: parsed.output,
          state: parsed.failed ? "failed" : "done",
          skeleton: false,
        });
        pushImage(parsed.imagePath);
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
    }
  }

  return { ...initialChatState, items, seq, seenImages };
}
