// Typed wrapper around the Rust backend. Command args are camelCase (Tauri
// camelCases them); event/return payloads keep their Rust serde snake_case
// names, so the discriminated unions below use snake_case fields.
import { invoke, Channel, convertFileSrc } from "@tauri-apps/api/core";
import type { ScreenInput } from "./screen-input";

export { Channel, convertFileSrc };

export type ToolCallInfo = { id: string; name: string; args: string };

export type Shortcut = { name: string; description: string };

export type UserQuestion = {
  header: string;
  question: string;
  options: { label: string; description: string }[];
  multiSelect: boolean;
};

export type UserQuestionAnswer = {
  header: string;
  question: string;
  selectedLabels: string[];
  otherText?: string;
};

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
  | { kind: "UserQuestionRequest"; tool_call_id: string; questions: UserQuestion[] }
  | { kind: "RunStarted" }
  | { kind: "AgentError"; message: string }
  | { kind: "RawLine"; line: string }
  | { kind: "Done"; exit_code: number; stderr: string }
  | {
      kind: "TokenUsage";
      input: number;
      output: number;
      cached_read: number;
      total_tool_calls: number;
      last_input: number;
      context_window: number;
      cost: number;
    }
  | { kind: "Cancelled" }
  | { kind: "ToolCancelled"; tool_call_id: string }
  | { kind: "ConversationOptimized"; message: string; original_count: number; optimized_count: number }
  | { kind: "ComputerUsePaused" }
  | { kind: "ComputerUseResumed" }
  | { kind: "AgentStatus"; name: string; state: string; message: string; done: number; total: number }
  | { kind: "UserMessage"; content: string }
  | { kind: "BackgroundNote"; content: string }
  | { kind: "BackgroundTasks"; running: number; jobs: BackgroundJob[] }
  | { kind: "NextStepSuggestion"; text: string }
  | { kind: "RecordingStarted"; path: string; area: RecordedArea }
  | {
      kind: "ComputerUseAction";
      tool_call_id: string;
      action: string;
      x: number | null;
      y: number | null;
      screen_width: number;
      screen_height: number;
    }
  | { kind: "RecordingInput"; input: InputEvent }
  | { kind: "RecordingStopped" };

/** An agent recording's capture rectangle in the CLI's Computer frame space,
 * plus the frame size of the whole primary screen. */
export type RecordedArea = {
  x: number;
  y: number;
  width: number;
  height: number;
  frame_width: number;
  frame_height: number;
};

/** A key press or mouse click during an agent recording; `t` is seconds since
 * the recording started, click `x`/`y` are screen points. */
export type InputEvent =
  | { kind: "key"; t: number; keys: string; text: string }
  | { kind: "click"; t: number; button: string; x: number; y: number };

export type BackgroundJob = {
  id: string;
  kind: string;
  label: string;
  description: string;
  detail: string;
  status: string;
};

export type ProgressEvent =
  | { kind: "Checking" }
  | { kind: "Downloading"; received: number; total: number }
  | { kind: "Verifying" }
  | { kind: "Installing" }
  | { kind: "Initializing" }
  | { kind: "Ready" }
  | { kind: "Error"; message: string };

export type UpdateInfo = {
  name: string;
  current: string;
  latest: string | null;
  outdated: boolean;
  /** Raw release tags of the component's repo, newest first and capped at 5.
   * Absent in cached updates from before this became a field. */
  versions?: string[];
};
export type SttStatus = { binary: boolean; model: boolean; downloadable: boolean; hint: string };
export type VoiceSample = { name: string; path: string };
export type Avatar = { name: string; images: string[] };
export type Artifact = { name: string; size: number; modified_at: string };
/** An open GitHub issue of the project, as the `#` menu offers it. */
export type Issue = { number: number; title: string; state: string };
export type Conversation = { id: string; title?: string | null; project?: string | null };
export type A2aAgent = { name: string; url: string; run: boolean; model: string };
/** A Markdown agent (`<name>.md` in one of the agent dirs): frontmatter profile plus its instructions. */
export type MarkdownAgent = {
  name: string;
  description: string;
  emoji: string;
  model: string;
  /** The daemon workspace the agent's chats run on, blank for this computer. */
  workspace: string;
  /** The project the agent's new chats open in, blank for none. */
  project: string;
  /** The agent's AgentMail inbox address, blank while it has none. */
  email: string;
  /** Addresses or domains the agent may mail. Empty lets it mail only itself. */
  allowed_recipients: string[];
  /** The MCP servers the agent uses, by name. Empty uses every server of its computer. */
  mcp_servers: string[];
  instructions: string;
  avatar: string | null;
  path: string;
};
/** The latest screen of a session's computer and the frame space a pointer maps onto, zero when unknown. */
export type Screen = { src: string; width: number; height: number };
/** One thing a Kubernetes workspace needs from its cluster, and what to do when it is missing. */
export type Prerequisite = { name: string; met: boolean; detail: string };
/** The state of a Kubernetes workspace's cluster. The session counts are null when it cannot list sessions. */
export type ClusterCheck = { prerequisites: Prerequisite[]; running: number | null; suspended: number | null };
/** A workspace entry of ~/.infer/daemon.yaml as Settings edits it. One that names a cluster in `compute` reads that
 * cluster's kind, context, namespace and provider keys. */
export type WorkspaceConfig = {
  name: string;
  compute: string;
  kind: string;
  context: string;
  namespace: string;
  image: string;
  storage: string;
  idle_ttl: string;
  retention: string;
  gateway_env: string[];
  forward_env: string[];
  env: Record<string, string>;
};
export type Workspace = { name: string; kind: string; reachable: boolean; error: string | null };
/** A cluster of ~/.infer/compute.yaml as the Compute tab edits it. Every workspace on it shares its namespace and
 * gateway. */
export type ClusterConfig = { name: string; context: string; namespace: string; gateway_env: string[] };
/** A secret of Settings > Secrets: its label, the variable it becomes, and where it goes. Its value never comes back
 * from the backend. `mcp_servers` names the mcp.yaml servers whose headers expand the variable. */
export type Secret = {
  label: string;
  variable: string;
  computer: boolean;
  workspaces: string[];
  mcp_servers: string[];
};
/** The secret an agent's inbox key was saved as, where it went, and the clusters it could not reach. */
export type InboxSecret = { label: string; workspaces: string[]; failures: string[] };
/** One server's row of `infer mcp status`. */
export type McpServerStatus = { name: string; connected: boolean; tools: number; error: string };
export type McpStatus = {
  enabled: boolean;
  total_servers: number;
  connected_servers: number;
  total_tools: number;
  servers: McpServerStatus[];
};
/** A hosted server's header, written as `<prefix>${<variable>}`. A header written otherwise in mcp.yaml reads back as
 * `literal`, without its value. */
export type McpHeader = { name: string; prefix: string; variable: string; literal: boolean };
/** A server of ~/.infer/mcp.yaml as the MCPs tab edits it. `enabled` is whether this computer connects to it, and
 * `workspaces` the Kubernetes workspaces whose pods get it. */
export type McpServerConfig = {
  name: string;
  enabled: boolean;
  run: boolean;
  url: string;
  headers: McpHeader[];
  image: string;
  port: number;
  workspaces: string[];
};
export type McpSettings = { enabled: boolean; servers: McpServerConfig[] };
export type A2aStatus = { total_agents: number; ready_agents: number };
export type ProjectFile = { name: string; size: number };
export type GitRepo = { name: string; path: string; group: string; context: string | null };
export type GitProjectStatus = {
  git: string[];
  dirty: string[];
  branches: Record<string, string>;
  default_branches: Record<string, string>;
  remotes: Record<string, string>;
};
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
  text_to_speech_enabled: boolean;
  text_to_video_enabled: boolean;
  status_bar_enabled: boolean;
  next_step_suggestions_enabled: boolean;
  vision_annotator_model: string;
};
export type Timelines = { dir: string; names: string[] };
export type UndoResult = { popped: string; base: string; canUndo: boolean };
export type ExportPlan = { width: number; height: number; fps: number; frames: number; output: string };
export type GithubAuthStatus = { installed: boolean; authenticated: boolean };
export type DesktopUiData = {
  snippets: { id: string; label: string; prompt: string }[];
  skills_registry_url: string;
};
export type ImportReport = { imported: string[]; warnings: string[] };
export type ExportResult = { location: string | null; warnings: string[] };
export type OsPermissionState = "granted" | "not_granted" | "unavailable" | "not_applicable";
export type ComputerUsePermissionStatus = {
  computer_use_enabled: boolean;
  accessibility: OsPermissionState;
  screen_recording: OsPermissionState;
};
export type BrowserUseStatus = { enabled: boolean; connected: boolean; port: number; token: string };
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
/** One message of the thread's MESSAGES_SNAPSHOT, as the daemon's worker
 * restored it: assistant toolCalls, tool toolCallId, error when the tool failed. */
export type SnapshotMessage = {
  id?: string;
  role: string;
  content?: unknown;
  toolCalls?: { id: string; function: { name: string; arguments: string } }[];
  toolCallId?: string;
  error?: string | null;
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

export const isMacOS = navigator.platform.toUpperCase().includes("MAC");

export const SCREEN_RECORD_KEEP_KEY = "screenRecordKeep";
export const DEFAULT_SCREEN_RECORD_KEEP = 5;

export function screenRecordKeep(): number {
  const n = parseInt(localStorage.getItem(SCREEN_RECORD_KEEP_KEY) || "", 10);
  return Number.isFinite(n) && n >= 1 ? n : DEFAULT_SCREEN_RECORD_KEEP;
}

const hexEncode = (value: string) =>
  Array.from(new TextEncoder().encode(value), (b) => b.toString(16).padStart(2, "0")).join("");

export const api = {
  checkAndInstallCli: (onEvent: Channel<ProgressEvent>, force = false, restart = false, tag: string | null = null) =>
    invoke<void>("check_and_install_cli", { onEvent, force, restart, tag }),
  sendMessage: (args: {
    prompt: string;
    model: string;
    sessionId: string;
    onEvent: Channel<AgentEvent>;
    systemPrompt?: string;
    extraInstructions?: string;
    autoMode: boolean;
    project?: string;
    workspace?: string;
    agent?: string;
  }) => invoke<void>("send_message", args),
  /** Opens (or reuses) the chat's daemon thread and returns its history. */
  openThread: (args: {
    sessionId: string;
    model: string;
    onEvent: Channel<AgentEvent>;
    systemPrompt?: string;
    extraInstructions?: string;
    autoMode: boolean;
    project?: string;
    agent?: string;
  }) => invoke<string>("open_thread", args).then((json) => JSON.parse(json) as SnapshotMessage[]),
  /** Stops following a chat the user left, so the daemon may idle its worker out. */
  closeThread: (sessionId: string) => invoke<void>("close_thread", { sessionId }),
  sendApproval: (sessionId: string, toolCallId: string, approved: boolean) =>
    invoke<void>("send_approval", { sessionId, toolCallId, approved }),
  sendUserMessage: (sessionId: string, content: string) => invoke<void>("send_user_message", { sessionId, content }),
  sendQuestionAnswers: (sessionId: string, toolCallId: string, answers: UserQuestionAnswer[] | null) =>
    invoke<void>("send_question_answers", { sessionId, toolCallId, answers }),
  /** Continues a thread after a stop with a new run (resumes paused computer use). */
  resumeAgent: (sessionId: string) => invoke<void>("resume_agent", { sessionId }),
  cancelAgent: (sessionId: string) => invoke<void>("cancel_agent", { sessionId }),
  listConversations: () => invoke<string>("list_conversations"),
  deleteConversation: (sessionId: string, cwd?: string) =>
    invoke<void>("delete_conversation", { sessionId, cwd: cwd ?? null }),
  moveConversation: (sessionId: string, fromCwd?: string, toProject?: string) =>
    invoke<void>("move_conversation", { sessionId, fromCwd: fromCwd ?? null, toProject: toProject ?? null }),
  listModels: () => invoke<string[]>("list_models"),
  getAuth: () => invoke<Record<string, string>>("get_auth"),
  setAuth: (keys: Record<string, string>) => invoke<void>("set_auth", { keys }),
  getConfig: () => invoke<DesktopConfig>("get_config"),
  setConfig: (cfg: DesktopConfig) => invoke<void>("set_config", { cfg }),
  setDefaultModel: (model: string) => invoke<void>("set_default_model", { model }),
  readDesktopData: () => invoke<DesktopUiData>("read_desktop_data"),
  saveDesktopSnippets: (snippets: DesktopUiData["snippets"]) => invoke<void>("save_desktop_snippets", { snippets }),
  saveSkillsRegistryUrl: (url: string) => invoke<void>("save_skills_registry_url", { url }),
  exportDesktopFile: (format: string, path?: string | null) =>
    invoke<ExportResult>("export_desktop_file", { format, path: path ?? null }),
  exportDesktopGithub: (repo: string, format: string, token: string) =>
    invoke<ExportResult>("export_desktop_github", { repo, format, token: token || null }),
  importDesktopFile: (path?: string | null) => invoke<ImportReport>("import_desktop_file", { path: path ?? null }),
  importDesktopGithub: (repo: string, token: string) =>
    invoke<ImportReport>("import_desktop_github", { repo, token: token || null }),
  startGateway: (force = false, restart = false, tag: string | null = null) =>
    invoke<void>("start_gateway", { force, restart, tag }),
  startScheduler: () => invoke<void>("start_scheduler"),
  stopScheduler: () => invoke<void>("stop_scheduler"),
  getSchedulerStatus: () => invoke<boolean>("get_scheduler_status"),
  getSchedulerLog: () => invoke<string[]>("get_scheduler_log"),
  listSchedules: () => invoke<ScheduleJob[]>("list_schedules"),
  computerUsePermissionStatus: () => invoke<ComputerUsePermissionStatus>("computer_use_permission_status"),
  setComputerUseEnabled: (enabled: boolean) => invoke<void>("set_computer_use_enabled", { enabled }),
  browserUseStatus: () => invoke<BrowserUseStatus>("browser_use_status"),
  setBrowserUseEnabled: (enabled: boolean) => invoke<BrowserUseStatus>("set_browser_use_enabled", { enabled }),
  requestAccessibilityPermission: () => invoke<void>("request_accessibility_permission"),
  requestScreenRecordingPermission: () => invoke<void>("request_screen_recording_permission"),
  startScreenRecording: (keep: number) => invoke<string>("start_screen_recording", { keep }),
  stopScreenRecording: () => invoke<string>("stop_screen_recording"),
  screenRecordingStatus: () => invoke<boolean>("screen_recording_status"),
  githubAuthStatus: () => invoke<GithubAuthStatus>("github_auth_status"),
  githubOwners: () => invoke<string[]>("github_owners"),
  githubRepoExists: (repo: string) => invoke<boolean>("github_repo_exists", { repo }),
  githubCreateRepo: (repo: string) => invoke<void>("github_create_repo", { repo }),
  githubListSecrets: (repo: string) => invoke<string[]>("github_list_secrets", { repo }),
  githubSetSecret: (repo: string, name: string, value: string) =>
    invoke<void>("github_set_secret", { repo, name, value }),
  githubListRepos: (owner: string) => invoke<RepoEntry[]>("github_list_repos", { owner }),
  githubCheckWorkflow: (repo: string) => invoke<WorkflowStatus>("github_check_workflow", { repo }),
  githubInstallWorkflow: (repo: string, model: string, apt: string, visionModel: string, imageModel: string) =>
    invoke<string>("github_install_workflow", { repo, model, apt, visionModel, imageModel }),
  githubBumpWorkflow: (repo: string) => invoke<string>("github_bump_workflow", { repo }),
  githubListTaskIssues: (repo: string) => invoke<TaskIssue[]>("github_list_task_issues", { repo }),
  githubListTaskPulls: (repo: string) => invoke<TaskPull[]>("github_list_task_pulls", { repo }),
  githubListWorkflowRuns: (repo: string) => invoke<WorkflowRun[]>("github_list_workflow_runs", { repo }),
  githubCreateTaskIssue: (repo: string, title: string, body: string) =>
    invoke<string>("github_create_task_issue", { repo, title, body }),
  githubRunTaskIssue: (repo: string, number: number, body: string) =>
    invoke<string>("github_run_task_issue", { repo, number, body }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  checkUpdates: () => invoke<UpdateInfo[]>("check_updates"),
  installDesktopUpdate: (tag: string | null = null) => invoke<void>("install_desktop_update", { tag }),
  sttStatus: () => invoke<SttStatus>("stt_status"),
  prepareStt: (onEvent: Channel<ProgressEvent>) => invoke<void>("prepare_stt", { onEvent }),
  transcribeAudio: (wav: number[]) => invoke<string>("transcribe_audio", { wav }),
  listVoiceSamples: () => invoke<VoiceSample[]>("list_voice_samples"),
  addVoiceSample: () => invoke<VoiceSample | null>("add_voice_sample"),
  saveVoiceSample: (name: string, wav: number[]) => invoke<VoiceSample>("save_voice_sample", { name, wav }),
  deleteVoiceSample: (name: string) => invoke<void>("delete_voice_sample", { name }),
  listAvatars: () => invoke<Avatar[]>("list_avatars"),
  importAvatar: (name: string, onLine: Channel<string>) => invoke<boolean>("import_avatar", { name, onLine }),
  snapshotAvatar: (name: string, jpeg: number[], onLine: Channel<string>) =>
    invoke<void>("snapshot_avatar", { name, jpeg, onLine }),
  deleteAvatar: (name: string) => invoke<void>("delete_avatar", { name }),
  listTimelines: (project: string) => invoke<Timelines>("list_timelines", { project }),
  watchProject: (project: string) => invoke<void>("watch_project", { project }),
  readTimeline: (project: string, name: string) => invoke<string>("read_timeline", { project, name }),
  writeTimeline: (project: string, name: string, data: string) =>
    invoke<void>("write_timeline", { project, name, data }),
  checkpointContentProject: (project: string, message: string) =>
    invoke<boolean>("checkpoint_content_project", { name: project, message }),
  undoContentProject: (project: string) => invoke<UndoResult>("undo_content_project", { name: project }),
  redoContentProject: (project: string, sha: string, base: string) =>
    invoke<boolean>("redo_content_project", { name: project, sha, base }),
  contentCanUndo: (project: string) => invoke<boolean>("content_can_undo", { name: project }),
  revealProjectFile: (project: string, name: string) => invoke<void>("reveal_project_file", { project, name }),
  deleteProjectFile: (project: string, name: string) => invoke<void>("delete_project_file", { project, name }),
  addProjectVideo: (project: string) => invoke<string | null>("add_project_video", { project }),
  importProjectFile: (project: string, name: string, bytes: ArrayBuffer) =>
    invoke<string>("import_project_file", bytes, {
      headers: { "x-project": project, "x-name": hexEncode(name) },
    }),
  prepareContentTools: (onEvent: Channel<ProgressEvent>) => invoke<void>("prepare_content_tools", { onEvent }),
  exportBegin: (project: string, name: string, timeline: string) =>
    invoke<ExportPlan>("export_begin", { project, name, timeline }),
  exportFrame: (frame: ArrayBuffer) => invoke<void>("export_frame", frame),
  exportEnd: () => invoke<string>("export_end"),
  exportCancel: () => invoke<void>("export_cancel"),
  readHistory: () => invoke<string[]>("read_history"),
  appendHistory: (line: string) => invoke<void>("append_history", { line }),
  readBashHistory: () => invoke<string[]>("read_bash_history"),
  appendBashHistory: (line: string) => invoke<void>("append_bash_history", { line }),
  readProjects: () => invoke<string>("read_projects"),
  writeProjects: (data: string) => invoke<void>("write_projects", { data }),
  saveImage: (path: string) => invoke<string>("save_image", { path }),
  saveAudio: (path: string) => invoke<string>("save_audio", { path }),
  attachPick: (project: string | null) => invoke<string | null>("attach_pick", { project }),
  attachBytes: (project: string | null, name: string, bytes: ArrayBuffer) =>
    invoke<string>("attach_bytes", bytes, {
      headers: { "x-project": project ? hexEncode(project) : "", "x-name": hexEncode(name) },
    }),
  createProjectDir: (name: string, group?: string) => invoke<string>("create_project_dir", { name, group }),
  moveProject: (name: string, group: string) => invoke<string>("move_project", { name, group }),
  listProjectFiles: (project: string) => invoke<ProjectFile[]>("list_project_files", { project }),
  listProjectMedia: (project: string) => invoke<ProjectFile[]>("list_project_media", { project }),
  scanGitRepos: (root: string) => invoke<GitRepo[]>("scan_git_repos", { root }),
  cloneGithubRepo: (repo: string) => invoke<GitRepo>("clone_github_repo", { repo }),
  gitProjectStatus: () => invoke<GitProjectStatus>("git_project_status"),
  syncDefaultBranch: (name: string) => invoke<string>("sync_default_branch", { name }),
  cleanupProject: (name: string) => invoke<string>("cleanup_project", { name }),
  projectDirExists: (name: string) => invoke<boolean>("project_dir_exists", { name }),
  openInVsCode: (name: string) => invoke<void>("open_in_vs_code", { name }),
  refreshProjectContext: (name: string) => invoke<string | null>("refresh_project_context", { name }),
  installSkill: (name: string) => invoke<void>("install_skill", { name }),
  uninstallSkill: (name: string) => invoke<void>("uninstall_skill", { name }),
  listInstalledSkills: () => invoke<string[]>("list_installed_skills"),
  listTools: () => invoke<string[]>("list_tools"),
  listFiles: (project: string | null) => invoke<string[]>("list_files", { project }),
  listIssues: (project: string | null) => invoke<Issue[]>("list_issues", { project }),
  listShortcuts: () => invoke<Shortcut[]>("list_shortcuts"),
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  a2aStatus: () => invoke<A2aStatus>("a2a_status"),
  startServices: () => invoke<void>("start_services"),
  getTraces: () => invoke<StoredSpan[]>("get_traces"),
  getMetrics: () => invoke<StoredMetric[]>("get_metrics"),
  listA2aAgents: () => invoke<A2aAgent[]>("list_a2a_agents"),
  addA2aAgent: (name: string, url: string) => invoke<void>("add_a2a_agent", { name, url }),
  removeA2aAgent: (name: string) => invoke<void>("remove_a2a_agent", { name }),
  setA2aAgentModel: (name: string, model: string) => invoke<void>("set_a2a_agent_model", { name, model }),
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  workspaceConfigs: () => invoke<WorkspaceConfig[]>("workspace_configs"),
  mcpSettings: () => invoke<McpSettings>("mcp_settings"),
  /** Answers whether daemon.yaml changed, beside the container start or stop that failed. */
  saveMcpServer: (server: McpServerConfig, isNew: boolean) =>
    invoke<[boolean, string[]]>("save_mcp_server", { server, isNew }),
  deleteMcpServer: (name: string) => invoke<[boolean, string[]]>("delete_mcp_server", { name }),
  setMcpEnabled: (enabled: boolean) => invoke<void>("set_mcp_enabled", { enabled }),
  listSecrets: () => invoke<Secret[]>("list_secrets"),
  saveSecret: (secret: Secret, value: string, isNew: boolean) =>
    invoke<string[]>("save_secret", { secret, value, isNew }),
  deleteSecret: (label: string) => invoke<string[]>("delete_secret", { label }),
  saveInboxSecret: (name: string, ownerKey: string) => invoke<InboxSecret>("save_inbox_secret", { name, ownerKey }),
  saveWorkspace: (config: WorkspaceConfig, previousName?: string) =>
    invoke<void>("save_workspace", { config, previousName }),
  deleteWorkspace: (name: string) => invoke<void>("delete_workspace", { name }),
  kubeContexts: () => invoke<string[]>("kube_contexts"),
  defaultWorkspaceImage: () => invoke<string>("default_workspace_image"),
  clusterConfigs: () => invoke<ClusterConfig[]>("cluster_configs"),
  saveCluster: (config: ClusterConfig, previousName?: string) => invoke<void>("save_cluster", { config, previousName }),
  deleteCluster: (name: string) => invoke<void>("delete_cluster", { name }),
  checkCluster: (name: string) => invoke<ClusterCheck>("check_cluster", { name }),
  /** Whether the installed infer reads compute.yaml, which every cluster action needs. */
  computeSupported: () => invoke<boolean>("compute_supported"),
  detectClusters: () => invoke<string[]>("detect_clusters"),
  dockerAvailable: () => invoke<boolean>("docker_available"),
  /** Provisions a k3s cluster in Docker, registers it with an Ubuntu desktop workspace and answers the workspace's
   * name. */
  provisionLocalCluster: (onStage: (stage: string) => void) => {
    const stages = new Channel<string>();
    stages.onmessage = onStage;
    return invoke<string>("provision_local_cluster", { onStage: stages });
  },
  checkWorkspace: (context: string, namespace: string, name: string) =>
    invoke<ClusterCheck>("check_workspace", { context, namespace, name }),
  moveSession: (
    sessionId: string,
    project: string | undefined,
    target: string,
    remote: boolean,
    onStage: (stage: string) => void,
  ) => {
    const stages = new Channel<string>();
    stages.onmessage = onStage;
    return invoke<void>("move_session", { sessionId, project, target, remote, onStage: stages });
  },
  listArtifacts: (sessionId: string, project?: string) => invoke<Artifact[]>("list_artifacts", { sessionId, project }),
  saveArtifact: (sessionId: string, project: string | undefined, name: string) =>
    invoke<string>("save_artifact", { sessionId, project, name }),
  cleanMachines: (sessions: { id: string; project?: string }[]) => invoke<number>("clean_machines", { sessions }),
  deleteMachine: (session: { id: string; project?: string }) => invoke<boolean>("delete_machine", { session }),
  screenFrame: (sessionId: string, project?: string) => invoke<Screen | null>("screen_frame", { sessionId, project }),
  screenControl: (sessionId: string, project: string | undefined, takeOver: boolean) =>
    invoke<boolean>("screen_control", { sessionId, project, takeOver }),
  screenInput: (sessionId: string, project: string | undefined, input: ScreenInput) =>
    invoke<boolean>("screen_input", { sessionId, project, input }),
  listMarkdownAgents: () => invoke<MarkdownAgent[]>("list_markdown_agents"),
  saveMarkdownAgent: (agent: MarkdownAgent, previousName?: string) =>
    invoke<string>("save_markdown_agent", { agent, previousName }),
  deleteMarkdownAgent: (name: string) => invoke<void>("delete_markdown_agent", { name }),
  pickAgentAvatar: (name: string) => invoke<boolean>("pick_agent_avatar", { name }),
  removeAgentAvatar: (name: string) => invoke<void>("remove_agent_avatar", { name }),
  setAgentAvatarFromUrl: (name: string, url: string) => invoke<void>("set_agent_avatar_from_url", { name, url }),
  createAgentInbox: (name: string, apiKey: string) => invoke<string>("create_agent_inbox", { name, apiKey }),
  setAgentEmailKey: (name: string, apiKey: string) => invoke<void>("set_agent_email_key", { name, apiKey }),
  moveAgentEmailKey: (from: string, to: string) => invoke<void>("move_agent_email_key", { from, to }),
  syncAgentMail: (name: string) => invoke<string[]>("sync_agent_mail", { name }),
};
