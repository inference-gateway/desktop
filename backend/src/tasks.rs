use crate::scheduler::{gh_output, valid_repo, valid_secret_name};

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

      - uses: inference-gateway/infer-action@{version}
        with:
          github-token: ${{ steps.app-token.outputs.token }}
          github-app-slug: ${{ steps.app-token.outputs.app-slug }}
          trigger-phrase: "@opentask"
          model: ${{ inputs.model || vars.DEFAULT_MODEL || '{model}' }}
          direct-prompt: ${{ inputs.prompt }}
          system-prompt-direct: ${{ inputs.system_prompt }}
          enable-git-operations: "${{ inputs.enable_git || 'true' }}"{extras}
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
    version: Option<String>,
    latest: Option<String>,
}

/// The `inference-gateway/infer-action@<version>` pin in a workflow file.
fn action_version(yaml: &str) -> Option<String> {
    let rest = yaml.split("inference-gateway/infer-action@").nth(1)?;
    let v: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect::<String>()
        .trim_end_matches('"')
        .to_string();
    (!v.is_empty()).then_some(v)
}

fn latest_action_version() -> Option<String> {
    gh_output(&[
        "api",
        "repos/inference-gateway/infer-action/releases/latest",
        "--jq",
        ".tag_name",
    ])
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Decoded content of the installed workflow file on `git_ref`.
fn workflow_content(repo: &str, git_ref: &str) -> Result<String, String> {
    use base64::Engine as _;
    let raw = gh_output(&[
        "api",
        &format!("repos/{repo}/contents/{WORKFLOW_PATH}?ref={git_ref}"),
        "--jq",
        ".content",
    ])?;
    let stripped: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(stripped)
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn workflow_status(repo: &str, git_ref: Option<&str>) -> Result<WorkflowStatus, String> {
    let path = match git_ref {
        Some(r) => format!("repos/{repo}/contents/{WORKFLOW_PATH}?ref={r}"),
        None => format!("repos/{repo}/contents/{WORKFLOW_PATH}"),
    };
    match gh_output(&["api", &path, "--jq", "[.html_url, .sha] | join(\" \")"]) {
        Ok(out) => {
            let mut parts = out.split_whitespace();
            Ok(WorkflowStatus {
                installed: true,
                url: parts.next().map(String::from),
                sha: parts.next().map(String::from),
                version: None,
                latest: None,
            })
        }
        Err(e) if e.contains("404") || e.contains("Not Found") => Ok(WorkflowStatus {
            installed: false,
            url: None,
            sha: None,
            version: None,
            latest: None,
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
    tokio::task::spawn_blocking(move || {
        let mut status = workflow_status(&repo, None)?;
        if status.installed {
            let base = gh_output(&["api", &format!("repos/{repo}"), "--jq", ".default_branch"])?
                .trim()
                .to_string();
            status.version = workflow_content(&repo, &base)
                .ok()
                .as_deref()
                .and_then(action_version);
            status.latest = latest_action_version();
        }
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Branch + commit `yaml` as the workflow file + open a PR against the
/// default branch. The install branch is recreated from the base head every
/// time so a stale branch never poisons the flow.
fn open_workflow_pr(repo: &str, yaml: &str, message: &str, body: &str) -> Result<String, String> {
    use base64::Engine as _;
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
    let _ = gh_output(&[
        "api",
        "-X",
        "DELETE",
        &format!("repos/{repo}/git/refs/heads/{INSTALL_BRANCH}"),
    ]);
    gh_output(&[
        "api",
        "-X",
        "POST",
        &format!("repos/{repo}/git/refs"),
        "-f",
        &format!("ref=refs/heads/{INSTALL_BRANCH}"),
        "-f",
        &format!("sha={sha}"),
    ])?;
    let existing = workflow_status(repo, Some(&base))?;
    let content = base64::engine::general_purpose::STANDARD.encode(yaml.as_bytes());
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
        &format!("body={body}"),
        "--jq",
        ".html_url",
    ])
    .map(|s| s.trim().to_string())
    .map_err(|e| {
        if e.contains("No commits between") {
            "The workflow is already up to date - nothing to change.".into()
        } else {
            e
        }
    })
}

/// Install (or update) the infer-action task workflow into `repo` via a pull
/// request. `apt`, `vision_model`, and `image_model` are optional extras
/// written into the workflow when non-empty. Returns the PR URL.
#[tauri::command]
pub(crate) async fn github_install_workflow(
    repo: String,
    model: String,
    apt: String,
    vision_model: String,
    image_model: String,
) -> Result<String, String> {
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
        let version = latest_action_version().unwrap_or_else(|| "v0.49.0".into());
        let mut extras = String::new();
        for (key, val) in [
            ("apt", apt.trim()),
            ("vision-model", vision_model.trim()),
            ("image-model", image_model.trim()),
        ] {
            if !val.is_empty() {
                extras.push_str(&format!("\n          {key}: {val}"));
            }
        }
        let yaml = TASKS_YML
            .replace("{version}", &version)
            .replace("{model}", model.trim())
            .replace("{extras}", &extras)
            .replace(
                "{client_id_secret}",
                &cfg.scheduler_github_app_client_id_secret,
            )
            .replace(
                "{private_key_secret}",
                &cfg.scheduler_github_app_private_key_secret,
            );
        let installed = workflow_status(&repo, None)?.installed;
        let message = if installed {
            "ci: sync infer-action task workflow"
        } else {
            "feat: add infer-action task workflow"
        };
        open_workflow_pr(
            &repo,
            &yaml,
            message,
            "Installs the infer-action task workflow. Trigger tasks with @opentask in issues and comments, or via workflow dispatch.",
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Bump only the infer-action version pin in the installed workflow via a
/// pull request, leaving the rest of the user's file untouched.
#[tauri::command]
pub(crate) async fn github_bump_workflow(repo: String) -> Result<String, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        let base = gh_output(&["api", &format!("repos/{repo}"), "--jq", ".default_branch"])?
            .trim()
            .to_string();
        let content = workflow_content(&repo, &base)?;
        let current = action_version(&content)
            .ok_or("no inference-gateway/infer-action pin found in the workflow")?;
        let latest =
            latest_action_version().ok_or("couldn't resolve the latest infer-action release")?;
        if current == latest {
            return Err(format!("already on the latest infer-action ({latest})"));
        }
        let yaml = content.replace(
            &format!("infer-action@{current}"),
            &format!("infer-action@{latest}"),
        );
        let message = format!("chore(deps): bump infer-action to {latest}");
        let body = format!(
            "Bumps inference-gateway/infer-action from {current} to {latest}. Only the version pin changes - the rest of the workflow is untouched."
        );
        open_workflow_pr(&repo, &yaml, &message, &body)
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
            &format!("repos/{repo}/issues?state=open&per_page=30"),
            "--jq",
            "[.[] | select(.pull_request | not) | {number, title, state, html_url, created_at}]",
        ])?;
        serde_json::from_str(&out).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct TaskPull {
    number: u64,
    title: String,
    html_url: String,
    body: Option<String>,
}

/// Open pull requests in the task repository, newest first.
/// ponytail: newest 30, no pagination - enough for a dashboard.
#[tauri::command]
pub(crate) async fn github_list_task_pulls(repo: String) -> Result<Vec<TaskPull>, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    tokio::task::spawn_blocking(move || {
        let out = gh_output(&[
            "api",
            &format!("repos/{repo}/pulls?state=open&per_page=30"),
            "--jq",
            "[.[] | {number, title, html_url, body}]",
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
        ]);
        match out {
            Ok(out) => serde_json::from_str(&out).map_err(|e| e.to_string()),
            Err(e) if e.contains("404") || e.contains("Not Found") => Ok(Vec::new()),
            Err(e) => Err(e),
        }
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

/// Trigger the installed workflow on an issue by posting the trigger-phrase comment.
#[tauri::command]
pub(crate) async fn github_run_task_issue(
    repo: String,
    number: u64,
    body: String,
) -> Result<String, String> {
    if !valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    if body.trim().is_empty() {
        return Err("comment is empty".into());
    }
    tokio::task::spawn_blocking(move || {
        gh_output(&[
            "api",
            "-X",
            "POST",
            &format!("repos/{repo}/issues/{number}/comments"),
            "-f",
            &format!("body={}", body.trim()),
            "--jq",
            ".html_url",
        ])
        .map(|s| s.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{TASKS_YML, action_version};

    #[test]
    fn tasks_yml_has_placeholders_and_trigger() {
        assert!(TASKS_YML.contains("{model}"));
        assert!(TASKS_YML.contains("{version}"));
        assert!(TASKS_YML.contains("{extras}"));
        assert!(TASKS_YML.contains("{client_id_secret}"));
        assert!(TASKS_YML.contains("{private_key_secret}"));
        assert!(TASKS_YML.contains("trigger-phrase: \"@opentask\""));
        assert!(TASKS_YML.contains("inference-gateway/infer-action@"));
    }

    #[test]
    fn action_version_parses_the_pin() {
        assert_eq!(
            action_version("      - uses: inference-gateway/infer-action@v0.48.1\n        with:"),
            Some("v0.48.1".into())
        );
        assert_eq!(
            action_version("uses: \"inference-gateway/infer-action@v1.2.3\""),
            Some("v1.2.3".into())
        );
        assert_eq!(action_version("uses: actions/checkout@v7"), None);
        assert_eq!(action_version("inference-gateway/infer-action@"), None);
    }
}
