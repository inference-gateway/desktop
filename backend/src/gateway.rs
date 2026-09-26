use crate::AppState;
use crate::agent::gateway_url;
use crate::config::auth_env;
use crate::env::{collector_env, home_dir};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

// --- Gateway lifecycle (desktop-owned) ---
// The desktop downloads and runs the inference-gateway binary itself so /v1/models
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
    fn from_flags(force: bool, restart: bool) -> Self {
        match (force, restart) {
            (false, false) => Self::Reuse,
            (false, true) => Self::Restart,
            (true, _) => Self::Reinstall,
        }
    }
}

pub(crate) fn gateway_bin_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "inference-gateway.exe"
    } else {
        "inference-gateway"
    };
    home_dir().join(".infer").join("bin").join(name)
}

/// Release asset name for the gateway binary, matching goreleaser's naming.
pub(crate) fn gateway_asset_name() -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        "arm" => "armv7",
        _ => return None,
    };
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    Some(format!("inference-gateway_{}_{}.{}", os, arch, ext))
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

/// Download and extract the gateway binary if it isn't already present.
/// `Reinstall` re-downloads over an existing binary; the caller must have
/// stopped it first, otherwise the extraction hits ETXTBSY.
pub(crate) fn ensure_gateway_binary(mode: GatewayStart) -> Result<PathBuf, String> {
    let bin = gateway_bin_path();
    if bin.exists() && mode != GatewayStart::Reinstall {
        return Ok(bin);
    }
    if cfg!(target_os = "windows") {
        return Err("Automatic gateway download is not supported on Windows yet".into());
    }

    let asset = gateway_asset_name().ok_or_else(|| {
        format!(
            "Unsupported platform for gateway binary: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let bin_dir = home_dir().join(".infer").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let url = format!(
        "https://github.com/inference-gateway/inference-gateway/releases/latest/download/{}",
        asset
    );
    let archive = bin_dir.join(&asset);
    let _ = std::fs::remove_file(&bin);

    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("Failed to download gateway: {}", e))?;
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    drop(file);

    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&bin_dir)
        .arg("inference-gateway")
        .status()
        .map_err(|e| format!("Failed to extract gateway: {}", e))?;
    let _ = std::fs::remove_file(&archive);

    if !status.success() || !bin.exists() {
        return Err("Failed to extract gateway binary from release archive".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).map_err(|e| e.to_string())?;
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

pub(crate) fn spawn_gateway(bin: &Path) -> Result<std::process::Child, String> {
    std::process::Command::new(bin)
        .envs(auth_env())
        .envs(collector_env())
        .envs(audio_env(
            crate::config::read_config().text_to_speech_enabled,
        ))
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
) -> Result<(), String> {
    let processes = Arc::clone(&state.processes);
    tokio::task::spawn_blocking(move || {
        processes.start_gateway(GatewayStart::from_flags(force, restart))
    })
    .await
    .map_err(|error| format!("gateway startup task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_flags_maps_flag_pairs_to_start_modes() {
        for (force, restart, mode) in [
            (false, false, GatewayStart::Reuse),
            (false, true, GatewayStart::Restart),
            (true, false, GatewayStart::Reinstall),
            (true, true, GatewayStart::Reinstall),
        ] {
            assert_eq!(GatewayStart::from_flags(force, restart), mode);
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
