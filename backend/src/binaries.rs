use crate::download::ProgressEvent;
use crate::env::{bin_dir, home_dir, infer_bin_path, infer_env};
use crate::stt::{find_on_path, is_executable_file};
use std::path::PathBuf;
use std::process::Output;
use tauri::ipc::Channel;

/// Where a binary stands against the latest binaries release.
#[derive(Debug, PartialEq)]
enum BinaryState {
    Current,
    Stale,
    Missing,
    Unavailable,
}

impl BinaryState {
    fn needs_install(&self) -> bool {
        matches!(self, Self::Stale | Self::Missing)
    }
}

/// The CLI-managed tools directory, ~/.infer/bin/tools.
fn tools_dir() -> PathBuf {
    bin_dir().join("tools")
}

/// Installed file name for a binary; Windows needs the `.exe` to run.
fn bin_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The CLI-managed copy of a binary in ~/.infer/bin/tools, if installed.
pub(crate) fn owned_bin(name: &str) -> Option<PathBuf> {
    let owned = tools_dir().join(bin_file_name(name));
    is_executable_file(&owned).then_some(owned)
}

/// Install stale binaries among `names`, and missing ones the release publishes
/// for this platform, through `infer binaries install`. When the CLI cannot
/// report status (offline, or a CLI without `infer binaries`), binaries already
/// in the tools dir or on PATH are kept and only a missing one is an error.
pub(crate) fn ensure(names: &[&str], on_event: &Channel<ProgressEvent>) -> Result<(), String> {
    let to_install: Vec<String> = match status(names) {
        Ok(statuses) => statuses
            .into_iter()
            .filter(|(_, state)| state.needs_install())
            .map(|(name, _)| name)
            .collect(),
        Err(_) if names.iter().all(|n| on_disk(n)) => return Ok(()),
        Err(e) => return Err(e),
    };
    if to_install.is_empty() {
        return Ok(());
    }
    let _ = on_event.send(ProgressEvent::Installing);
    let output = run_binaries("install", &to_install)?;
    if !output.status.success() {
        return Err(cli_error(&output.stderr));
    }
    Ok(())
}

/// Whether the release publishes `name` for this platform. Assumes it does when
/// the CLI cannot tell, so an install attempt surfaces the actual error.
pub(crate) fn has_prebuilt(name: &str) -> bool {
    status(&[name]).map_or(true, |statuses| {
        statuses
            .iter()
            .all(|(_, state)| *state != BinaryState::Unavailable)
    })
}

fn on_disk(name: &str) -> bool {
    owned_bin(name).or_else(|| find_on_path(name)).is_some()
}

/// `infer binaries status` exits non-zero unless every binary is current, so
/// only a run that printed no status lines counts as a failure.
fn status(names: &[&str]) -> Result<Vec<(String, BinaryState)>, String> {
    let output = run_binaries("status", names)?;
    let statuses = parse_status(&String::from_utf8_lossy(&output.stdout));
    if statuses.is_empty() {
        return Err(cli_error(&output.stderr));
    }
    Ok(statuses)
}

/// Parse the CLI's `<name> <state> <path> [(detail)]` lines. A missing binary
/// whose detail says the release has no prebuilt is `Unavailable`.
/// ponytail: parses the text table; switch if the CLI grows `--format json`.
fn parse_status(stdout: &str) -> Vec<(String, BinaryState)> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let state = match fields.next()? {
                "current" => BinaryState::Current,
                "stale" => BinaryState::Stale,
                "missing" if line.contains("(no prebuilt ") => BinaryState::Unavailable,
                "missing" => BinaryState::Missing,
                _ => return None,
            };
            Some((name.to_string(), state))
        })
        .collect()
}

/// The CLI's error from stderr, its padded `ERROR` banner flattened to one line.
fn cli_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let words: Vec<&str> = text.split_whitespace().collect();
    let message = words.strip_prefix(&["ERROR"]).unwrap_or(&words).join(" ");
    if message.is_empty() {
        "infer binaries failed without an error message".into()
    } else {
        message
    }
}

fn run_binaries(subcommand: &str, names: &[impl AsRef<std::ffi::OsStr>]) -> Result<Output, String> {
    std::process::Command::new(infer_bin_path())
        .args(["binaries", subcommand])
        .args(names)
        .env("HOME", home_dir())
        .envs(infer_env())
        .output()
        .map_err(|e| format!("Failed to run infer binaries {subcommand}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_reads_each_state() {
        let stdout = "\
ffmpeg       stale    /home/u/.infer/bin/tools/ffmpeg
whisper-cli  current  /home/u/.infer/bin/tools/whisper-cli
llama-tts    missing  /home/u/.infer/bin/tools/llama-tts
";
        assert_eq!(
            parse_status(stdout),
            vec![
                ("ffmpeg".into(), BinaryState::Stale),
                ("whisper-cli".into(), BinaryState::Current),
                ("llama-tts".into(), BinaryState::Missing),
            ]
        );
    }

    #[test]
    fn parse_status_marks_platform_without_prebuilt_unavailable() {
        let stdout = "ffmpeg       missing  /Users/u/.infer/bin/tools/ffmpeg (no prebuilt ffmpeg-darwin-amd64 in the release for darwin/amd64)\n";
        assert_eq!(
            parse_status(stdout),
            vec![("ffmpeg".into(), BinaryState::Unavailable)]
        );
        assert!(!BinaryState::Unavailable.needs_install());
    }

    #[test]
    fn parse_status_ignores_non_status_lines() {
        assert!(parse_status("").is_empty());
        assert!(parse_status("Error: unknown command \"binaries\" for \"infer\"\n").is_empty());
    }

    #[test]
    fn only_stale_and_missing_need_install() {
        assert!(BinaryState::Stale.needs_install());
        assert!(BinaryState::Missing.needs_install());
        assert!(!BinaryState::Current.needs_install());
    }

    /// Real network and CLI: seeds a stale ffmpeg in a temp HOME, then checks that
    /// `ensure` upgrades it and leaves the now-current copy alone.
    /// Run with: INFER_BIN=$HOME/.infer/bin/infer cargo test ensure_upgrades -- --ignored
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn ensure_upgrades_a_stale_binary_and_keeps_a_current_one() {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::temp_dir().join(format!("infer-binaries-{}", std::process::id()));
        let tools = home.join(".infer").join("bin").join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        unsafe { std::env::set_var("HOME", &home) };
        let ffmpeg = tools.join("ffmpeg");
        std::fs::write(&ffmpeg, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ch = Channel::new(|_| Ok(()));

        assert_eq!(
            status(&["ffmpeg"]).unwrap(),
            vec![("ffmpeg".into(), BinaryState::Stale)]
        );
        ensure(&["ffmpeg"], &ch).unwrap();
        assert_eq!(
            status(&["ffmpeg"]).unwrap(),
            vec![("ffmpeg".into(), BinaryState::Current)]
        );
        let installed = std::fs::metadata(&ffmpeg).unwrap().modified().unwrap();
        ensure(&["ffmpeg"], &ch).unwrap();
        assert_eq!(
            std::fs::metadata(&ffmpeg).unwrap().modified().unwrap(),
            installed
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn cli_error_flattens_the_error_banner() {
        let stderr = b"          \n   ERROR  \n          \n  install.sh failed for ffmpeg: exit status 1: curl: (6) Could not resolve host    \n\n";
        assert_eq!(
            cli_error(stderr),
            "install.sh failed for ffmpeg: exit status 1: curl: (6) Could not resolve host"
        );
        assert_eq!(
            cli_error(b""),
            "infer binaries failed without an error message"
        );
    }
}
