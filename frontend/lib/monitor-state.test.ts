import { expect, test } from "bun:test";
import { monitorReducer, resolveApproval, type MonitorState } from "./monitor-state";
import type { AgentEvent } from "./tauri";

const msg = (sessionId: string, event: AgentEvent, name = "click the button") => ({
  sessionId,
  name,
  event,
});

function run(events: Parameters<typeof monitorReducer>[1][]): MonitorState {
  return events.reduce(monitorReducer, {});
}

test("computer-use tool call creates a session with the last action", () => {
  const s = run([
    msg("s1", {
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "MouseClick", args: '{"x":1}' }],
    }),
  ]);
  expect(s.s1).toMatchObject({ status: "running", lastAction: 'MouseClick {"x":1}' });
});

test("non-computer-use events from unknown sessions are ignored", () => {
  const s = run([
    msg("s1", {
      kind: "AssistantMessage",
      content: "hi",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "Write", args: "{}" }],
    }),
    msg("s1", { kind: "ApprovalRequest", tool_name: "Write", tool_args: "{}", tool_call_id: "c1" }),
  ]);
  expect(s).toEqual({});
});

test("approval, pause, resume, and done drive the session status", () => {
  let s = run([
    msg("s1", {
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "MouseClick", args: "{}" }],
    }),
    msg("s1", {
      kind: "ApprovalRequest",
      tool_name: "MouseClick",
      tool_args: "{}",
      tool_call_id: "c1",
    }),
  ]);
  expect(s.s1).toMatchObject({ status: "awaiting", pendingApproval: { callId: "c1" } });
  s = resolveApproval(s, "s1");
  expect(s.s1).toMatchObject({ status: "running", pendingApproval: null });
  s = monitorReducer(s, msg("s1", { kind: "ComputerUsePaused" }));
  expect(s.s1.status).toBe("paused");
  s = monitorReducer(s, msg("s1", { kind: "ComputerUseResumed" }));
  expect(s.s1.status).toBe("running");
  s = monitorReducer(s, msg("s1", { kind: "Done", exit_code: 0, stderr: "" }));
  expect(s.s1.status).toBe("done");
});
