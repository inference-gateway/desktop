// Pure state machine for the computer-use monitor window. Consumes the
// "agent-event" re-broadcast from the main window and keeps a compact
// per-session summary - just enough for a status strip, not a transcript.
import type { AgentEvent } from "./tauri";
import { parseToolResult } from "./tools";
import { COMPUTER_USE_TOOLS } from "./transcript";

export type MonitorSession = {
  name: string;
  status: "running" | "paused" | "awaiting" | "done";
  log: string[];
  currentMessageId: string | null;
  lastFrame: string | null;
  pendingApproval: { callId: string; toolName: string; toolArgs: string } | null;
};

// ponytail: flat string log, "▸ "-prefixed entries are actions; go structured if per-entry styling is ever needed.
const LOG_CAP = 100;
const ACTION_PREFIX = "▸ ";

export type MonitorState = Record<string, MonitorSession>;

export type MonitorEvent = { sessionId: string; name: string; event: AgentEvent };

export function monitorReducer(state: MonitorState, msg: MonitorEvent): MonitorState {
  const { sessionId, name, event } = msg;
  const session: MonitorSession = state[sessionId] ?? {
    name,
    status: "running",
    log: [],
    currentMessageId: null,
    lastFrame: null,
    pendingApproval: null,
  };
  switch (event.kind) {
    case "AssistantMessage": {
      const cu = event.tool_calls.filter((tc) => COMPUTER_USE_TOOLS.has(tc.name));
      if (!state[sessionId] && !cu.length) return state;
      if (!cu.length && !event.content) return state;
      let log = session.log;
      let currentMessageId = session.currentMessageId;
      if (event.content) {
        const last = log[log.length - 1];
        const sameMessage = !event.message_id || !currentMessageId || event.message_id === currentMessageId;
        log =
          last !== undefined && !last.startsWith(ACTION_PREFIX) && sameMessage
            ? [...log.slice(0, -1), last + event.content]
            : [...log, event.content];
        if (event.message_id) currentMessageId = event.message_id;
      }
      if (cu.length) {
        log = [...log, ...cu.map((tc) => `${ACTION_PREFIX}${tc.name} ${tc.args}`.trimEnd())];
        currentMessageId = null;
      }
      return {
        ...state,
        [sessionId]: {
          ...session,
          status: cu.length ? "running" : session.status,
          log: log.slice(-LOG_CAP),
          currentMessageId,
        },
      };
    }
    case "ToolResult": {
      if (!state[sessionId]) return state;
      const parsed = parseToolResult(event.content);
      const frame = parsed?.imageData ?? parsed?.imagePath;
      if (!frame) return state;
      return { ...state, [sessionId]: { ...session, lastFrame: frame } };
    }
    case "ApprovalRequest":
      if (!state[sessionId] && !COMPUTER_USE_TOOLS.has(event.tool_name)) return state;
      return {
        ...state,
        [sessionId]: {
          ...session,
          status: "awaiting",
          pendingApproval: {
            callId: event.tool_call_id,
            toolName: event.tool_name,
            toolArgs: event.tool_args,
          },
        },
      };
    case "ComputerUsePaused":
      if (!state[sessionId]) return state;
      return { ...state, [sessionId]: { ...session, status: "paused" } };
    case "ComputerUseResumed":
      if (!state[sessionId]) return state;
      return { ...state, [sessionId]: { ...session, status: "running", pendingApproval: null } };
    case "Done":
    case "Cancelled":
    case "AgentError": {
      if (!state[sessionId]) return state;
      return { ...state, [sessionId]: { ...session, status: "done", pendingApproval: null } };
    }
    default:
      return state;
  }
}

export function resolveApproval(state: MonitorState, sessionId: string): MonitorState {
  const session = state[sessionId];
  if (!session) return state;
  return { ...state, [sessionId]: { ...session, status: "running", pendingApproval: null } };
}
