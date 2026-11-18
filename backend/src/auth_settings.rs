use crate::cli::config::{merge_auth_in, read_auth};
use crate::env::home_dir;
use std::collections::BTreeMap;

#[tauri::command]
pub(crate) async fn get_auth() -> Result<BTreeMap<String, String>, String> {
    Ok(read_auth())
}

/// Saves the provider keys, then restarts the daemon so its workers spawn
/// with them.
#[tauri::command]
pub(crate) async fn set_auth(
    keys: BTreeMap<String, String>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking_with_state(app, move |state| {
        merge_auth_in(&home_dir(), keys)?;
        crate::cli::daemon::restart(&state.processes, &state.scheduler_log, &state.threads)
    })
    .await
}
