use crate::download::{ProgressEvent, download, find_checksum, sha256_digest, try_gh_download};
use crate::env::{
    asset_name, bin_dir, binary_name, config_path, home_dir, infer_bin_path, mock_mode,
};
use std::io::Read;
use tauri::ipc::Channel;

/// Base URL for the downloads of the CLI release `tag`. Release tags are
/// spelled `v0.249.0`, so the tag must be passed through verbatim.
fn cli_release_base(tag: &str) -> String {
    format!("https://github.com/inference-gateway/cli/releases/download/{tag}")
}

/// The CLI release this desktop release was tested with, from `.cli-version`.
pub(crate) fn tested_cli_tag() -> &'static str {
    include_str!("../../.cli-version").trim()
}

/// The tested release's `checksums.txt`, from `.cli-checksums.txt`, which CI
/// checks against the release's cosign signature.
fn tested_cli_checksums() -> &'static str {
    include_str!("../../.cli-checksums.txt")
}

/// The SHA-256 an archive of the CLI release `tag` must have. Only the tested
/// release has one, so an install of any other release is refused.
fn pinned_checksum(tag: &str, asset: &str) -> Result<String, String> {
    let tested = tested_cli_tag();
    if tag != tested {
        return Err(format!(
            "This desktop installs only the CLI release it is tested with, {tested}, not {tag}"
        ));
    }
    find_checksum(tested_cli_checksums(), asset)
        .ok_or_else(|| format!("The tested CLI release {tested} has no checksum for {asset}"))
}

/// The release an install downloads: the `tag` asked for, otherwise the tested
/// release, on a fresh install and on **Update** alike.
fn release_tag(tag: Option<&str>) -> &str {
    tag.unwrap_or(tested_cli_tag())
}

/// Install the CLI if it is missing. `force` reinstalls over an existing binary,
/// which is how an update is applied. `restart` also restarts the daemon, so
/// its workers run the installed binary. `tag` pins the release to install,
/// which implies `force`. When it is empty, the newest release wins.
#[tauri::command]
pub(crate) async fn check_and_install_cli(
    on_event: Channel<ProgressEvent>,
    force: bool,
    restart: bool,
    tag: Option<String>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking_with_state(app, move |state| {
        install_cli(&on_event, force, tag.as_deref())?;
        if restart {
            crate::cli::daemon::restart(&state.processes, &state.scheduler_log, &state.threads)?;
        }
        let _ = on_event.send(ProgressEvent::Ready);
        Ok(())
    })
    .await
}

/// Install (or reinstall) the CLI. A pinned tag installs over an existing
/// binary on purpose, so a tag implies `force`.
fn install_cli(
    on_event: &Channel<ProgressEvent>,
    force: bool,
    tag: Option<&str>,
) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Checking);
    let force = force || tag.is_some();
    let tag = release_tag(tag);
    if force && mock_mode() {
        crate::updates_mock::apply(|event| {
            let _ = on_event.send(event);
        });
        return Ok(());
    }

    let bin_path = infer_bin_path();

    if bin_path.exists() && !force {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&bin_path).map_err(|e| e.to_string())?;
            if meta.permissions().mode() & 0o111 != 0 {
                return Ok(());
            }
        }
        #[cfg(not(unix))]
        {
            return Ok(());
        }
    }

    let asset = asset_name().ok_or_else(|| {
        format!(
            "Unsupported platform: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let expected_hash = pinned_checksum(tag, asset)?;
    let release_base = cli_release_base(tag);

    let bin_dir = bin_dir();
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let archive_path = bin_dir.join(asset);

    let _ = on_event.send(ProgressEvent::Downloading {
        received: 0,
        total: 0,
    });
    if !try_gh_download(asset, &archive_path, Some(tag))? {
        download(
            &format!("{}/{}", release_base, asset),
            &archive_path,
            on_event,
        )?;
    }

    let _ = on_event.send(ProgressEvent::Verifying);
    let actual_hash = sha256_digest(&archive_path)?;
    if actual_hash != expected_hash {
        let _ = std::fs::remove_file(&archive_path);
        return Err(format!(
            "{asset} does not match the tested CLI release {tag} and was not installed: expected SHA-256 {expected_hash}, got {actual_hash}"
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

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_takes_the_tested_release_unless_a_tag_is_asked_for() {
        assert_eq!(release_tag(None), tested_cli_tag());
        assert_eq!(release_tag(Some("v0.250.0")), "v0.250.0");
    }

    #[test]
    fn only_the_tested_release_has_a_pinned_checksum() {
        let asset = asset_name().expect("a supported platform");
        let hash = pinned_checksum(tested_cli_tag(), asset).unwrap();
        assert_eq!(hash.len(), 64, "{hash:?}");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{hash:?}");
        assert!(pinned_checksum("v0.1.0", asset).is_err());
        assert!(pinned_checksum(tested_cli_tag(), "infer-plan9-mips.tar.gz").is_err());
    }

    #[test]
    fn the_tested_release_is_a_version_tag() {
        let tag = tested_cli_tag();
        let numbers: Vec<&str> = tag.trim_start_matches('v').split('.').collect();
        assert!(tag.starts_with('v'), "{tag:?}");
        assert_eq!(numbers.len(), 3, "{tag:?}");
        assert!(numbers.iter().all(|n| n.parse::<u32>().is_ok()), "{tag:?}");
    }

    #[test]
    fn test_cli_release_base() {
        assert_eq!(
            cli_release_base("v0.249.0"),
            "https://github.com/inference-gateway/cli/releases/download/v0.249.0"
        );
    }
}
