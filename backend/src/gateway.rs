use crate::AppState;
use crate::agent::gateway_url;
use crate::config::auth_env;
use crate::env::{bin_dir, collector_env};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

// --- Gateway lifecycle (desktop-owned) ---
// The desktop installs the inference-gateway binary through the CLI and runs it itself so /v1/models
// stays served. Once it's up, `infer agent` detects it (its own isBinaryRunning
// health check) and won't start a competing gateway.

/// How the desktop wants the gateway brought up: keep whatever is already
/// serving the URL, respawn the owned process with fresh env (API keys,
/// AUDIO_ENABLED), or replace the binary first. The IPC `force` flag maps to
/// `Reinstall` because a fresh binary needs a restart, so the redundant
/// `(force, restart)` pair cannot be expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayStart {
    Reuse,
    Restart,
    Reinstall,
}

impl GatewayStart {
    /// Without download support (Windows) a reinstall has no new binary to
    /// install, so it becomes `Reuse`: the gateway already serving the URL -
    /// desktop-owned or external - keeps running instead of being stopped for
    /// a download that fails.
    fn from_flags(force: bool, restart: bool, download_supported: bool) -> Self {
        match (force, restart) {
            (false, false) => Self::Reuse,
            (false, true) => Self::Restart,
            (true, _) if download_supported => Self::Reinstall,
            (true, _) => Self::Reuse,
        }
    }
}

pub(crate) fn gateway_download_supported() -> bool {
    !cfg!(target_os = "windows")
}

/// Whether the desktop runs the gateway itself, so that no CLI child may run
/// one. A child that found no gateway for a moment, while the desktop was
/// restarting its own, started a second one, took the port and later stopped
/// it. On Windows the desktop cannot install the gateway, and the CLI's own
/// stays the fallback.
pub(crate) fn desktop_runs_gateway() -> bool {
    gateway_download_supported() && !crate::env::mock_mode()
}

pub(crate) fn gateway_bin_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "inference-gateway.exe"
    } else {
        "inference-gateway"
    };
    bin_dir().join(name)
}

pub(crate) fn gateway_reachable() -> bool {
    let url = format!("{}/v1/models", gateway_url().trim_end_matches('/'));
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(750)))
        .build();
    ureq::Agent::new_with_config(config)
        .get(&url)
        .call()
        .is_ok()
}

/// Whether a gateway already serving `url` can be reused. Decided once
/// everything the desktop owns has stopped, so whatever answers is foreign.
/// `Reuse` adopts it - that is how an externally run gateway is supported -
/// while `Restart` and `Reinstall` respawn with fresh env and cannot take a
/// held port, so a foreign gateway is an error the user can act on.
pub(crate) fn reuse_running_gateway(
    reachable: bool,
    mode: GatewayStart,
    url: &str,
) -> Result<bool, String> {
    if !reachable {
        return Ok(false);
    }
    if mode != GatewayStart::Reuse {
        return Err(format!(
            "{url} is already served by a gateway the desktop does not own, so it cannot be restarted with the current settings; stop that process and try again"
        ));
    }
    Ok(true)
}

/// The CLI's `gateway.version` that follows the newest gateway release.
const LATEST_GATEWAY_VERSION: &str = "latest";

/// The `gateway.version` a reinstall leaves in the CLI config: the release the
/// user picked, or the latest one. The CLI then keeps the gateway on it.
fn chosen_gateway_version(tag: Option<&str>) -> &str {
    tag.unwrap_or(LATEST_GATEWAY_VERSION)
}

fn run_infer(args: &[&str]) -> Result<(), String> {
    let output = crate::env::command(crate::env::infer_bin_path())
        .args(args)
        .env("HOME", crate::env::home_dir())
        .envs(crate::env::infer_env())
        .output()
        .map_err(|e| format!("Failed to run infer {}: {e}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(crate::binaries::cli_error(&output.stderr))
}

/// Install the gateway binary through the CLI, the single owner of it, when
/// it is missing or a reinstall is asked for. A reinstall first records the
/// release to run as the CLI's `gateway.version`, so the CLI does not replace
/// it with another one later. The desktop downloads nothing itself.
pub(crate) fn ensure_gateway_binary(
    mode: GatewayStart,
    tag: Option<&str>,
) -> Result<PathBuf, String> {
    let bin = gateway_bin_path();
    if bin.exists() && mode != GatewayStart::Reinstall {
        return Ok(bin);
    }
    if !gateway_download_supported() {
        return Err("Automatic gateway download is not supported on Windows yet".into());
    }

    if mode == GatewayStart::Reinstall {
        run_infer(&[
            "config",
            "set",
            "gateway.version",
            chosen_gateway_version(tag),
        ])?;
    }
    run_infer(&["gateway", "install"])?;

    if !bin.exists() {
        return Err("infer gateway install left no gateway binary".into());
    }
    Ok(bin)
}

/// Env vars that switch on /v1/audio/speech in the gateway so the CLI's
/// TextToSpeech tool (engine "gateway") can reach it; the gateway defaults
/// audio off. Read at spawn, so toggling TTS needs a gateway restart.
pub(crate) fn audio_env(tts_enabled: bool) -> Vec<(&'static str, &'static str)> {
    if tts_enabled {
        vec![
            ("AUDIO_ENABLED", "true"),
            ("AUDIO_LOCAL_AUTO_DOWNLOAD", "true"),
        ]
    } else {
        vec![]
    }
}

/// Env var that serves the gateway's Videos API for the CLI's TextToVideo
/// tool; the gateway defaults it off. Read at spawn, like `audio_env`.
fn video_env(ttv_enabled: bool) -> Option<(&'static str, &'static str)> {
    ttv_enabled.then_some(("VIDEOS_ENABLED", "true"))
}

pub(crate) fn spawn_gateway(bin: &Path) -> Result<std::process::Child, String> {
    let cfg = crate::config::read_config();
    crate::env::command(bin)
        .envs(auth_env())
        .envs(collector_env())
        .envs(audio_env(cfg.text_to_speech_enabled))
        .envs(video_env(cfg.text_to_video_enabled))
        .env("TELEMETRY_ENABLED", "true")
        .env("TELEMETRY_TRACING_ENABLED", "true")
        .env("IMAGES_ENABLED", "true")
        .env("CLIENT_RESPONSE_HEADER_TIMEOUT", "200s")
        .env("SERVER_WRITE_TIMEOUT", "200s")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start gateway: {e}"))
}

#[tauri::command]
pub(crate) async fn start_gateway(
    state: tauri::State<'_, AppState>,
    force: bool,
    restart: bool,
    tag: Option<String>,
) -> Result<(), String> {
    let processes = Arc::clone(&state.processes);
    tokio::task::spawn_blocking(move || {
        processes.start_gateway(
            GatewayStart::from_flags(force, restart, gateway_download_supported()),
            tag.as_deref(),
        )
    })
    .await
    .map_err(|error| format!("gateway startup task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reinstall_records_the_picked_release_or_the_latest() {
        assert_eq!(chosen_gateway_version(Some("v0.63.1")), "v0.63.1");
        assert_eq!(chosen_gateway_version(None), LATEST_GATEWAY_VERSION);
    }

    #[test]
    fn from_flags_maps_flag_pairs_to_start_modes() {
        for (force, restart, mode) in [
            (false, false, GatewayStart::Reuse),
            (false, true, GatewayStart::Restart),
            (true, false, GatewayStart::Reinstall),
            (true, true, GatewayStart::Reinstall),
        ] {
            assert_eq!(GatewayStart::from_flags(force, restart, true), mode);
        }
    }

    #[test]
    fn from_flags_keeps_the_gateway_up_when_it_cannot_be_downloaded() {
        for (force, restart, mode) in [
            (false, false, GatewayStart::Reuse),
            (false, true, GatewayStart::Restart),
            (true, false, GatewayStart::Reuse),
            (true, true, GatewayStart::Reuse),
        ] {
            assert_eq!(GatewayStart::from_flags(force, restart, false), mode);
        }
    }

    #[test]
    fn reuse_running_gateway_adopts_a_foreign_gateway_only_in_reuse_mode() {
        for (mode, reachable, reuse) in [
            (GatewayStart::Reuse, false, false),
            (GatewayStart::Restart, false, false),
            (GatewayStart::Reinstall, false, false),
            (GatewayStart::Reuse, true, true),
        ] {
            assert_eq!(reuse_running_gateway(reachable, mode, "u"), Ok(reuse));
        }
        for mode in [GatewayStart::Restart, GatewayStart::Reinstall] {
            let err = reuse_running_gateway(true, mode, "http://localhost:8080")
                .expect_err("a foreign gateway cannot be restarted");
            assert!(err.contains("http://localhost:8080"), "{err}");
        }
    }

    #[test]
    fn audio_env_sets_audio_vars_only_when_tts_enabled() {
        assert!(audio_env(false).is_empty());
        assert_eq!(
            audio_env(true),
            vec![
                ("AUDIO_ENABLED", "true"),
                ("AUDIO_LOCAL_AUTO_DOWNLOAD", "true")
            ]
        );
    }
}
