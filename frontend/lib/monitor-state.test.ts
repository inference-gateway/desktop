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
      tool_calls: [{ id: "c1", name: "Computer", args: '{"action":"click","x":1}' }],
    }),
  ]);
  expect(s.s1).toMatchObject({ status: "running", log: ['▸ Computer {"action":"click","x":1}'] });
});

test("text deltas append to the last text entry, actions start new entries", () => {
  const text = (content: string): AgentEvent => ({
    kind: "AssistantMessage",
    content,
    reasoning_content: null,
    tool_calls: [],
  });
  const s = run([
    msg("s1", {
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "GetLatestFrame", args: "{}" }],
    }),
    msg("s1", text("Opening ")),
    msg("s1", text("the app.")),
    msg("s1", {
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c2", name: "Computer", args: '{"action":"click","x":1}' }],
    }),
    msg("s1", text("Done.")),
  ]);
  expect(s.s1.log).toEqual([
    "▸ GetLatestFrame {}",
    "Opening the app.",
    '▸ Computer {"action":"click","x":1}',
    "Done.",
  ]);
});

test("tool result with a base64 frame or image path sets lastFrame", () => {
  const start = msg("s1", {
    kind: "AssistantMessage",
    content: "",
    reasoning_content: null,
    tool_calls: [{ id: "c1", name: "GetLatestFrame", args: "{}" }],
  } as AgentEvent);
  const frame = JSON.stringify({
    tool_name: "GetLatestFrame",
    success: true,
    data: { source: "screen", width: 1024, height: 768 },
    images: [{ data: "aGVsbG8=", mime_type: "image/jpeg", display_name: "frame-screen" }],
  });
  const generated = JSON.stringify({
    tool_name: "ImageGeneration",
    success: true,
    data: { output: "saved", path: "/Users/x/.infer/tmp/frame.png" },
  });
  let s = run([
    start,
    msg("s1", { kind: "ToolResult", content: frame, tool_call_id: "c1" }),
    msg("s1", { kind: "ToolResult", content: "not json", tool_call_id: "c2" }),
  ]);
  expect(s.s1.lastFrame).toBe("data:image/jpeg;base64,aGVsbG8=");
  s = run([start, msg("s1", { kind: "ToolResult", content: generated, tool_call_id: "c3" })]);
  expect(s.s1.lastFrame).toBe("/Users/x/.infer/tmp/frame.png");
});

test("log is capped at 100 entries", () => {
  const click = (i: number): AgentEvent => ({
    kind: "AssistantMessage",
    content: "",
    reasoning_content: null,
    tool_calls: [{ id: `c${i}`, name: "Computer", args: `{"action":"click","x":${i}}` }],
  });
  const s = run(Array.from({ length: 120 }, (_, i) => msg("s1", click(i))));
  expect(s.s1.log).toHaveLength(100);
  expect(s.s1.log[99]).toBe('▸ Computer {"action":"click","x":119}');
  expect(s.s1.log[0]).toBe('▸ Computer {"action":"click","x":20}');
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
      tool_calls: [{ id: "c1", name: "Computer", args: "{}" }],
    }),
    msg("s1", {
      kind: "ApprovalRequest",
      tool_name: "Computer",
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
