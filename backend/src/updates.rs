use crate::env::infer_bin_path;
use crate::gateway::gateway_bin_path;
use std::io::Read;
use tauri_plugin_updater::UpdaterExt;

/// Release tags are spelled `v0.63.1`, so download URLs must not drop the `v`
/// the way `parse_version` does for display.
const DESKTOP_REPO: &str = "inference-gateway/desktop";

// --- Update checks ---
// GitHub allows 60 API requests/hour unauthenticated, 5000 through an authenticated
// `gh`. The frontend caches results for 6h, so neither ceiling is anywhere near.

#[derive(Clone, serde::Serialize)]
pub(crate) struct UpdateInfo {
    name: String,
    current: String,
    latest: String,
    outdated: bool,
    /// Raw release tags of the repo, newest first and capped at 5. Unlike
    /// `latest` they keep their `v` prefix, so download URLs can use them as
    /// `releases/download/<tag>/<asset>`.
    versions: Vec<String>,
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
    let out = crate::env::command(bin).arg(arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

/// Most recent release tags for a repo, newest first and capped at 5, via
/// `gh` when it is available and authenticated, falling back to the public
/// GitHub API.
pub(crate) fn recent_tags(repo: &str) -> Vec<String> {
    let gh = crate::env::command(crate::download::gh_bin())
        .args([
            "release",
            "list",
            "--repo",
            repo,
            "--limit",
            "5",
            "--json",
            "tagName",
            "-q",
            ".[].tagName",
        ])
        .output();
    if let Ok(out) = gh
        && out.status.success()
    {
        let tags = tags_from_lines(&String::from_utf8_lossy(&out.stdout));
        if !tags.is_empty() {
            return tags;
        }
    }
    api_recent_tags(repo).unwrap_or_default()
}

/// Release tags of a repo through the public GitHub API. `releases` lists
/// newest first, like `gh release list` does.
fn api_recent_tags(repo: &str) -> Option<Vec<String>> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=5");
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
    let releases: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(
        releases
            .as_array()?
            .iter()
            .filter_map(|release| release.get("tag_name")?.as_str().map(str::to_string))
            .collect(),
    )
}

/// One raw tag per line, as `gh release list --json tagName` prints.
fn tags_from_lines(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

/// A component is outdated when both versions are known and differ. Locally built
/// (`dev`) binaries and failed lookups never report an update.
pub(crate) fn is_outdated(current: &str, latest: &str) -> bool {
    current != "dev" && !latest.is_empty() && current != latest
}

/// `latest` derived from the raw tag list: the newest tag, normalized the
/// way `parse_version` normalizes `current` (`v` stripped), so an
/// up-to-date component compares equal against its own tag.
fn latest_from_tags(versions: &[String]) -> String {
    versions
        .first()
        .and_then(|tag| parse_version(tag))
        .unwrap_or_default()
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
                let versions = recent_tags(repo);
                let latest = latest_from_tags(&versions);
                Some(UpdateInfo {
                    outdated: is_outdated(&current, &latest),
                    name: name.to_string(),
                    current,
                    latest,
                    versions,
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
pub(crate) fn desktop_updater_builder(
    app: &tauri::AppHandle,
    timeout_secs: u64,
) -> Result<tauri_plugin_updater::UpdaterBuilder, String> {
    Ok(app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(timeout_secs)))
}

pub(crate) fn desktop_updater(
    app: &tauri::AppHandle,
    timeout_secs: u64,
) -> Result<tauri_plugin_updater::Updater, String> {
    desktop_updater_builder(app, timeout_secs)?
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
    // `recent_tags` shells out to `gh` / blocks on HTTP, so it stays off the
    // async executor like the CLI and gateway lookups above.
    let versions = tokio::task::spawn_blocking(|| recent_tags(DESKTOP_REPO))
        .await
        .unwrap_or_default();
    UpdateInfo {
        outdated: is_outdated(&current, &latest),
        name: "Desktop".to_string(),
        current,
        latest,
        versions,
    }
}

/// Download, verify and install the desktop app update, then relaunch. Does not
/// return on success. `tag` pins the updater to that release's manifest, and
/// drops the default newer-than-current rule, so an older release can be
/// installed over a bad update.
#[tauri::command]
pub(crate) async fn install_desktop_update(
    app: tauri::AppHandle,
    tag: Option<String>,
) -> Result<(), String> {
    let builder = desktop_updater_builder(&app, 300)?;
    let updater = match &tag {
        Some(tag) => {
            let manifest = url::Url::parse(&format!(
                "https://github.com/{DESKTOP_REPO}/releases/download/{tag}/latest.json"
            ))
            .map_err(|e| e.to_string())?;
            // Same rule the plugin applies for `allowDowngrades`.
            builder
                .endpoints(vec![manifest])
                .map_err(|e| error_chain(&e))?
                .version_comparator(|current, release| release.version != current)
        }
        None => builder,
    }
    .build()
    .map_err(|e| error_chain(&e))?;
    let update = updater
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
    fn test_tags_from_lines() {
        assert_eq!(
            tags_from_lines("v0.249.0\nv0.248.0\n"),
            vec!["v0.249.0".to_string(), "v0.248.0".to_string()]
        );
        assert_eq!(
            tags_from_lines("\n v0.1.0  \n\n"),
            vec!["v0.1.0".to_string()]
        );
        assert!(tags_from_lines("").is_empty());
    }

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

    #[test]
    fn test_latest_from_tags() {
        assert_eq!(latest_from_tags(&["v0.249.0".into()]), "0.249.0");
        let tags = vec!["v0.249.0".to_string(), "v0.248.0".to_string()];
        let latest = latest_from_tags(&tags);
        // An up-to-date pair must compare equal even though the raw tag
        // keeps its `v` prefix.
        assert!(!is_outdated("0.249.0", &latest));
        assert!(is_outdated("0.248.0", &latest));
        assert!(latest_from_tags(&[]).is_empty());
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
