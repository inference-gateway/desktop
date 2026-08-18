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

pub(crate) fn gh_output(args: &[&str]) -> Result<String, String> {
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

fn valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct RepoEntry {
    name: String,
    open: u64,
}

/// Repositories under `owner` with their open issue + PR counts, busiest
/// first, for the tasks repo dropdown.
/// ponytail: first 100 repos, no pagination - enough for a dropdown.
#[tauri::command]
pub(crate) async fn github_list_repos(owner: String) -> Result<Vec<RepoEntry>, String> {
    if !valid_owner(&owner) {
        return Err(format!("invalid owner: {owner}"));
    }
    tokio::task::spawn_blocking(move || {
        let out = gh_output(&[
            "repo",
            "list",
            &owner,
            "--limit",
            "100",
            "--json",
            "name,issues,pullRequests",
            "--jq",
            "[.[] | {name, open: (.issues.totalCount + .pullRequests.totalCount)}] | sort_by(-.open)",
        ])?;
        serde_json::from_str(&out).map_err(|e| format!("unexpected gh output: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn valid_repo(repo: &str) -> bool {
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

pub(crate) fn valid_secret_name(name: &str) -> bool {
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

#[tauri::command]
pub(crate) async fn get_scheduler_log(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let guard = state.scheduler_log.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::{read_schedules_dir, valid_repo, valid_secret_name};

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
