use crate::download::{ProgressEvent, download, find_checksum, sha256_digest, try_gh_download};
use crate::env::{asset_name, bin_dir, binary_name, config_path, home_dir, infer_bin_path};
use std::io::Read;
use tauri::ipc::Channel;

/// Base URL for CLI release downloads: pinned to `tag` when given, `latest`
/// otherwise. Release tags are spelled `v0.249.0`, so the tag must be passed
/// through verbatim.
fn cli_release_base(tag: Option<&str>) -> String {
    match tag {
        Some(tag) => format!("https://github.com/inference-gateway/cli/releases/download/{tag}"),
        None => "https://github.com/inference-gateway/cli/releases/latest/download".to_string(),
    }
}

/// Install the CLI if it is missing. `force` reinstalls over an existing binary,
/// which is how an update is applied. `restart` also restarts the daemon, so
/// its workers run the installed binary. `tag` pins the release to install,
/// which implies `force`; when it is empty, the newest release wins.
#[tauri::command]
pub(crate) async fn check_and_install_cli(
    on_event: Channel<ProgressEvent>,
    force: bool,
    restart: bool,
    tag: Option<String>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking_with_state(app, move |state| {
        install_cli(&on_event, force, restart, tag.as_deref(), state)
    })
    .await
}

fn install_cli(
    on_event: &Channel<ProgressEvent>,
    force: bool,
    restart: bool,
    tag: Option<&str>,
    state: &crate::AppState,
) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Checking);
    // A pinned tag installs over an existing binary on purpose, so force is implied.
    let force = force || tag.is_some();

    let bin_path = infer_bin_path();

    if bin_path.exists() && !force {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&bin_path).map_err(|e| e.to_string())?;
            if meta.permissions().mode() & 0o111 != 0 {
                return finish(on_event, restart, state);
            }
        }
        #[cfg(not(unix))]
        {
            return finish(on_event, restart, state);
        }
    }

    let asset = asset_name().ok_or_else(|| {
        format!(
            "Unsupported platform: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let release_base = cli_release_base(tag);

    let bin_dir = bin_dir();
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let archive_path = bin_dir.join(asset);

    let _ = on_event.send(ProgressEvent::Downloading {
        received: 0,
        total: 0,
    });
    if !try_gh_download(asset, &archive_path, tag)? {
        download(
            &format!("{}/{}", release_base, asset),
            &archive_path,
            on_event,
        )?;
    }

    let _ = on_event.send(ProgressEvent::Verifying);
    let checksums_url = format!("{}/checksums.txt", release_base);
    let checksums_text = {
        let checksums_temp = bin_dir.join("checksums.txt.tmp");
        if try_gh_download("checksums.txt", &checksums_temp, tag)? {
            let text = std::fs::read_to_string(&checksums_temp).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&checksums_temp);
            text
        } else {
            let checksums_resp = ureq::get(&checksums_url)
                .call()
                .map_err(|e| format!("Failed to download checksums.txt: {}", e))?;
            let mut text = String::new();
            checksums_resp
                .into_body()
                .into_reader()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            text
        }
    };

    let expected_hash = find_checksum(&checksums_text, asset)
        .ok_or_else(|| format!("Checksum not found for {}", asset))?;
    let actual_hash = sha256_digest(&archive_path)?;

    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&archive_path);
        return Err(format!(
            "Checksum mismatch for {}: expected {}, got {}",
            asset, expected_hash, actual_hash
        ));
    }

    let _ = on_event.send(ProgressEvent::Installing);
    let staging = extract_cli(&archive_path, &bin_dir)?;
    let _ = std::fs::remove_file(&archive_path);
    std::fs::rename(staging.join(binary_name()), &bin_path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&staging);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).map_err(|e| e.to_string())?;
    }

    let cfg_path = config_path();
    if !cfg_path.exists() {
        let _ = on_event.send(ProgressEvent::Initializing);
        let status = crate::env::command(&bin_path)
            .args(["init"])
            .env("HOME", home_dir().to_str().unwrap_or(""))
            .status()
            .map_err(|e| format!("Failed to run infer init: {}", e))?;
        if !status.success() {
            return Err("infer init failed".into());
        }

        let mut config = String::new();
        let _ = std::fs::File::open(&cfg_path).and_then(|mut f| f.read_to_string(&mut config));
        let mut needs_write = false;
        if !config.contains("gateway.run:") {
            config.push_str("\ngateway.run: true\n");
            needs_write = true;
        }
        if !config.contains("gateway.standalone_binary:") {
            config.push_str("gateway.standalone_binary: true\n");
            needs_write = true;
        }
        if needs_write {
            std::fs::write(&cfg_path, &config).map_err(|e| e.to_string())?;
        }
    }

    finish(on_event, restart, state)
}

/// Extracts the executable from the release tarball into a staging directory
/// next to the binary and returns that directory, so the rename into place
/// stays atomic and never writes over a running `infer`.
fn extract_cli(
    archive: &std::path::Path,
    bin_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let staging = bin_dir.join("cli-install.tmp");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let status = crate::env::command("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&staging)
        .arg(binary_name())
        .status()
        .map_err(|e| format!("Failed to extract the CLI: {}", e))?;
    if !status.success() || !staging.join(binary_name()).exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("Failed to extract the CLI binary from the release archive".into());
    }
    Ok(staging)
}

/// Reports the binary ready, after restarting the daemon when asked.
fn finish(
    on_event: &Channel<ProgressEvent>,
    restart: bool,
    state: &crate::AppState,
) -> Result<(), String> {
    if restart {
        crate::daemon::restart(state)?;
    }
    let _ = on_event.send(ProgressEvent::Ready);
    Ok(())
}

// --- Pinned download URLs ---

#[test]
fn test_cli_release_base() {
    assert_eq!(
        cli_release_base(Some("v0.249.0")),
        "https://github.com/inference-gateway/cli/releases/download/v0.249.0"
    );
    assert_eq!(
        cli_release_base(None),
        "https://github.com/inference-gateway/cli/releases/latest/download"
    );
}

// --- Agent streaming types ---
