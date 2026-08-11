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
  | { kind: "TokenUsage"; input: number; output: number; cached_read: number; total_tool_calls: number }
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
export type DesktopConfig = {
  storage_backend: string;
  storage_directory: string;
  gateway_url: string;
  default_model: string;
  sqlite_path: string;
  postgres_host: string;
  postgres_port: string;
  postgres_database: string;
  postgres_username: string;
  postgres_password: string;
  postgres_ssl_mode: string;
  redis_host: string;
  redis_port: string;
  redis_password: string;
  redis_db: string;
  d1_account_id: string;
  d1_database_id: string;
  d1_api_token: string;
  d1_base_url: string;
  extra_instructions: string;
  system_prompt: string;
};
export type HistoryLine = {
  role: "user" | "assistant" | "tool";
  content?: string;
  reasoning_content?: string | null;
};

export type StoredSpan = {
  trace_id: string;
  span_id: string;
  parent_span_id: string;
  name: string;
  kind: number;
  start_time_unix_nano: number;
  end_time_unix_nano: number;
  attributes: [string, string][];
  status_code: number;
  service_name: string;
};

export type StoredMetric = {
  name: string;
  unit: string;
  value: number;
  count: number;
  attributes: [string, string][];
  time_unix_nano: number;
};

export const api = {
  checkAndInstallCli: (onEvent: Channel<ProgressEvent>, force = false) =>
    invoke<void>("check_and_install_cli", { onEvent, force }),
  sendMessage: (args: {
    prompt: string;
    model: string;
    sessionId: string;
    onEvent: Channel<AgentEvent>;
    systemPrompt?: string;
    extraInstructions?: string;
  }) => invoke<string | null>("send_message", args),
  sendApproval: (sessionId: string, toolCallId: string, approved: boolean) =>
    invoke<void>("send_approval", { sessionId, toolCallId, approved }),
  cancelAgent: (sessionId: string) => invoke<void>("cancel_agent", { sessionId }),
  listConversations: () => invoke<string>("list_conversations"),
  getConversation: (sessionId: string) => invoke<string>("get_conversation", { sessionId }),
  deleteConversation: (sessionId: string) => invoke<void>("delete_conversation", { sessionId }),
  listModels: () => invoke<string[]>("list_models"),
  getAuth: () => invoke<Record<string, string>>("get_auth"),
  setAuth: (keys: Record<string, string>) => invoke<void>("set_auth", { keys }),
  getConfig: () => invoke<DesktopConfig>("get_config"),
  setConfig: (cfg: DesktopConfig) => invoke<void>("set_config", { cfg }),
  setDefaultModel: (model: string) => invoke<void>("set_default_model", { model }),
  startGateway: (force = false) => invoke<void>("start_gateway", { force }),
  checkUpdates: () => invoke<UpdateInfo[]>("check_updates"),
  installDesktopUpdate: () => invoke<void>("install_desktop_update"),
  sttStatus: () => invoke<SttStatus>("stt_status"),
  prepareStt: (onEvent: Channel<ProgressEvent>) => invoke<void>("prepare_stt", { onEvent }),
  transcribeAudio: (wav: number[]) => invoke<string>("transcribe_audio", { wav }),
  readHistory: () => invoke<string[]>("read_history"),
  appendHistory: (line: string) => invoke<void>("append_history", { line }),
  readProjects: () => invoke<string>("read_projects"),
  writeProjects: (data: string) => invoke<void>("write_projects", { data }),
  saveImage: (path: string) => invoke<string>("save_image", { path }),
  getTraces: () => invoke<StoredSpan[]>("get_traces"),
  getMetrics: () => invoke<StoredMetric[]>("get_metrics"),
  listA2aAgents: () => invoke<A2aAgent[]>("list_a2a_agents"),
  addA2aAgent: (name: string, url: string) => invoke<void>("add_a2a_agent", { name, url }),
  removeA2aAgent: (name: string) => invoke<void>("remove_a2a_agent", { name }),
  setA2aAgentModel: (name: string, model: string) =>
    invoke<void>("set_a2a_agent_model", { name, model }),
};
