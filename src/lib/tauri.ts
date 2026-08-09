// Typed wrapper around the Rust backend. Command args are camelCase (Tauri
// camelCases them); event/return payloads keep their Rust serde snake_case
// names, so the discriminated unions below use snake_case fields.
import { invoke, Channel, convertFileSrc } from "@tauri-apps/api/core";

export { Channel, convertFileSrc };

export type ToolCallInfo = { id: string; name: string; args: string };

export type AgentEvent =
  | { kind: "SessionId"; session_id: string }
  | {
      kind: "AssistantMessage";
      content: string;
      reasoning_content: string | null;
      tool_calls: ToolCallInfo[];
    }
  | { kind: "ToolResult"; content: string; tool_call_id: string }
  | { kind: "ApprovalRequest"; tool_name: string; tool_args: string; tool_call_id: string }
  | { kind: "Info"; message: string }
  | { kind: "AgentError"; message: string }
  | { kind: "RawLine"; line: string }
  | { kind: "Done"; exit_code: number; stderr: string }
  | { kind: "Cancelled" };

export type ProgressEvent =
  | { kind: "Checking" }
  | { kind: "Downloading"; received: number; total: number }
  | { kind: "Verifying" }
  | { kind: "Installing" }
  | { kind: "Initializing" }
  | { kind: "Ready" }
  | { kind: "Error"; message: string };

export type UpdateInfo = { name: string; current: string; latest: string | null; outdated: boolean };
export type SttStatus = { binary: boolean; model: boolean; downloadable: boolean; hint: string };
export type Conversation = { id: string; title?: string | null };
export type A2aAgent = { name: string; url: string; run: boolean; model: string };
export type HistoryLine = {
  role: "user" | "assistant" | "tool";
  content?: string;
  reasoning_content?: string | null;
};

export const api = {
  checkAndInstallCli: (onEvent: Channel<ProgressEvent>, force = false) =>
    invoke<void>("check_and_install_cli", { onEvent, force }),
  sendMessage: (args: {
    prompt: string;
    model: string;
    sessionId: string | null;
    onEvent: Channel<AgentEvent>;
  }) => invoke<string | null>("send_message", args),
  sendApproval: (toolCallId: string, approved: boolean) =>
    invoke<void>("send_approval", { toolCallId, approved }),
  cancelAgent: () => invoke<void>("cancel_agent"),
  listConversations: () => invoke<string>("list_conversations"),
  getConversation: (sessionId: string) => invoke<string>("get_conversation", { sessionId }),
  deleteConversation: (sessionId: string) => invoke<void>("delete_conversation", { sessionId }),
  listModels: () => invoke<string[]>("list_models"),
  getAuth: () => invoke<Record<string, string>>("get_auth"),
  setAuth: (keys: Record<string, string>) => invoke<void>("set_auth", { keys }),
  startGateway: (force = false) => invoke<void>("start_gateway", { force }),
  checkUpdates: () => invoke<UpdateInfo[]>("check_updates"),
  installDesktopUpdate: () => invoke<void>("install_desktop_update"),
  sttStatus: () => invoke<SttStatus>("stt_status"),
  prepareStt: (onEvent: Channel<ProgressEvent>) => invoke<void>("prepare_stt", { onEvent }),
  transcribeAudio: (wav: number[]) => invoke<string>("transcribe_audio", { wav }),
  readHistory: () => invoke<string[]>("read_history"),
  appendHistory: (line: string) => invoke<void>("append_history", { line }),
  listA2aAgents: () => invoke<A2aAgent[]>("list_a2a_agents"),
  addA2aAgent: (name: string, url: string) => invoke<void>("add_a2a_agent", { name, url }),
  removeA2aAgent: (name: string) => invoke<void>("remove_a2a_agent", { name }),
  setA2aAgentModel: (name: string, model: string) =>
    invoke<void>("set_a2a_agent_model", { name, model }),
};
