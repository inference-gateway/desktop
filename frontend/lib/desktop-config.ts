import type { DesktopConfig } from "@/lib/tauri";

/** One input of a settings form that edits a `DesktopConfig` key. */
export type StorageField = {
  key: keyof DesktopConfig;
  label: string;
  ph?: string;
  secret?: boolean;
  options?: readonly string[];
};

/** The config the Settings forms start from until the saved one has loaded. */
export const DEFAULT_CONFIG: DesktopConfig = {
  storage_backend: "jsonl",
  storage_directory: "",
  gateway_url: "http://localhost:8080",
  default_model: "",
  sqlite_path: "",
  postgres_host: "localhost",
  postgres_port: "5432",
  postgres_database: "infer_conversations",
  postgres_username: "",
  postgres_password: "",
  postgres_ssl_mode: "prefer",
  redis_host: "localhost",
  redis_port: "6379",
  redis_password: "",
  redis_db: "0",
  d1_account_id: "",
  d1_database_id: "",
  d1_api_token: "",
  d1_base_url: "https://api.cloudflare.com/client/v4",
  extra_instructions: "",
  system_prompt: "",
  schedule_enabled: false,
  scheduler_backend: "local",
  scheduler_github_repository: "",
  scheduler_github_app_client_id_secret: "APP_CLIENT_ID",
  scheduler_github_app_private_key_secret: "APP_PRIVATE_KEY",
  scheduler_github_bot_name: "infer",
  scheduler_github_bot_email: "infer@users.noreply.github.com",
  scheduler_github_pull_requests: false,
  scheduler_github_artifacts_enabled: true,
  scheduler_github_artifacts_poll_interval: "10m",
  scheduler_github_artifacts_initial_delay: "1m",
  scheduler_github_artifacts_max_attempts: "3",
  scheduler_github_artifacts_rate_limit_backoff: "1h",
  projects_root: "",
  projects_backend: "local",
  projects_github_repository: ".projects",
  projects_max_file_size_mb: "500",
  projects_allowed_mimes: "",
  text_to_speech_enabled: false,
  text_to_speech_model: "",
  text_to_speech_voice: "",
  text_to_speech_auto_download: true,
  text_to_speech_output_dir: "",
  text_to_speech_require_approval: false,
  text_to_video_enabled: false,
  text_to_video_avatar_model: "",
  status_bar_enabled: true,
  next_step_suggestions_enabled: false,
  vision_annotator_model: "",
};
