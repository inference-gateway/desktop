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

#[tauri::command]
pub(crate) async fn get_scheduler_log(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let guard = state.scheduler_log.lock().map_err(|e| e.to_string())?;
    Ok(guard.iter().cloned().collect())
}
