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
      message_id?: string | null;
    }
  | { kind: "ToolResult"; content: string; tool_call_id: string }
  | { kind: "ApprovalRequest"; tool_name: string; tool_args: string; tool_call_id: string }
  | { kind: "Info"; message: string }
  | { kind: "AgentError"; message: string }
  | { kind: "RawLine"; line: string }
  | { kind: "Done"; exit_code: number; stderr: string }
  | { kind: "TokenUsage"; input: number; output: number; cached_read: number; total_tool_calls: number }
  | { kind: "Cancelled" }
  | { kind: "ComputerUsePaused" }
  | { kind: "ComputerUseResumed" };

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
  schedule_enabled: boolean;
  agent_model: string;
  scheduler_backend: string;
  scheduler_github_repository: string;
  scheduler_github_app_client_id_secret: string;
  scheduler_github_app_private_key_secret: string;
  scheduler_github_bot_name: string;
  scheduler_github_bot_email: string;
  scheduler_github_pull_requests: boolean;
  scheduler_github_artifacts_enabled: boolean;
  scheduler_github_artifacts_poll_interval: string;
  scheduler_github_artifacts_initial_delay: string;
  scheduler_github_artifacts_max_attempts: string;
  scheduler_github_artifacts_rate_limit_backoff: string;
  projects_root: string;
  projects_backend: string;
  projects_github_repository: string;
  projects_max_file_size_mb: string;
  projects_allowed_mimes: string;
};
export type GithubAuthStatus = { installed: boolean; authenticated: boolean };
export type OsPermissionState = "granted" | "not_granted" | "unavailable" | "not_applicable";
export type ComputerUsePermissionStatus = {
  computer_use_enabled: boolean;
  accessibility: OsPermissionState;
  screen_recording: OsPermissionState;
};
export type WorkflowStatus = {
  installed: boolean;
  url: string | null;
  sha: string | null;
  version: string | null;
  latest: string | null;
};
export type RepoEntry = {
  name: string;
  open: number;
};
export type TaskIssue = {
  number: number;
  title: string;
  state: string;
  html_url: string;
  created_at: string;
};
export type TaskPull = {
  number: number;
  title: string;
  html_url: string;
  body: string | null;
};
export type WorkflowRun = {
  id: number;
  name: string;
  status: string;
  conclusion: string | null;
  html_url: string;
  created_at: string;
};
export type ScheduleJob = {
  id: string;
  name: string;
  description: string;
  cron_expression: string;
  prompt: string;
  run_once: boolean;
  last_run: string;
  last_error: string;
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
    autoMode: boolean;
  }) => invoke<string | null>("send_message", args),
  sendApproval: (sessionId: string, toolCallId: string, approved: boolean, scope?: "always") =>
    invoke<void>("send_approval", { sessionId, toolCallId, approved, scope: scope ?? null }),
  sendComputerUseControl: (sessionId: string, action: "pause" | "resume") =>
    invoke<void>("send_computer_use_control", { sessionId, action }),
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
  startScheduler: () => invoke<void>("start_scheduler"),
  stopScheduler: () => invoke<void>("stop_scheduler"),
  getSchedulerStatus: () => invoke<boolean>("get_scheduler_status"),
  getSchedulerLog: () => invoke<string[]>("get_scheduler_log"),
  listSchedules: () => invoke<ScheduleJob[]>("list_schedules"),
  computerUsePermissionStatus: () =>
    invoke<ComputerUsePermissionStatus>("computer_use_permission_status"),
  setComputerUseEnabled: (enabled: boolean) =>
    invoke<void>("set_computer_use_enabled", { enabled }),
  requestAccessibilityPermission: () => invoke<void>("request_accessibility_permission"),
  requestScreenRecordingPermission: () => invoke<void>("request_screen_recording_permission"),
  githubAuthStatus: () => invoke<GithubAuthStatus>("github_auth_status"),
  githubOwners: () => invoke<string[]>("github_owners"),
  githubRepoExists: (repo: string) => invoke<boolean>("github_repo_exists", { repo }),
  githubCreateRepo: (repo: string) => invoke<void>("github_create_repo", { repo }),
  githubListSecrets: (repo: string) => invoke<string[]>("github_list_secrets", { repo }),
  githubSetSecret: (repo: string, name: string, value: string) =>
    invoke<void>("github_set_secret", { repo, name, value }),
  githubListRepos: (owner: string) => invoke<RepoEntry[]>("github_list_repos", { owner }),
  githubCheckWorkflow: (repo: string) => invoke<WorkflowStatus>("github_check_workflow", { repo }),
  githubInstallWorkflow: (
    repo: string,
    model: string,
    apt: string,
    visionModel: string,
    imageModel: string,
  ) => invoke<string>("github_install_workflow", { repo, model, apt, visionModel, imageModel }),
  githubBumpWorkflow: (repo: string) => invoke<string>("github_bump_workflow", { repo }),
  githubListTaskIssues: (repo: string) => invoke<TaskIssue[]>("github_list_task_issues", { repo }),
  githubListTaskPulls: (repo: string) => invoke<TaskPull[]>("github_list_task_pulls", { repo }),
  githubListWorkflowRuns: (repo: string) =>
    invoke<WorkflowRun[]>("github_list_workflow_runs", { repo }),
  githubCreateTaskIssue: (repo: string, title: string, body: string) =>
    invoke<string>("github_create_task_issue", { repo, title, body }),
  githubRunTaskIssue: (repo: string, number: number, body: string) =>
    invoke<string>("github_run_task_issue", { repo, number, body }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
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
  saveUpload: (data: string, mime: string) =>
        invoke<string>("save_upload", { data, mime }),
  createProjectDir: (name: string) => invoke<string>("create_project_dir", { name }),
  saveProjectFile: (project: string, filename: string, mime: string, data: string) =>
        invoke<string>("save_project_file", { project, filename, mime, data }),
  installSkill: (name: string) => invoke<void>("install_skill", { name }),
  uninstallSkill: (name: string) => invoke<void>("uninstall_skill", { name }),
  listInstalledSkills: () => invoke<string[]>("list_installed_skills"),
  getTraces: () => invoke<StoredSpan[]>("get_traces"),
  getMetrics: () => invoke<StoredMetric[]>("get_metrics"),
  listA2aAgents: () => invoke<A2aAgent[]>("list_a2a_agents"),
  addA2aAgent: (name: string, url: string) => invoke<void>("add_a2a_agent", { name, url }),
  removeA2aAgent: (name: string) => invoke<void>("remove_a2a_agent", { name }),
  setA2aAgentModel: (name: string, model: string) =>
    invoke<void>("set_a2a_agent_model", { name, model }),
};
