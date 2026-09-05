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

/// Download and extract the gateway binary if it isn't already present.
/// `force` re-downloads over an existing binary; the caller must have stopped it
/// first, otherwise the extraction hits ETXTBSY.
pub(crate) fn ensure_gateway_binary(force: bool) -> Result<PathBuf, String> {
    let bin = gateway_bin_path();
    if bin.exists() && !force {
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
        .env("CLIENT_RESPONSE_HEADER_TIMEOUT", "120s")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start gateway: {e}"))
}

/// Start (or restart) the gateway. `force` re-downloads the binary first, so an
/// update lands on the next spawn. `restart` respawns an already-running gateway
/// without re-downloading, so a newly saved API key gets injected into its env
/// (keys are read only at spawn via `auth_env()`). Images are enabled here via `IMAGES_ENABLED=true`
/// (the gateway defaults them off, which otherwise 404s the `/v1/images` endpoints),
/// and the upstream response-header timeout is raised from its 10s default so
/// non-streaming image generation (which OpenAI answers in 20-60s) doesn't 502.
/// The audio endpoint (/v1/audio/speech) is enabled the same way when the
/// Settings text-to-speech toggle is on (see `audio_env`).
#[tauri::command]
pub(crate) async fn start_gateway(
    state: tauri::State<'_, AppState>,
    force: bool,
    restart: bool,
) -> Result<(), String> {
    let processes = Arc::clone(&state.processes);
    tokio::task::spawn_blocking(move || processes.start_gateway(force, restart))
        .await
        .map_err(|error| format!("gateway startup task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

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
