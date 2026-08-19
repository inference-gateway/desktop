import { expect, test } from "bun:test";
import { chatReducer, initialChatState, type ChatState } from "./transcript";
import type { AgentEvent } from "./tauri";

const ev = (event: AgentEvent): { type: "event"; event: AgentEvent } => ({ type: "event", event });

function run(actions: Parameters<typeof chatReducer>[1][]): ChatState {
  return actions.reduce(chatReducer, initialChatState);
}

test("userSend adds a user bubble and starts the typing indicator", () => {
  const s = run([{ type: "userSend", text: "hi" }]);
  expect(s.items).toHaveLength(1);
  expect(s.items[0]).toMatchObject({ kind: "user", text: "hi" });
  expect(s.typing).toBe(true);
});

test("assistant content chunks accumulate into one bubble, then reset on tool call", () => {
  const s = run([
    { type: "userSend", text: "hi" },
    ev({ kind: "AssistantMessage", content: "Hel", reasoning_content: null, tool_calls: [] }),
    ev({ kind: "AssistantMessage", content: "lo", reasoning_content: null, tool_calls: [] }),
  ]);
  const assistant = s.items.filter((i) => i.kind === "assistant");
  expect(assistant).toHaveLength(1);
  expect(assistant[0]).toMatchObject({ chunks: ["Hel", "lo"] });
  // Dots persist while the agent streams; only a terminal event stops them.
  expect(s.typing).toBe(true);
});

test("streamed reasoning deltas accumulate into one block and keep the dots on", () => {
  const s = run([
    { type: "userSend", text: "think" },
    ev({ kind: "AssistantMessage", content: "", reasoning_content: "Let me ", tool_calls: [] }),
    ev({ kind: "AssistantMessage", content: "", reasoning_content: "think.", tool_calls: [] }),
  ]);
  const reasoning = s.items.filter((i) => i.kind === "reasoning");
  expect(reasoning).toHaveLength(1);
  if (reasoning[0]?.kind === "reasoning") {
    expect(reasoning[0].paragraphs).toEqual(["Let me ", "think."]);
  }
  expect(s.typing).toBe(true);
});

test("dots persist through reasoning then content and only stop on Done", () => {
  let s = run([
    { type: "userSend", text: "go" },
    ev({ kind: "AssistantMessage", content: "", reasoning_content: "reasoning", tool_calls: [] }),
    ev({ kind: "AssistantMessage", content: "answer", reasoning_content: null, tool_calls: [] }),
  ]);
  expect(s.typing).toBe(true);
  s = chatReducer(s, ev({ kind: "Done", exit_code: 0, stderr: "" }));
  expect(s.typing).toBe(false);
});

test("approval decision turns the working dots back on", () => {
  let s = run([
    { type: "userSend", text: "write" },
    ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: "{}", tool_call_id: "c1" }),
  ]);
  expect(s.typing).toBe(false);
  s = chatReducer(s, { type: "setApproval", callId: "c1", status: "approved" });
  expect(s.typing).toBe(true);
});

test("a running tool call resolves by tool_call_id", () => {
  const s = run([
    { type: "userSend", text: "write" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "Write", args: '{"path":"a"}' }],
    }),
    ev({ kind: "ToolResult", tool_call_id: "c1", content: '{"tool_name":"Write","data":{"output":"ok"},"success":true}' }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({ callId: "c1", state: "done", output: "ok", skeleton: false });
  expect(s.currentAssistantId).toBeNull();
});

test("approval request then resolution flips status", () => {
  let s = run([
    ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: "{}", tool_call_id: "c9" }),
  ]);
  expect(s.items[0]).toMatchObject({ kind: "approval", status: "pending" });
  s = chatReducer(s, { type: "setApproval", callId: "c9", status: "approved" });
  expect(s.items[0]).toMatchObject({ kind: "approval", status: "approved" });
});

test("Done stops typing and finalizes running tools", () => {
  const s = run([
    { type: "userSend", text: "x" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "ImageGeneration", args: "{}" }],
    }),
    ev({ kind: "Done", exit_code: 0, stderr: "" }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({ state: "done", skeleton: false });
  expect(s.typing).toBe(false);
});

test("AgentError before ApprovalRequest puts approval before the error", () => {
      const s = run([
        { type: "userSend", text: "write" },
        ev({
          kind: "AssistantMessage",
          content: "",
          reasoning_content: null,
          tool_calls: [{ id: "c1", name: "Write", args: '{"path":"a"}' }],
        }),
        ev({ kind: "AgentError", message: "something went wrong" }),
        ev({
          kind: "ApprovalRequest",
          tool_name: "Write",
          tool_args: '{"path":"a"}',
          tool_call_id: "c1",
        }),
      ]);
      expect(s.items.map((i) => i.kind)).toEqual([
        "user",
        "tool",
        "approval",
        "error",
      ]);
});

test("ApprovalRequest before AgentError keeps the natural order", () => {
      const s = run([
        { type: "userSend", text: "write" },
        ev({
          kind: "AssistantMessage",
          content: "",
          reasoning_content: null,
          tool_calls: [{ id: "c1", name: "Write", args: '{"path":"a"}' }],
        }),
        ev({
          kind: "ApprovalRequest",
          tool_name: "Write",
          tool_args: '{"path":"a"}',
          tool_call_id: "c1",
        }),
        ev({ kind: "AgentError", message: "something went wrong" }),
      ]);
      expect(s.items.map((i) => i.kind)).toEqual([
        "user",
        "tool",
        "approval",
        "error",
      ]);
});

test("loadHistory rebuilds user/assistant/tool items from NDJSON", () => {
  const ndjson = [
    JSON.stringify({ role: "user", content: "hello" }),
    JSON.stringify({ role: "assistant", content: "hi there", reasoning_content: "thinking" }),
    JSON.stringify({ role: "tool", content: '{"tool_name":"Read","data":{"output":"file"},"success":true}' }),
  ].join("\n");
  const s = chatReducer(initialChatState, { type: "loadHistory", ndjson });
  expect(s.items.map((i) => i.kind)).toEqual(["user", "reasoning", "assistant", "tool"]);
});

test("computer-use tool call renders through the generic tool path", () => {
  const s = run([
    { type: "userSend", text: "click it" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "MouseClick", args: '{"x":100,"y":200}' }],
    }),
    ev({ kind: "ToolResult", tool_call_id: "c1", content: '{"tool_name":"MouseClick","data":{"output":"clicked"},"success":true}' }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({ callId: "c1", name: "MouseClick", state: "done", output: "clicked" });
});

test("computer-use pause and resume toggle the paused flag", () => {
  let s = run([{ type: "userSend", text: "go" }, ev({ kind: "ComputerUsePaused" })]);
  expect(s.paused).toBe(true);
  s = chatReducer(s, ev({ kind: "ComputerUseResumed" }));
  expect(s.paused).toBe(false);
  s = chatReducer(s, ev({ kind: "ComputerUsePaused" }));
  s = chatReducer(s, ev({ kind: "Done", exit_code: 0, stderr: "" }));
  expect(s.paused).toBe(false);
});
