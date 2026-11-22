use crate::env::{gateway_download_supported, home_dir, infer_bin_path, mock_mode};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use tauri::ipc::Channel;

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

/// Whether the desktop runs the gateway itself, so that no CLI child may run
/// one. A child that found no gateway for a moment, while the desktop was
/// restarting its own, started a second one, took the port and later stopped
/// it. On Windows the desktop cannot install the gateway, and the CLI's own
/// stays the fallback.
fn desktop_runs_gateway() -> bool {
    gateway_download_supported() && !mock_mode()
}

/// Extra env vars for spawned infer processes: provider keys, the secrets
/// assigned to this computer, the jsonl storage path resolved from
/// storage.jsonl.path when one is set (an unset path lets the CLI use its
/// home-anchored default, ~/.infer/projects/<slug>/conversations), plus the
/// CLI mock switch when the desktop runs in mock mode.
pub(crate) fn infer_env() -> Vec<(String, String)> {
    let mut env = crate::cli::config::auth_env();
    env.extend(crate::cli::secret_store::computer_env());
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
    if desktop_runs_gateway() {
        env.push((CLI_RUNS_GATEWAY.into(), "false".into()));
    }
    env
}

/// An `infer` command with the desktop's environment, run from `dir`.
pub(crate) fn infer_command(dir: &std::path::Path) -> std::process::Command {
    let mut command = crate::env::command(infer_bin_path());
    command
        .env("HOME", home_dir().to_str().unwrap_or(""))
        .envs(infer_env())
        .env("PWD", dir)
        .current_dir(dir);
    command
}

/// Runs `infer <args>` from `dir`, forwarding each stdout line to `on_line` as
/// progress. A failed run returns its trimmed stderr, which the caller prefixes
/// with the subcommand and subject it ran.
pub(crate) fn run_infer_streaming(
    dir: &Path,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    on_line: &Channel<String>,
) -> Result<(), String> {
    let mut command = infer_command(dir);
    command.args(args);
    run_streaming(command, on_line)
}

/// shortcut: stderr is read after stdout closes, fine for the CLI's one error line, a reader thread if it ever streams more than a pipe buffer.
fn run_streaming(mut command: Command, on_line: &Channel<String>) -> Result<(), String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run infer: {e}"))?;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = on_line.send(line);
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    mod streaming {
        use super::super::*;
        use std::sync::{Arc, Mutex};
        use tauri::ipc::InvokeResponseBody;

        fn sh(script: &str) -> Command {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }

        fn collecting_channel() -> (Channel<String>, Arc<Mutex<Vec<String>>>) {
            let lines = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&lines);
            let channel = Channel::new(move |body| {
                if let InvokeResponseBody::Json(json) = body {
                    let line: String =
                        serde_json::from_str(&json).expect("a sent line is a JSON string");
                    sink.lock()
                        .expect("the test holds no lock while sending")
                        .push(line);
                }
                Ok(())
            });
            (channel, lines)
        }

        #[test]
        fn a_streamed_run_forwards_each_stdout_line() {
            let (channel, lines) = collecting_channel();
            run_streaming(sh("echo one; echo two"), &channel).unwrap();
            assert_eq!(*lines.lock().unwrap(), ["one", "two"]);
        }

        #[test]
        fn a_failed_streamed_run_returns_its_trimmed_stderr() {
            let (channel, lines) = collecting_channel();
            let script = "echo progress; echo '  no such model  ' >&2; exit 3";
            let err = run_streaming(sh(script), &channel).unwrap_err();
            assert_eq!(err, "no such model");
            assert_eq!(*lines.lock().unwrap(), ["progress"]);
        }
    }

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
            gateway_download_supported(),
            "a CLI child runs no gateway where the desktop runs it"
        );
        unsafe { std::env::remove_var("DESKTOP_MOCK") };
    }
}
