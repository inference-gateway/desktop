// Pure state machine for the computer-use monitor window. Consumes the
// "agent-event" re-broadcast from the main window and keeps a compact
// per-session summary - just enough for a status strip, not a transcript.
import type { AgentEvent } from "./tauri";
import { COMPUTER_USE_TOOLS } from "./transcript";

export type MonitorSession = {
  name: string;
  status: "running" | "paused" | "awaiting" | "done";
  lastAction: string | null;
  pendingApproval: { callId: string; toolName: string; toolArgs: string } | null;
};

export type MonitorState = Record<string, MonitorSession>;

export type MonitorEvent = { sessionId: string; name: string; event: AgentEvent };

export function monitorReducer(state: MonitorState, msg: MonitorEvent): MonitorState {
  const { sessionId, name, event } = msg;
  const session: MonitorSession = state[sessionId] ?? {
    name,
    status: "running",
    lastAction: null,
    pendingApproval: null,
  };
  switch (event.kind) {
    case "AssistantMessage": {
      const cu = event.tool_calls.filter((tc) => COMPUTER_USE_TOOLS.has(tc.name));
      if (!cu.length) return state;
      const last = cu[cu.length - 1];
      return {
        ...state,
        [sessionId]: { ...session, status: "running", lastAction: `${last.name} ${last.args}`.trim() },
      };
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
