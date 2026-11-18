use crate::workspace_settings::{Provisioned, provision_local};
use tauri::ipc::Channel;

/// Provisions a k3s cluster on the chosen runtime and registers it and its
/// Ubuntu desktop workspace, reporting each step. Answers the workspace's
/// name and the secrets that could not be restored. It sits above
/// `workspace_settings` and `cli::secrets`, so neither imports the other.
#[tauri::command]
pub(crate) async fn provision_local_cluster(
    on_stage: Channel<String>,
    recreate: bool,
) -> Result<Provisioned, String> {
    tokio::task::spawn_blocking(move || {
        provision_local(
            |stage| {
                let _ = on_stage.send(stage.to_owned());
            },
            recreate,
            crate::cli::secrets::restore_in,
        )
    })
    .await
    .map_err(|e| format!("provisioning stopped: {e}"))?
}
