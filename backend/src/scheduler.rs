use crate::AppState;
use crate::env::{infer_bin_path, infer_env, mock_mode};
use std::io::BufRead;

/// Max log lines kept in memory.
const MAX_LOG: usize = 200;

#[tauri::command]
pub(crate) async fn start_scheduler(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if mock_mode() {
        return Ok(());
    }

    // Kill any existing instance first.
    {
        let mut guard = state.scheduler_child.lock().map_err(|e| e.to_string())?;
        if let Some(mut old) = guard.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
    }

    let bin = infer_bin_path();
    let mut child = std::process::Command::new(&bin)
        .args(["channels-manager"])
        .envs(infer_env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start scheduler: {}", e))?;

    // Collect stdout/stderr into the shared log buffer.
    let log = state.scheduler_log.clone();
    if let Some(stdout) = child.stdout.take() {
        let log_clone = log.clone();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let mut buf = log_clone.lock().unwrap();
                buf.push(line);
                if buf.len() > MAX_LOG {
                    buf.remove(0);
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let log_clone = log;
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let mut buf = log_clone.lock().unwrap();
                buf.push(line);
                if buf.len() > MAX_LOG {
                    buf.remove(0);
                }
            }
        });
    }

    *state.scheduler_child.lock().map_err(|e| e.to_string())? = Some(child);
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
    Ok(guard.clone())
}
