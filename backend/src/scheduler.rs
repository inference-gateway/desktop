use crate::AppState;
use crate::env::{infer_bin_path, infer_env, mock_mode};
use std::collections::VecDeque;
use std::io::BufRead;
use std::sync::{Arc, Mutex};

/// Max log lines kept in memory.
const MAX_LOG: usize = 200;

/// Spawn a background thread that reads lines from `pipe` and appends them to `log`.
fn pipe_logger<R: std::io::Read + Send + 'static>(pipe: R, log: Arc<Mutex<VecDeque<String>>>) {
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines().map_while(Result::ok) {
            let mut buf = match log.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("scheduler_log mutex poisoned: {e}");
                    return;
                }
            };
            buf.push_back(line);
            if buf.len() > MAX_LOG {
                buf.pop_front();
            }
        }
    });
}

#[tauri::command]
pub(crate) async fn start_scheduler(state: tauri::State<'_, AppState>) -> Result<(), String> {
    spawn_daemon(&state)
}

/// Kill any previous daemon child and spawn a fresh one. Called from the
/// Settings save flow and from app setup (autostart when scheduling is
/// enabled), so the daemon survives app restarts.
pub(crate) fn spawn_daemon(state: &AppState) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }

    let mut guard = state.scheduler_child.lock().map_err(|e| e.to_string())?;
    if let Some(mut old) = guard.take() {
        let _ = old.kill();
        let _ = old.wait();
    }
    if let Ok(mut log) = state.scheduler_log.lock() {
        log.clear();
    }

    let bin = infer_bin_path();
    let mut child = std::process::Command::new(&bin)
        .arg("daemon")
        .current_dir(crate::env::agent_cwd())
        .envs(infer_env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start scheduler: {}", e))?;

    let log = state.scheduler_log.clone();
    if let Some(stdout) = child.stdout.take() {
        pipe_logger(stdout, log.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_logger(stderr, log);
    }

    *guard = Some(child);
    Ok(())
}

#[tauri::command]
pub(crate) async fn stop_scheduler(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.scheduler_child.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_scheduler_status(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let mut guard = state.scheduler_child.lock().map_err(|e| e.to_string())?;
    Ok(guard
        .as_mut()
        .and_then(|c| c.try_wait().ok())
        .is_some_and(|s| s.is_none()))
}

/// A scheduled job as persisted by the CLI in `~/.infer/schedules/*.yaml`.
#[derive(serde::Serialize)]
pub(crate) struct ScheduleJob {
    id: String,
    name: String,
    description: String,
    cron_expression: String,
    prompt: String,
    run_once: bool,
    last_run: String,
    last_error: String,
}

// Schedules are machine-global: the CLI always persists them under
// ~/.infer/schedules (cli#1053), independent of the conversation storage
// backend or path.
#[tauri::command]
pub(crate) async fn list_schedules() -> Result<Vec<ScheduleJob>, String> {
    Ok(read_schedules_dir(
        &crate::env::home_dir().join(".infer").join("schedules"),
    ))
}

fn read_schedules_dir(dir: &std::path::Path) -> Vec<ScheduleJob> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut jobs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_norway::from_str::<serde_norway::Value>(&text) else {
            continue;
        };
        let s = |k: &str| {
            val.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        jobs.push(ScheduleJob {
            id: s("id"),
            name: s("name"),
            description: s("description"),
            cron_expression: s("cron_expression"),
            prompt: s("prompt"),
            run_once: val
                .get("run_once")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            last_run: s("last_run"),
            last_error: s("last_error"),
        });
    }
    jobs.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    jobs
}

/// GitHub CLI availability/auth, shown by the Settings scheduling section as a
/// prerequisite for the github scheduling backend.
#[derive(serde::Serialize)]
pub(crate) struct GithubAuthStatus {
    installed: bool,
    authenticated: bool,
}

#[tauri::command]
pub(crate) async fn github_auth_status() -> Result<GithubAuthStatus, String> {
    tokio::task::spawn_blocking(|| {
        let installed = crate::download::gh_available();
        GithubAuthStatus {
            installed,
            authenticated: installed && crate::download::gh_authenticated(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

fn gh_output(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(crate::download::gh_bin())
        .args(args)
        .output()
        .map_err(|e| format!("gh failed to start: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The user's own login plus the orgs they belong to, for the repository
/// owner dropdown in Settings.
#[tauri::command]
pub(crate) async fn github_owners() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        let user = gh_output(&["api", "user", "--jq", ".login"])?;
        let orgs = gh_output(&["api", "user/orgs", "--jq", ".[].login"]).unwrap_or_default();
        let mut owners: Vec<String> = std::iter::once(user.as_str())
            .chain(orgs.lines())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        owners.dedup();
        Ok(owners)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn valid_repo(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    ok(owner) && ok(name)
}

fn valid_secret_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[tauri::command]
pub(crate) async fn github_repo_exists(repo: String) -> Result<bool, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        Ok(gh_output(&["repo", "view", &repo, "--json", "name"]).is_ok())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Create the routines repository, private with an initial commit so a
/// default branch exists - mirrors the CLI's ensureRepo.
#[tauri::command]
pub(crate) async fn github_create_repo(repo: String) -> Result<(), String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        gh_output(&["repo", "create", &repo, "--private", "--add-readme"]).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Names of the Actions secrets already set on `repo`.
#[tauri::command]
pub(crate) async fn github_list_secrets(repo: String) -> Result<Vec<String>, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        let out = gh_output(&[
            "secret", "list", "--repo", &repo, "--json", "name", "--jq", ".[].name",
        ])?;
        Ok(out.lines().map(String::from).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Set an Actions secret on `repo` via `gh secret set`, value piped over
/// stdin so it never appears in argv. The value is never stored locally.
#[tauri::command]
pub(crate) async fn github_set_secret(
    repo: String,
    name: String,
    value: String,
) -> Result<(), String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    if !valid_secret_name(&name) {
        return Err(format!("invalid secret name: {name}"));
    }
    if value.is_empty() {
        return Err("secret value is empty".into());
    }
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new(crate::download::gh_bin())
            .args(["secret", "set", &name, "--repo", &repo])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("gh failed to start: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("gh stdin unavailable")?
            .write_all(value.as_bytes())
            .map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open a URL in the default browser. Restricted to http(s) so a malformed
/// config value can't be used to launch arbitrary schemes.
#[tauri::command]
pub(crate) async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("refusing to open non-http url: {url}"));
    }
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(opener)
            .arg(&url)
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| {
                s.success()
                    .then_some(())
                    .ok_or_else(|| format!("{opener} exited with {s}"))
            })
    })
    .await
    .map_err(|e| e.to_string())?
}

// infer-action workflow installed into a task repository, modeled on the
// opentask extension's template. Placeholders filled by str::replace because
// the YAML is full of `${{ }}` that format! would reject.
const TASKS_YML: &str = r#"name: Task

on:
  workflow_dispatch:
    inputs:
      model:
        description: Model to use (provider/model, e.g. llamacpp/phi-4)
        required: false
        default: {model}
      prompt:
        description: Task for the agent (workflow_dispatch only)
        required: false
        default: ""
      enable_git:
        description: Enable git operations - branch, commit, PR (workflow_dispatch only)
        required: false
        default: "true"
      system_prompt:
        description: Override the direct-prompt system prompt (workflow_dispatch only)
        required: false
        default: ""
  issues:
    types:
      - opened
      - edited
  issue_comment:
    types:
      - created
  pull_request_review_comment:
    types:
      - created

permissions:
  issues: write
  contents: write
  pull-requests: write

jobs:
  opentask:
    runs-on: ubuntu-24.04
    timeout-minutes: 25
    steps:
      - uses: actions/create-github-app-token@v3.2.0
        id: app-token
        with:
          client-id: ${{ secrets.{client_id_secret} }}
          private-key: ${{ secrets.{private_key_secret} }}

      - uses: actions/checkout@v7.0.1
        with:
          token: ${{ steps.app-token.outputs.token }}

      - uses: inference-gateway/infer-action@v0.48.1
        with:
          github-token: ${{ steps.app-token.outputs.token }}
          github-app-slug: ${{ steps.app-token.outputs.app-slug }}
          trigger-phrase: "@opentask"
          model: ${{ inputs.model || vars.DEFAULT_MODEL || '{model}' }}
          direct-prompt: ${{ inputs.prompt }}
          system-prompt-direct: ${{ inputs.system_prompt }}
          enable-git-operations: "${{ inputs.enable_git || 'true' }}"
          llamacpp-api-url: ${{ secrets.LLAMACPP_API_URL }}
          llamacpp-api-key: ${{ secrets.LLAMACPP_API_KEY }}
          anthropic-api-key: ${{ secrets.ANTHROPIC_API_KEY }}
          openai-api-key: ${{ secrets.OPENAI_API_KEY }}
          google-api-key: ${{ secrets.GOOGLE_API_KEY }}
          deepseek-api-key: ${{ secrets.DEEPSEEK_API_KEY }}
          groq-api-key: ${{ secrets.GROQ_API_KEY }}
          mistral-api-key: ${{ secrets.MISTRAL_API_KEY }}
          cohere-api-key: ${{ secrets.COHERE_API_KEY }}
          ollama-cloud-api-key: ${{ secrets.OLLAMA_CLOUD_API_KEY }}
"#;

const WORKFLOW_PATH: &str = ".github/workflows/tasks.yml";
const INSTALL_BRANCH: &str = "infer-agent-install";

#[derive(serde::Serialize)]
pub(crate) struct WorkflowStatus {
    installed: bool,
    url: Option<String>,
    sha: Option<String>,
}

fn workflow_status(repo: &str) -> Result<WorkflowStatus, String> {
    match gh_output(&[
        "api",
        &format!("repos/{repo}/contents/{WORKFLOW_PATH}"),
        "--jq",
        "[.html_url, .sha] | join(\" \")",
    ]) {
        Ok(out) => {
            let mut parts = out.split_whitespace();
            Ok(WorkflowStatus {
                installed: true,
                url: parts.next().map(String::from),
                sha: parts.next().map(String::from),
            })
        }
        Err(e) if e.contains("404") || e.contains("Not Found") => Ok(WorkflowStatus {
            installed: false,
            url: None,
            sha: None,
        }),
        Err(e) => Err(e),
    }
}

/// Whether the infer-action task workflow exists in `repo`.
#[tauri::command]
pub(crate) async fn github_check_workflow(repo: String) -> Result<WorkflowStatus, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || workflow_status(&repo))
        .await
        .map_err(|e| e.to_string())?
}

/// Install (or update) the infer-action task workflow into `repo` via a pull
/// request: branch, commit tasks.yml, open PR. Returns the PR URL.
#[tauri::command]
pub(crate) async fn github_install_workflow(repo: String, model: String) -> Result<String, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    let cfg = crate::config::read_config();
    if !valid_secret_name(&cfg.scheduler_github_app_client_id_secret)
        || !valid_secret_name(&cfg.scheduler_github_app_private_key_secret)
    {
        return Err("invalid GitHub App secret names in config".into());
    }
    tokio::task::spawn_blocking(move || {
        use base64::Engine as _;
        let yaml = TASKS_YML
            .replace("{model}", model.trim())
            .replace(
                "{client_id_secret}",
                &cfg.scheduler_github_app_client_id_secret,
            )
            .replace(
                "{private_key_secret}",
                &cfg.scheduler_github_app_private_key_secret,
            );
        let base = gh_output(&["api", &format!("repos/{repo}"), "--jq", ".default_branch"])?
            .trim()
            .to_string();
        let sha = gh_output(&[
            "api",
            &format!("repos/{repo}/git/ref/heads/{base}"),
            "--jq",
            ".object.sha",
        ])?
        .trim()
        .to_string();
        if let Err(e) = gh_output(&[
            "api",
            "-X",
            "POST",
            &format!("repos/{repo}/git/refs"),
            "-f",
            &format!("ref=refs/heads/{INSTALL_BRANCH}"),
            "-f",
            &format!("sha={sha}"),
        ]) && !e.contains("already exists")
        {
            return Err(e);
        }
        let existing = workflow_status(&repo)?;
        let content = base64::engine::general_purpose::STANDARD.encode(yaml.as_bytes());
        let message = if existing.installed {
            "ci: sync infer-action task workflow"
        } else {
            "feat: add infer-action task workflow"
        };
        let mut put_args: Vec<String> = vec![
            "api".into(),
            "-X".into(),
            "PUT".into(),
            format!("repos/{repo}/contents/{WORKFLOW_PATH}"),
            "-f".into(),
            format!("message={message}"),
            "-f".into(),
            format!("branch={INSTALL_BRANCH}"),
            "-f".into(),
            format!("content={content}"),
        ];
        if let Some(sha) = existing.sha {
            put_args.push("-f".into());
            put_args.push(format!("sha={sha}"));
        }
        let put_refs: Vec<&str> = put_args.iter().map(String::as_str).collect();
        gh_output(&put_refs)?;
        gh_output(&[
            "api",
            "-X",
            "POST",
            &format!("repos/{repo}/pulls"),
            "-f",
            &format!("title={message}"),
            "-f",
            &format!("head={INSTALL_BRANCH}"),
            "-f",
            &format!("base={base}"),
            "-f",
            "body=Installs the infer-action task workflow. Trigger tasks with @opentask in issues and comments, or via workflow dispatch.",
            "--jq",
            ".html_url",
        ])
        .map(|s| s.trim().to_string())
        .map_err(|e| {
            if e.contains("No commits between") {
                "The workflow is already up to date - nothing to install.".into()
            } else {
                e
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct TaskIssue {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    created_at: String,
}

/// Recent issues in the task repository, newest first.
/// ponytail: newest 30, no pagination - enough for a dashboard.
#[tauri::command]
pub(crate) async fn github_list_task_issues(repo: String) -> Result<Vec<TaskIssue>, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        let out = gh_output(&[
            "api",
            &format!("repos/{repo}/issues?state=all&per_page=30"),
            "--jq",
            "[.[] | select(.pull_request | not) | {number, title, state, html_url, created_at}]",
        ])?;
        serde_json::from_str(&out).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct WorkflowRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: String,
    created_at: String,
}

/// Recent runs of the installed task workflow, newest first.
#[tauri::command]
pub(crate) async fn github_list_workflow_runs(repo: String) -> Result<Vec<WorkflowRun>, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        let out = gh_output(&[
            "api",
            &format!("repos/{repo}/actions/workflows/tasks.yml/runs?per_page=20"),
            "--jq",
            "[.workflow_runs[] | {id, name: .display_title, status, conclusion, html_url, created_at}]",
        ])?;
        serde_json::from_str(&out).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Create a task issue; the body is prefixed with the @opentask trigger so
/// the installed workflow always picks it up. Returns the issue URL.
#[tauri::command]
pub(crate) async fn github_create_task_issue(
    repo: String,
    title: String,
    body: String,
) -> Result<String, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    if title.trim().is_empty() {
        return Err("task title is empty".into());
    }
    tokio::task::spawn_blocking(move || {
        gh_output(&[
            "api",
            "-X",
            "POST",
            &format!("repos/{repo}/issues"),
            "-f",
            &format!("title={}", title.trim()),
            "-f",
            &format!("body=@opentask\n\n{}", body.trim()),
            "--jq",
            ".html_url",
        ])
        .map(|s| s.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn get_scheduler_log(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let guard = state.scheduler_log.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::{TASKS_YML, read_schedules_dir, valid_repo, valid_secret_name};

    #[test]
    fn tasks_yml_has_placeholders_and_trigger() {
        assert!(TASKS_YML.contains("{model}"));
        assert!(TASKS_YML.contains("{client_id_secret}"));
        assert!(TASKS_YML.contains("{private_key_secret}"));
        assert!(TASKS_YML.contains("trigger-phrase: \"@opentask\""));
        assert!(TASKS_YML.contains("inference-gateway/infer-action@"));
    }

    #[test]
    fn valid_repo_accepts_owner_slash_name_only() {
        assert!(valid_repo("edenreich/.routines"));
        assert!(valid_repo("inference-gateway/desktop"));
        assert!(!valid_repo("no-slash"));
        assert!(!valid_repo("/name"));
        assert!(!valid_repo("owner/"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("owner/na me"));
        assert!(!valid_repo("owner/$(rm)"));
    }

    #[test]
    fn valid_secret_name_is_upper_snake() {
        assert!(valid_secret_name("APP_CLIENT_ID"));
        assert!(valid_secret_name("OPENAI_API_KEY"));
        assert!(!valid_secret_name(""));
        assert!(!valid_secret_name("lower"));
        assert!(!valid_secret_name("1STARTS_WITH_DIGIT"));
        assert!(!valid_secret_name("HAS-DASH"));
    }

    #[test]
    fn read_schedules_dir_parses_job_yaml() {
        let dir = std::env::temp_dir().join(format!("sched-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("test-job.yaml"),
            "id: test-job\nname: fixture\ncron_expression: '@every 3m'\nprompt: say hello\nrun_once: true\ncreated_at: 2026-08-12T09:49:30Z\nupdated_at: 2026-08-12T09:49:30Z\n",
        )
        .unwrap();
        std::fs::write(dir.join("ignored.txt"), "not yaml").unwrap();

        let jobs = read_schedules_dir(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "test-job");
        assert_eq!(jobs[0].name, "fixture");
        assert_eq!(jobs[0].cron_expression, "@every 3m");
        assert!(jobs[0].run_once);
    }

    #[test]
    fn read_schedules_dir_missing_dir_is_empty() {
        assert!(read_schedules_dir(std::path::Path::new("/nonexistent/schedules")).is_empty());
    }
}
