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
      status: "pending" | "approved" | "denied";
    }
  | { kind: "image"; id: string; src: string; filename: string }
  | { kind: "error"; id: string; text: string }
  | { kind: "cancelled"; id: string };

type ToolItem = Extract<TranscriptItem, { kind: "tool" }>;

export type ChatState = {
  items: TranscriptItem[];
  typing: boolean;
  seq: number;
  currentAssistantId: string | null;
  currentReasoningId: string | null;
  seenImages: string[];
};

export const initialChatState: ChatState = {
  items: [],
  typing: false,
  seq: 0,
  currentAssistantId: null,
  currentReasoningId: null,
  seenImages: [],
};

export type ChatAction =
  | { type: "newChat" }
  | { type: "loadHistory"; ndjson: string }
  | { type: "userSend"; text: string }
  | { type: "event"; event: AgentEvent }
  | { type: "setApproval"; callId: string; status: "approved" | "denied" }
  | { type: "error"; text: string }
  | { type: "stopTyping" };

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
      return { ...state, items, seq, typing: true, currentAssistantId: null, currentReasoningId: null };
    }
    case "stopTyping":
      return state.typing ? { ...state, typing: false } : state;
    case "error": {
      let seq = state.seq;
      const items = [...state.items, { kind: "error", id: String(seq++), text: action.text } as TranscriptItem];
      return { ...state, items, seq, typing: false };
    }
    case "setApproval":
      // A decision resumes the agent, so the working dots come back on.
      return {
        ...state,
        typing: true,
        items: state.items.map((it) =>
          it.kind === "approval" && it.callId === action.callId ? { ...it, status: action.status } : it
        ),
      };
    case "event":
      return applyEvent(state, action.event);
  }
}

function applyEvent(state: ChatState, event: AgentEvent): ChatState {
  switch (event.kind) {
    // SessionId is handled by the store; Info/RawLine are intentionally dropped
    // (matches the old behavior).
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
      const items = [
        ...state.items,
        {
          kind: "approval",
          id: String(seq++),
          callId: event.tool_call_id,
          toolName: event.tool_name,
          toolArgs: event.tool_args,
          status: "pending",
        } as TranscriptItem,
      ];
      return { ...state, items, seq, typing: false };
    }
    case "AgentError": {
      let seq = state.seq;
      const items = [...state.items, { kind: "error", id: String(seq++), text: event.message } as TranscriptItem];
      return { ...state, items, seq, typing: false };
    }
    case "Done":
      return { ...finalizeTools(state), typing: false, currentAssistantId: null };
    case "Cancelled": {
      const finalized = finalizeTools(state);
      let seq = finalized.seq;
      const items = [...finalized.items, { kind: "cancelled", id: String(seq++) } as TranscriptItem];
      return { ...finalized, items, seq, typing: false, currentAssistantId: null };
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
  let currentReasoningId = state.currentReasoningId;

  // `typing` (the working dots) is left untouched here: it stays on through
  // streamed reasoning/content/tool deltas and is only cleared by a terminal
  // event (Done/Cancelled/ApprovalRequest/error).

  if (event.reasoning_content) {
    const text = event.reasoning_content;
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
  }

  if (event.content) {
    const text = event.content;
    currentReasoningId = null;
    if (currentAssistantId) {
      items = items.map((it) =>
        it.kind === "assistant" && it.id === currentAssistantId ? { ...it, chunks: [...it.chunks, text] } : it
      );
    } else {
      const id = String(seq++);
      items = [...items, { kind: "assistant", id, chunks: [text] }];
      currentAssistantId = id;
    }
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
    currentReasoningId = null;
  }

  return { ...state, items, seq, currentAssistantId, currentReasoningId };
}

function applyToolResult(state: ChatState, callId: string, content: string): ChatState {
  let seq = state.seq;
  let seenImages = state.seenImages;
  const parsed = parseToolResult(content);
  const idx = state.items.findIndex((it) => it.kind === "tool" && it.callId === callId);

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

  const src = parsed ? safeImageSrc(parsed.imagePath) : null;
  if (src && parsed?.imagePath) {
    const file = imageFilename(parsed.imagePath);
    if (!seenImages.includes(file)) {
      seenImages = [...seenImages, file];
      items = [...items, { kind: "image", id: String(seq++), src, filename: file }];
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
    items.push({ kind: "image", id: String(seq++), src, filename: file });
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
