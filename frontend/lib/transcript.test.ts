import { expect, test } from "bun:test";
import {
  chatReducer,
  delegationsFrom,
  initialChatState,
  subagentParentId,
  type ChatState,
  type TranscriptItem,
} from "./transcript";
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

test("distinct streamed assistant messages start new bubbles", () => {
  const s = run([
    { type: "userSend", text: "hi" },
    ev({
      kind: "AssistantMessage",
      content: "First ",
      reasoning_content: null,
      tool_calls: [],
      message_id: "m1",
    }),
    ev({
      kind: "AssistantMessage",
      content: "message.",
      reasoning_content: null,
      tool_calls: [],
      message_id: "m1",
    }),
    ev({
      kind: "AssistantMessage",
      content: "Second message.",
      reasoning_content: null,
      tool_calls: [],
      message_id: "m2",
    }),
  ]);
  const assistant = s.items.filter((i) => i.kind === "assistant");
  expect(assistant).toHaveLength(2);
  expect(assistant[0]).toMatchObject({ chunks: ["First ", "message."] });
  expect(assistant[1]).toMatchObject({ chunks: ["Second message."] });
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
    ev({
      kind: "ToolResult",
      tool_call_id: "c1",
      content: '{"tool_name":"Write","data":{"output":"ok"},"success":true}',
    }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({ callId: "c1", state: "done", output: "ok", skeleton: false });
  expect(s.currentAssistantId).toBeNull();
});

test("a TextToSpeech result with a tts wav path adds an inline audio item", () => {
  (globalThis as Record<string, unknown>).window = {
    __TAURI_INTERNALS__: { convertFileSrc: (p: string) => `asset://localhost/${p}` },
  };
  try {
    const s = run([
      { type: "userSend", text: "speak" },
      ev({
        kind: "AssistantMessage",
        content: "",
        reasoning_content: null,
        tool_calls: [{ id: "c1", name: "TextToSpeech", args: '{"text":"hi"}' }],
      }),
      ev({
        kind: "ToolResult",
        tool_call_id: "c1",
        content:
          '{"tool_name":"TextToSpeech","data":{"path":"/Users/x/.infer/tts/speech-20260102-150405-123.wav","text":"hi","duration_seconds":1.5},"success":true}',
      }),
    ]);
    const audio = s.items.find((i) => i.kind === "audio");
    expect(audio).toMatchObject({
      kind: "audio",
      filename: "speech-20260102-150405-123.wav",
      path: "/Users/x/.infer/tts/speech-20260102-150405-123.wav",
    });
    // Non-tts tool results never produce audio items.
    const other = run([
      ev({
        kind: "ToolResult",
        tool_call_id: "c2",
        content: '{"tool_name":"Write","data":{"path":"/Users/x/.infer/tmp/out.wav"},"success":true}',
      }),
    ]);
    expect(other.items.some((i) => i.kind === "audio")).toBe(false);
  } finally {
    delete (globalThis as Record<string, unknown>).window;
  }
});

test("a failed tool call retains its top-level error", () => {
  const s = run([
    { type: "userSend", text: "read" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "Read", args: '{"file_path":"/outside/file"}' }],
    }),
    ev({
      kind: "ToolResult",
      tool_call_id: "c1",
      content: '{"tool_name":"Read","success":false,"error":"path is outside configured sandbox directories"}',
    }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({
    callId: "c1",
    state: "failed",
    output: "path is outside configured sandbox directories",
  });
});

test("a mismatched tool_call_id attaches to the unresolved tool card instead of duplicating", () => {
  const s = run([
    { type: "userSend", text: "read" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "Read", args: '{"file_path":"a"}' }],
    }),
    ev({
      kind: "ToolResult",
      tool_call_id: "",
      content: '{"tool_name":"Read","data":{"output":"file"},"success":true}',
    }),
  ]);
  const tools = s.items.filter((i) => i.kind === "tool");
  expect(tools).toHaveLength(1);
  expect(tools[0]).toMatchObject({ callId: "c1", state: "done", output: "file" });
});

test("Done expires pending approvals so their buttons go away", () => {
  const s = run([
    { type: "userSend", text: "capture" },
    ev({ kind: "ApprovalRequest", tool_name: "Bash", tool_args: "{}", tool_call_id: "c1" }),
    ev({ kind: "Done", exit_code: 0, stderr: "" }),
  ]);
  const approval = s.items.find((i) => i.kind === "approval");
  expect(approval).toMatchObject({ status: "expired" });
});

test("approval request then resolution flips status", () => {
  let s = run([ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: "{}", tool_call_id: "c9" })]);
  expect(s.items[0]).toMatchObject({ kind: "approval", status: "pending" });
  s = chatReducer(s, { type: "setApproval", callId: "c9", status: "approved" });
  expect(s.items[0]).toMatchObject({ kind: "approval", status: "approved" });
});

test("a duplicate setApproval is a no-op and cannot restart the dots", () => {
  let s = run([
    { type: "userSend", text: "write" },
    ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: "{}", tool_call_id: "c1" }),
    { type: "setApproval", callId: "c1", status: "approved" },
    ev({ kind: "Done", exit_code: 0, stderr: "" }),
  ]);
  expect(s.typing).toBe(false);
  const echoed = chatReducer(s, { type: "setApproval", callId: "c1", status: "approved" });
  expect(echoed).toBe(s);
});

test("setApproval for an unknown callId leaves state untouched", () => {
  const s = run([{ type: "userSend", text: "write" }]);
  expect(chatReducer(s, { type: "setApproval", callId: "nope", status: "denied" })).toBe(s);
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
  expect(s.items.map((i) => i.kind)).toEqual(["user", "tool", "approval", "error"]);
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
  expect(s.items.map((i) => i.kind)).toEqual(["user", "tool", "approval", "error"]);
});

test("approval record renders above its tool card once the result arrives", () => {
  const actions = (assistantFirst: boolean) => [
    { type: "userSend", text: "write" },
    ...(assistantFirst
      ? [
          ev({
            kind: "AssistantMessage",
            content: "",
            reasoning_content: null,
            tool_calls: [{ id: "c1", name: "Write", args: '{"path":"a"}' }],
          }),
          ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: '{"path":"a"}', tool_call_id: "c1" }),
        ]
      : [ev({ kind: "ApprovalRequest", tool_name: "Write", tool_args: '{"path":"a"}', tool_call_id: "c1" })]),
    { type: "setApproval", callId: "c1", status: "approved" as const },
    ...(assistantFirst
      ? []
      : [
          ev({
            kind: "AssistantMessage",
            content: "",
            reasoning_content: null,
            tool_calls: [{ id: "c1", name: "Write", args: '{"path":"a"}' }],
          }),
        ]),
    ev({ kind: "ToolResult", tool_call_id: "c1", content: '{"tool_name":"Write","success":false,"error":"nope"}' }),
  ];
  for (const assistantFirst of [false, true]) {
    const s = run(actions(assistantFirst));
    expect(s.items.map((i) => i.kind)).toEqual(["user", "approval", "tool"]);
    expect(s.items[1]).toMatchObject({ status: "approved", toolName: "Write" });
    expect(s.items[2]).toMatchObject({ callId: "c1", state: "failed" });
  }
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

test("loadHistory unwraps the CLI v2 entry envelope and skips meta/system-reminder lines", () => {
  const wrap = (message: object) => JSON.stringify({ v: 2, type: "entry", index: 0, entry: { message } });
  const ndjson = [
    wrap({ role: "user", content: "hello" }),
    JSON.stringify({ type: "meta", metadata: { id: "x" } }),
    wrap({ role: "user", content: "<system-reminder>\ninjected context\n</system-reminder>" }),
    wrap({ role: "assistant", content: "hi there", reasoning_content: "thinking" }),
    wrap({ role: "tool", content: '{"tool_name":"Read","data":{"output":"file"},"success":true}' }),
  ].join("\n");
  const s = chatReducer(initialChatState, { type: "loadHistory", ndjson });
  expect(s.items.map((i) => i.kind)).toEqual(["user", "reasoning", "assistant", "tool"]);
});

test("loadHistory recovers audio players from pretty-printed v2 tool results", () => {
  const content =
    "TextToSpeech(text=hi)\n╰── Result:\n    Speech saved to /Users/me/.infer/tts/speech-1.wav (1.0s of audio)";
  const ndjson = JSON.stringify({ v: 2, type: "entry", index: 0, entry: { message: { role: "tool", content } } });
  const s = chatReducer(initialChatState, { type: "loadHistory", ndjson });
  expect(s.items.map((i) => i.kind)).toEqual(["tool", "audio"]);
  expect(s.items[1]).toMatchObject({
    kind: "audio",
    filename: "speech-1.wav",
    path: "/Users/me/.infer/tts/speech-1.wav",
  });
});

test("computer-use tool call renders through the generic tool path", () => {
  const s = run([
    { type: "userSend", text: "click it" },
    ev({
      kind: "AssistantMessage",
      content: "",
      reasoning_content: null,
      tool_calls: [{ id: "c1", name: "Computer", args: '{"action":"click","x":100,"y":200}' }],
    }),
    ev({
      kind: "ToolResult",
      tool_call_id: "c1",
      content: '{"tool_name":"Computer","data":{"output":"clicked"},"success":true}',
    }),
  ]);
  const tool = s.items.find((i) => i.kind === "tool");
  expect(tool).toMatchObject({ callId: "c1", name: "Computer", state: "done", output: "clicked" });
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

const toolItem = (name: string, args: string, state: "running" | "done" | "failed" = "running"): TranscriptItem => ({
  kind: "tool",
  id: "1",
  callId: "c1",
  name,
  args,
  output: null,
  state,
  skeleton: false,
});

test("delegationsFrom expands a running Agent tasks array into subagent rows", () => {
  const args = JSON.stringify({
    tasks: [
      { description: "map the repo", label: "repo-overview" },
      { description: "a very long description that should be truncated because it exceeds the label limit" },
    ],
  });
  const d = delegationsFrom([toolItem("Agent", args)]);
  expect(d).toHaveLength(2);
  expect(d[0]).toMatchObject({ label: "repo-overview", kind: "subagent" });
  expect(d[1].kind).toBe("subagent");
  expect(d[1].label.endsWith("…")).toBe(true);
});

test("delegationsFrom handles single-task Agent args and malformed args", () => {
  expect(delegationsFrom([toolItem("Agent", '{"description":"one task"}')])).toMatchObject([
    { label: "one task", kind: "subagent" },
  ]);
  expect(delegationsFrom([toolItem("Agent", "not json")])).toMatchObject([{ label: "agent", kind: "subagent" }]);
});

test("delegationsFrom maps A2A tool calls to a2a rows", () => {
  expect(delegationsFrom([toolItem("A2A_QueryAgent", '{"agent":"docs-agent"}')])).toMatchObject([
    { label: "docs-agent", kind: "a2a" },
  ]);
  expect(delegationsFrom([toolItem("A2A_SubmitTask", "{}")])).toMatchObject([{ label: "SubmitTask", kind: "a2a" }]);
});

test("delegationsFrom ignores finished delegations and unrelated tools", () => {
  expect(delegationsFrom([toolItem("Agent", '{"tasks":[{"label":"x"}]}', "done")])).toHaveLength(0);
  expect(delegationsFrom([toolItem("Read", '{"path":"a"}')])).toHaveLength(0);
});

test("subagentParentId extracts the orchestrator id from subagent session ids", () => {
  expect(subagentParentId("subagent-28f1b14b-7b10-4950-9487-8c0e10bf4917-464d7ff2-5da8-4d97-bedb-a3c86e78daff")).toBe(
    "28f1b14b-7b10-4950-9487-8c0e10bf4917",
  );
  expect(subagentParentId("28f1b14b-7b10-4950-9487-8c0e10bf4917")).toBeNull();
  expect(subagentParentId("subagent-not-a-uuid")).toBeNull();
});
