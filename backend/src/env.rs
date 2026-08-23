use crate::config::auth_env;
use std::path::{Path, PathBuf};

const COMPUTER_USE_GUIDANCE: &str = "Computer use: bring the intended app to the foreground and prefer the Computer accessibility action with target `frontmost`. If accessibility is temporarily unavailable immediately after launching an app, wait briefly and retry once before falling back to screenshots.";

/// Map the running platform to the CLI release asset name.
/// Returns `None` for unsupported platforms.
pub(crate) fn asset_name() -> Option<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Some("infer-linux-amd64"),
        ("linux", "aarch64") => Some("infer-linux-arm64"),
        ("macos", "x86_64") => Some("infer-darwin-amd64"),
        ("macos", "aarch64") => Some("infer-darwin-arm64"),
        ("windows", "x86_64") => Some("infer-windows-amd64"),
        ("windows", "aarch64") => Some("infer-windows-arm64"),
        _ => None,
    }
}

pub(crate) fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "infer.exe"
    } else {
        "infer"
    }
}

pub(crate) fn home_dir() -> PathBuf {
    #[cfg(unix)]
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    #[cfg(windows)]
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".into());
    PathBuf::from(home)
}

/// Path of the infer binary to spawn. INFER_BIN overrides the installed one
/// so test harnesses can point at a specific build without touching
/// ~/.infer/bin.
pub(crate) fn infer_bin_path() -> PathBuf {
    match std::env::var("INFER_BIN") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => home_dir().join(".infer").join("bin").join(binary_name()),
    }
}

/// Token-free e2e testing: DESKTOP_MOCK=true skips the desktop-owned gateway,
/// serves a canned model list, and spawns infer children with
/// INFER_GATEWAY_MOCK=true - the CLI's own mock mode, where infer serves its
/// embedded scenario gateway (see cli/internal/mockgateway).
pub(crate) fn mock_mode() -> bool {
    std::env::var("DESKTOP_MOCK").is_ok_and(|v| v == "true" || v == "1")
}

/// The Go OTLP/HTTP exporters in the CLI and gateway only speak
/// http/protobuf (the Go SDK never implemented http/json), so the collector
/// must accept protobuf; no protocol pin is set here.
pub(crate) fn collector_env() -> Vec<(String, String)> {
    vec![(
        "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
        "http://localhost:4318/".into(),
    )]
}

/// Extra env vars for spawned infer processes: provider keys, the jsonl
/// storage path pinned to the absolute directory from Settings (desktop
/// conversations are machine-global; without this a relative
/// storage.jsonl.path resolves against each child's cwd, so dev, release,
/// and the daemon would each read a different directory), plus the CLI mock
/// switch when the desktop runs in mock mode.
pub(crate) fn infer_env() -> Vec<(String, String)> {
    let mut env = auth_env();
    env.extend(collector_env());
    #[cfg(unix)]
    env.push(("PATH".into(), composed_path()));
    env.push((
        "INFER_STORAGE_JSONL_PATH".into(),
        crate::config::read_config().storage_directory,
    ));
    env.push(("INFER_COMPUTER_USE_APPROVAL".into(), "destructive".into()));
    if mock_mode() {
        env.push(("INFER_GATEWAY_MOCK".into(), "true".into()));
    }
    env
}

/// PATH for spawned infer children. Finder-launched apps inherit launchd's
/// minimal PATH (no /opt/homebrew/bin), so tools like `gh` are unresolvable
/// inside agent bash sessions; prepend the dirs launchd omits, mirroring
/// download.rs gh_bin(). Duplicates in a dev shell PATH are harmless.
#[cfg(unix)]
pub(crate) fn composed_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    let home = home_dir();
    format!(
        "/opt/homebrew/bin:/usr/local/bin:{}:{}:{base}",
        home.join(".local/bin").display(),
        home.join(".infer/bin").display()
    )
}

/// Prompt customisation env vars for the spawned agent. The CLI applies these
/// after loading prompts.yaml, so they win over any file config: the override
/// replaces the base system prompt, extras are appended after it
/// (prompts.agent.custom_instructions).
pub(crate) fn prompt_env(
    system_prompt: Option<&str>,
    extra_instructions: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut env = Vec::new();
    if let Some(sp) = system_prompt.filter(|s| !s.trim().is_empty()) {
        env.push(("INFER_PROMPTS_AGENT_SYSTEM_PROMPT", sp.to_string()));
    }
    if let Some(ei) = extra_instructions.filter(|s| !s.trim().is_empty()) {
        env.push(("INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS", ei.to_string()));
    }
    env
}

/// Append the agent's actual working directory to the custom instructions so
/// the model states it instead of guessing (Finder launches land in $HOME).
pub(crate) fn compose_extras(extra_instructions: Option<&str>, cwd: &Path) -> String {
    let desktop_notes = format!(
        "{COMPUTER_USE_GUIDANCE}\n\nCurrent working directory: {}",
        cwd.display()
    );
    match extra_instructions.filter(|s| !s.trim().is_empty()) {
        Some(ei) => format!("{ei}\n\n{desktop_notes}"),
        None => desktop_notes,
    }
}

pub(crate) fn config_path() -> PathBuf {
    home_dir().join(".infer").join("config.yaml")
}

/// Pure core of `agent_cwd`, split out so the fallback is testable.
pub(crate) fn resolve_agent_cwd(current: Option<PathBuf>, home: PathBuf) -> PathBuf {
    match current {
        Some(dir) if dir.ends_with("backend") => dir.parent().map(Path::to_path_buf).unwrap_or(dir),
        Some(dir) if dir.as_path() != std::path::Path::new("/") => dir,
        _ => home.join(".infer").join("workspace"),
    }
}

/// Working directory for spawned infer processes. `cargo tauri dev` runs the
/// app from backend/ regardless of where it was invoked, but agent sessions
/// should work in the project root - one level up. Finder-launched `.app`
/// bundles inherit cwd `/` (read-only), so infer's cwd-relative `.infer`
/// storage would land at `/.infer` and panic; fall back to ~/.infer/workspace
/// there ($HOME itself makes the agent scan TCC-protected dirs like
/// ~/Documents, prompting once per spawned process).
pub(crate) fn agent_cwd() -> PathBuf {
    let dir = resolve_agent_cwd(std::env::current_dir().ok(), home_dir());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Directory for user-uploaded/pasted images, created on first access.
pub(crate) fn uploads_dir() -> PathBuf {
    let dir = home_dir().join(".infer").join("uploads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_name_mapping() {
        assert_eq!(asset_name_for("linux", "x86_64"), Some("infer-linux-amd64"));
        assert_eq!(
            asset_name_for("linux", "aarch64"),
            Some("infer-linux-arm64")
        );
        assert_eq!(
            asset_name_for("macos", "x86_64"),
            Some("infer-darwin-amd64")
        );
        assert_eq!(
            asset_name_for("macos", "aarch64"),
            Some("infer-darwin-arm64")
        );
        assert_eq!(
            asset_name_for("windows", "x86_64"),
            Some("infer-windows-amd64")
        );
        assert_eq!(
            asset_name_for("windows", "aarch64"),
            Some("infer-windows-arm64")
        );

        assert_eq!(asset_name_for("linux", "i686"), None);
        assert_eq!(asset_name_for("freebsd", "x86_64"), None);
    }

    /// Test helper that mirrors asset_name() but takes explicit os/arch strings.
    fn asset_name_for(os: &str, arch: &str) -> Option<&'static str> {
        match (os, arch) {
            ("linux", "x86_64") => Some("infer-linux-amd64"),
            ("linux", "aarch64") => Some("infer-linux-arm64"),
            ("macos", "x86_64") => Some("infer-darwin-amd64"),
            ("macos", "aarch64") => Some("infer-darwin-arm64"),
            ("windows", "x86_64") => Some("infer-windows-amd64"),
            ("windows", "aarch64") => Some("infer-windows-arm64"),
            _ => None,
        }
    }

    #[test]
    fn test_mock_mode_parses_env() {
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
        assert!(!mock_mode());
        unsafe { std::env::set_var("DESKTOP_MOCK", "true") };
        assert!(mock_mode());
        assert!(infer_env().contains(&("INFER_GATEWAY_MOCK".to_string(), "true".to_string())));
        assert!(infer_env().contains(&(
            "INFER_COMPUTER_USE_APPROVAL".to_string(),
            "destructive".to_string()
        )));
        unsafe { std::env::set_var("DESKTOP_MOCK", "false") };
        assert!(!mock_mode());
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
    }

    /// The updater plugin looks up `{os}-{arch}` keys and hard-fails on a
    /// malformed `pub_date`, so pin the latest.json shape release.yml emits.

    #[test]
    fn test_resolve_agent_cwd_falls_back_to_workspace_at_root() {
        let home = PathBuf::from("/Users/x");
        let workspace = PathBuf::from("/Users/x/.infer/workspace");
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/")), home.clone()),
            workspace
        );
        assert_eq!(resolve_agent_cwd(None, home.clone()), workspace);
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/tmp/work")), home.clone()),
            PathBuf::from("/tmp/work")
        );
    }

    #[test]
    fn test_resolve_agent_cwd_escapes_backend() {
        let home = PathBuf::from("/Users/x");
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/repo/desktop/backend")), home.clone()),
            PathBuf::from("/repo/desktop")
        );
        assert_eq!(
            resolve_agent_cwd(Some(PathBuf::from("/repo/backend-ish")), home),
            PathBuf::from("/repo/backend-ish")
        );
    }

    #[test]
    fn prompt_env_maps_override_and_extras_to_cli_env_vars() {
        assert!(prompt_env(None, None).is_empty());
        assert!(prompt_env(Some("  "), Some("")).is_empty());
        assert_eq!(
            prompt_env(None, Some("be a pirate")),
            vec![(
                "INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS",
                "be a pirate".to_string()
            )]
        );
        assert_eq!(
            prompt_env(Some("full override"), Some("extras")),
            vec![
                (
                    "INFER_PROMPTS_AGENT_SYSTEM_PROMPT",
                    "full override".to_string()
                ),
                (
                    "INFER_PROMPTS_AGENT_CUSTOM_INSTRUCTIONS",
                    "extras".to_string()
                ),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn composed_path_prepends_launchd_omitted_dirs() {
        let path = composed_path();
        assert!(path.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
        assert!(path.contains("/.local/bin"));
        assert!(path.contains("/.infer/bin"));
    }

    #[test]
    fn compose_extras_appends_cwd_to_instructions() {
        let cwd = Path::new("/Users/edenreich/project");
        assert_eq!(
            compose_extras(None, cwd),
            format!(
                "{COMPUTER_USE_GUIDANCE}\n\nCurrent working directory: /Users/edenreich/project"
            )
        );
        assert_eq!(
            compose_extras(Some("  "), cwd),
            format!(
                "{COMPUTER_USE_GUIDANCE}\n\nCurrent working directory: /Users/edenreich/project"
            )
        );
        assert_eq!(
            compose_extras(Some("be a pirate"), cwd),
            format!(
                "be a pirate\n\n{COMPUTER_USE_GUIDANCE}\n\nCurrent working directory: /Users/edenreich/project"
            )
        );
    }
}
