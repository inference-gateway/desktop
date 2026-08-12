use crate::env::infer_bin_path;
use crate::gateway::gateway_bin_path;
use std::io::Read;
use tauri_plugin_updater::UpdaterExt;

// --- Update checks ---
// GitHub allows 60 API requests/hour unauthenticated, 5000 through an authenticated
// `gh`. The frontend caches results for 6h, so neither ceiling is anywhere near.

#[derive(Clone, serde::Serialize)]
pub(crate) struct UpdateInfo {
    name: String,
    current: String,
    latest: String,
    outdated: bool,
}

/// Last whitespace token of the first line, minus a leading `v`. Covers
/// `infer v0.158.0`, `infer version v0.158.0` and the gateway's bare `0.44.0`.
pub(crate) fn parse_version(output: &str) -> Option<String> {
    let token = output.lines().next()?.split_whitespace().next_back()?;
    let version = token.trim_start_matches('v');
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

pub(crate) fn installed_version(bin: &std::path::Path, arg: &str) -> Option<String> {
    let out = std::process::Command::new(bin).arg(arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

/// Latest release tag for a repo, via `gh` when it is available and authenticated,
/// falling back to the public GitHub API.
pub(crate) fn latest_tag(repo: &str) -> Option<String> {
    let gh = std::process::Command::new(crate::download::gh_bin())
        .args([
            "release",
            "list",
            "--repo",
            repo,
            "--limit",
            "1",
            "--json",
            "tagName",
            "-q",
            ".[0].tagName",
        ])
        .output();
    if let Ok(out) = gh
        && out.status.success()
        && let Some(tag) = parse_version(&String::from_utf8_lossy(&out.stdout))
    {
        return Some(tag);
    }

    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let resp = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .call()
        .ok()?;
    let mut text = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut text)
        .ok()?;
    let body: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_version(body.get("tag_name")?.as_str()?)
}

/// A component is outdated when both versions are known and differ. Locally built
/// (`dev`) binaries and failed lookups never report an update.
pub(crate) fn is_outdated(current: &str, latest: &str) -> bool {
    current != "dev" && !latest.is_empty() && current != latest
}

#[tauri::command]
pub(crate) async fn check_updates(app: tauri::AppHandle) -> Result<Vec<UpdateInfo>, String> {
    let mut updates: Vec<UpdateInfo> = tokio::task::spawn_blocking(|| {
        let components = [
            ("CLI", infer_bin_path(), "inference-gateway/cli", "version"),
            (
                "Gateway",
                gateway_bin_path(),
                "inference-gateway/inference-gateway",
                "--version",
            ),
        ];
        components
            .into_iter()
            .filter_map(|(name, bin, repo, arg)| {
                let current = installed_version(&bin, arg)?;
                let latest = latest_tag(repo).unwrap_or_default();
                Some(UpdateInfo {
                    outdated: is_outdated(&current, &latest),
                    name: name.to_string(),
                    current,
                    latest,
                })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    updates.push(desktop_update_info(&app).await);
    Ok(updates)
}

// --- Desktop app self-update ---
// The Tauri updater plugin reads `latest.json` published with every GitHub
// release, verifies the bundle against the minisign public key baked into
// `plugins.updater`, swaps it in place and relaunches. Everything runs from
// Rust, so no updater/process capability is needed in the webview.

/// reqwest's `Display` drops the source chain, so flatten it: without this the
/// UI shows "error sending request for url (...)" with the actual cause
/// (timeout, dns, tls) missing.
pub(crate) fn error_chain(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    msg
}

/// Updater for the app itself. Checks use the same 10s ceiling as the
/// CLI/gateway version lookups; the install path gets 300s because the
/// timeout is a total-request deadline that also covers the bundle download.
pub(crate) fn desktop_updater(
    app: &tauri::AppHandle,
    timeout_secs: u64,
) -> Result<tauri_plugin_updater::Updater, String> {
    app.updater_builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| error_chain(&e))
}

/// Version pair for the desktop app. Debug builds report `dev`, so a locally
/// built app never offers to replace itself with a release bundle.
pub(crate) async fn desktop_update_info(app: &tauri::AppHandle) -> UpdateInfo {
    let current = if cfg!(debug_assertions) {
        "dev".to_string()
    } else {
        app.package_info().version.to_string()
    };
    let latest = match desktop_updater(app, 10) {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => update.version,
            Ok(None) => current.clone(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };
    UpdateInfo {
        outdated: is_outdated(&current, &latest),
        name: "Desktop".to_string(),
        current,
        latest,
    }
}

/// Download, verify and install the desktop app update, then relaunch. Does not
/// return on success.
#[tauri::command]
pub(crate) async fn install_desktop_update(app: tauri::AppHandle) -> Result<(), String> {
    let update = desktop_updater(&app, 300)?
        .check()
        .await
        .map_err(|e| error_chain(&e))?
        .ok_or("Desktop app is already up to date")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| error_chain(&e))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("infer v0.158.0\n"), Some("0.158.0".into()));
        assert_eq!(
            parse_version("infer version v0.158.0\n"),
            Some("0.158.0".into())
        );
        assert_eq!(parse_version("0.44.0\n"), Some("0.44.0".into()));
        assert_eq!(parse_version("v0.44.0\n"), Some("0.44.0".into()));
        assert_eq!(parse_version("dev\n"), Some("dev".into()));
        assert_eq!(
            parse_version("infer v0.158.0\nextra line\n"),
            Some("0.158.0".into())
        );
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("  \n"), None);
        assert_eq!(parse_version("v\n"), None);
    }

    #[test]
    fn test_error_chain() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "error sending request for url")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let e = Outer(std::io::Error::other("operation timed out"));
        assert_eq!(
            error_chain(&e),
            "error sending request for url: operation timed out"
        );
    }

    #[test]
    fn test_is_outdated() {
        assert!(is_outdated("0.158.0", "0.158.1"));
        assert!(!is_outdated("0.158.1", "0.158.1"));
        assert!(!is_outdated("dev", "0.158.1"));
        assert!(!is_outdated("0.158.0", ""));
    }

    /// macOS ships one universal bundle under both darwin keys.
    #[test]
    fn test_latest_json_manifest_shape() {
        let manifest = serde_json::json!({
            "version": "0.2.3",
            "pub_date": "2026-08-08T12:00:00Z",
            "platforms": {
                "darwin-aarch64": {"signature": "mac-sig", "url": "https://example.com/App_0.2.3_universal.app.tar.gz"},
                "darwin-x86_64": {"signature": "mac-sig", "url": "https://example.com/App_0.2.3_universal.app.tar.gz"},
                "linux-x86_64": {"signature": "linux-sig", "url": "https://example.com/App_0.2.3_amd64.AppImage"},
                "windows-x86_64": {"signature": "win-sig", "url": "https://example.com/App_0.2.3_x64-setup.exe"}
            }
        });

        let release: tauri_plugin_updater::RemoteRelease =
            serde_json::from_value(manifest).expect("latest.json must parse");

        assert_eq!(release.version.to_string(), "0.2.3");
        for target in [
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
            "windows-x86_64",
        ] {
            assert!(release.download_url(target).is_ok(), "no url for {target}");
            assert!(release.signature(target).is_ok(), "no sig for {target}");
        }
    }
}
