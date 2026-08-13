use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use tauri::ipc::Channel;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub(crate) enum ProgressEvent {
    Checking,
    Downloading {
        received: u64,
        total: u64,
    },
    Verifying,
    Installing,
    Initializing,
    Ready,
    #[allow(dead_code)]
    Error {
        message: String,
    },
}
/// Download a file from `url` to `dest`, sending progress through the channel.
pub(crate) fn download(
    url: &str,
    dest: &std::path::Path,
    on_event: &Channel<ProgressEvent>,
) -> Result<(), String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("Failed to download {}: {}", url, e))?;

    let total: u64 = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 8192];
    let mut received = 0u64;

    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        received += n as u64;
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        let _ = on_event.send(ProgressEvent::Downloading { received, total });
    }
    Ok(())
}

/// Compute SHA256 hex digest of a file.
pub(crate) fn sha256_digest(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Parse checksums.txt and find the expected hash for `asset_name`.
pub(crate) fn find_checksum(checksums_text: &str, asset_name: &str) -> Option<String> {
    for line in checksums_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == asset_name {
            return Some(parts[0].to_string());
        }
    }
    None
}

/// Absolute path to the `gh` CLI. A Finder-launched .app inherits launchd's
/// minimal PATH (no /opt/homebrew/bin), so probe common install dirs beyond
/// PATH before falling back to the bare name.
pub(crate) fn gh_bin() -> &'static std::path::Path {
    static GH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    GH.get_or_init(|| {
        crate::stt::find_on_path("gh")
            .or_else(|| {
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                [
                    std::path::PathBuf::from("/opt/homebrew/bin/gh"),
                    std::path::PathBuf::from("/usr/local/bin/gh"),
                ]
                .into_iter()
                .chain(home.map(|h| h.join(".local/bin/gh")))
                .find(|p| p.is_file())
            })
            .unwrap_or_else(|| "gh".into())
    })
}

pub(crate) fn gh_available() -> bool {
    std::process::Command::new(gh_bin())
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

pub(crate) fn gh_authenticated() -> bool {
    std::process::Command::new(gh_bin())
        .args(["auth", "status"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Try to download a release asset using `gh` CLI (authenticated).
/// Returns `Ok(true)` if downloaded, `Ok(false)` to fall back to ureq.
pub(crate) fn try_gh_download(asset: &str, dest: &std::path::Path) -> Result<bool, String> {
    if !gh_available() || !gh_authenticated() {
        return Ok(false);
    }
    let status = std::process::Command::new(gh_bin())
        .args([
            "release",
            "download",
            "latest",
            "--repo",
            "inference-gateway/cli",
            "--pattern",
            asset,
            "--output",
            dest.to_str().unwrap_or(""),
            "--clobber",
        ])
        .status()
        .map_err(|e| format!("gh release download failed: {}", e))?;
    if !status.success() {
        return Err("gh release download exited with non-zero status".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_checksum() {
        let text = "abc123deadbeef  infer-linux-amd64\ndef456cafebabe  infer-darwin-arm64\n";
        assert_eq!(
            find_checksum(text, "infer-linux-amd64"),
            Some("abc123deadbeef".into())
        );
        assert_eq!(
            find_checksum(text, "infer-darwin-arm64"),
            Some("def456cafebabe".into())
        );
        assert_eq!(find_checksum(text, "infer-windows-amd64"), None);
    }

    #[test]
    fn test_checksum_mismatch_detected() {
        let text =
            "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8  infer-linux-amd64\n";
        let found = find_checksum(text, "infer-linux-amd64").unwrap();
        let hello_hash = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_ne!(found, hello_hash, "mismatch should be detected");
    }
}
