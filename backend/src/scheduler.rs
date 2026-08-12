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

#[tauri::command]
pub(crate) async fn get_scheduler_log(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let guard = state.scheduler_log.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::read_schedules_dir;

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
