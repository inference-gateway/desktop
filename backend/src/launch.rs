use crate::env::{home_dir, mock_mode};
use std::path::Path;

/// The Go OTLP/HTTP exporters in the CLI and gateway only speak
/// http/protobuf (the Go SDK never implemented http/json), so the collector
/// must accept protobuf. No protocol pin is set here.
pub(crate) fn collector_env() -> Vec<(String, String)> {
    vec![(
        "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
        "http://localhost:4318/".into(),
    )]
}

/// Absolute form of a configured `storage.jsonl.path`: a leading `~` expands
/// against `home` and a relative path anchors there, because the app's cwd is
/// not the user's project directory (Finder-launched apps inherit `/`). Applied
/// in memory only - ~/.infer/config.yaml keeps the verbatim value so the CLI's
/// cwd-anchored reading is never rewritten by a Settings save.
fn absolute_storage_path(configured: &str, home: &Path) -> String {
    if let Some(rest) = configured.strip_prefix('~') {
        return format!("{}{}", home.display(), rest);
    }
    let path = Path::new(configured);
    if path.is_relative() {
        home.join(path).display().to_string()
    } else {
        configured.to_string()
    }
}

/// The CLI's `gateway.run` as an environment variable.
const CLI_RUNS_GATEWAY: &str = "INFER_GATEWAY_RUN";

/// Extra env vars for spawned infer processes: provider keys, the secrets
/// assigned to this computer, the jsonl storage path resolved from
/// storage.jsonl.path when one is set (an unset path lets the CLI use its
/// home-anchored default, ~/.infer/projects/<slug>/conversations), plus the
/// CLI mock switch when the desktop runs in mock mode.
pub(crate) fn infer_env() -> Vec<(String, String)> {
    let mut env = crate::cli::config::auth_env();
    env.extend(crate::cli::secrets::computer_env());
    env.extend(collector_env());
    #[cfg(unix)]
    env.push(("PATH".into(), crate::env::composed_path()));
    let storage_directory = crate::cli::config::read_config().storage_directory;
    if !storage_directory.is_empty() {
        env.push((
            "INFER_STORAGE_JSONL_PATH".into(),
            absolute_storage_path(&storage_directory, &home_dir()),
        ));
    }
    if let Some(dirs) = crate::projects::sandbox_allowed_dirs() {
        env.push(("INFER_TOOLS_SANDBOX_DIRECTORIES".into(), dirs));
    }
    env.push(("INFER_COMPUTER_USE_APPROVAL".into(), "destructive".into()));
    env.push(("INFER_AGENT_MAX_TURNS".into(), "500".into()));
    if mock_mode() {
        env.push(("INFER_GATEWAY_MOCK".into(), "true".into()));
    }
    if crate::gateway::desktop_runs_gateway() {
        env.push((CLI_RUNS_GATEWAY.into(), "false".into()));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn absolute_storage_path_expands_tilde_and_anchors_relative_at_home() {
        let home = Path::new("/home/tester");
        assert_eq!(absolute_storage_path("~/conv", home), "/home/tester/conv");
        assert_eq!(
            absolute_storage_path(".infer/conversations", home),
            "/home/tester/.infer/conversations"
        );
        assert_eq!(absolute_storage_path("/abs/conv", home), "/abs/conv");
        assert_eq!(absolute_storage_path("~", home), "/home/tester");
    }

    #[test]
    fn test_mock_mode_parses_env() {
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
        assert!(!mock_mode());
        unsafe { std::env::set_var("DESKTOP_MOCK", "true") };
        assert!(mock_mode());
        assert!(infer_env().contains(&("INFER_GATEWAY_MOCK".to_string(), "true".to_string())));
        assert!(
            infer_env().iter().all(|(name, _)| name != CLI_RUNS_GATEWAY),
            "mock mode has no gateway for the desktop to run"
        );
        assert!(infer_env().contains(&(
            "INFER_COMPUTER_USE_APPROVAL".to_string(),
            "destructive".to_string()
        )));
        unsafe { std::env::set_var("DESKTOP_MOCK", "false") };
        assert!(!mock_mode());
        assert_eq!(
            infer_env().contains(&(CLI_RUNS_GATEWAY.to_string(), "false".to_string())),
            crate::gateway::gateway_download_supported(),
            "a CLI child runs no gateway where the desktop runs it"
        );
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
    }
}
