use crate::env::{agent_cwd, home_dir};
use std::path::{Path, PathBuf};

const COMPUTER_USE_CONFIG_FILE: &str = "computer_use.yaml";
const COMPUTER_USE_ENABLED_ENV: &str = "INFER_COMPUTER_USE_ENABLED";

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OsPermissionState {
    Granted,
    NotGranted,
    Unavailable,
    NotApplicable,
}

impl From<bool> for OsPermissionState {
    fn from(granted: bool) -> Self {
        if granted {
            Self::Granted
        } else {
            Self::NotGranted
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ComputerUsePermissionStatus {
    computer_use_enabled: bool,
    accessibility: OsPermissionState,
    screen_recording: OsPermissionState,
}

fn parse_bool_override(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "t" | "true" => Some(true),
        "0" | "f" | "false" => Some(false),
        _ => None,
    }
}

fn enabled_from_yaml(text: &str) -> bool {
    serde_norway::from_str::<serde_norway::Value>(text)
        .ok()
        .and_then(|value| value.get("enabled").and_then(serde_norway::Value::as_bool))
        .unwrap_or(false)
}

fn effective_computer_use_config(agent_dir: &Path, home: &Path) -> PathBuf {
    let project_config = agent_dir.join(".infer").join(COMPUTER_USE_CONFIG_FILE);
    if project_config.is_file() {
        return project_config;
    }
    home.join(".infer").join(COMPUTER_USE_CONFIG_FILE)
}

fn computer_use_enabled(agent_dir: &Path, home: &Path, env_override: Option<&str>) -> bool {
    if let Some(enabled) = env_override.and_then(parse_bool_override) {
        return enabled;
    }
    std::fs::read_to_string(effective_computer_use_config(agent_dir, home))
        .ok()
        .is_some_and(|text| enabled_from_yaml(&text))
}

fn runs_from_app_bundle(executable: &Path) -> bool {
    executable
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "MacOS"))
        .and_then(Path::parent)
        .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
        .and_then(Path::parent)
        .is_some_and(|path| path.extension().is_some_and(|extension| extension == "app"))
}

#[cfg(target_os = "macos")]
mod macos {
    // These declarations mirror Apple's stable, argument-free permission queries.
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }

    pub(super) fn permission_status() -> (bool, bool) {
        let accessibility = unsafe { AXIsProcessTrusted() != 0 };
        let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
        (accessibility, screen_recording)
    }
}

#[tauri::command]
pub(crate) fn computer_use_permission_status() -> ComputerUsePermissionStatus {
    let computer_use_enabled = computer_use_enabled(
        &agent_cwd(),
        &home_dir(),
        std::env::var(COMPUTER_USE_ENABLED_ENV).ok().as_deref(),
    );
    #[cfg(target_os = "macos")]
    let (accessibility, screen_recording) = if !computer_use_enabled {
        (
            OsPermissionState::NotApplicable,
            OsPermissionState::NotApplicable,
        )
    } else if std::env::current_exe()
        .ok()
        .is_some_and(|executable| runs_from_app_bundle(&executable))
    {
        let (accessibility, screen_recording) = macos::permission_status();
        (accessibility.into(), screen_recording.into())
    } else {
        (
            OsPermissionState::Unavailable,
            OsPermissionState::Unavailable,
        )
    };
    #[cfg(not(target_os = "macos"))]
    let (accessibility, screen_recording) = (
        OsPermissionState::NotApplicable,
        OsPermissionState::NotApplicable,
    );

    ComputerUsePermissionStatus {
        computer_use_enabled,
        accessibility,
        screen_recording,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_computer_use_enabled() {
        assert!(enabled_from_yaml("---\nenabled: true\n"));
        assert!(!enabled_from_yaml("---\nenabled: false\n"));
        assert!(!enabled_from_yaml("---\nscreenshot:\n  enabled: true\n"));
        assert!(!enabled_from_yaml("not: valid: yaml: ["));
    }

    #[test]
    fn parses_cli_boolean_overrides() {
        for value in ["1", "t", "T", "true", "TRUE", " True "] {
            assert_eq!(parse_bool_override(value), Some(true));
        }
        for value in ["0", "f", "F", "false", "FALSE", " False "] {
            assert_eq!(parse_bool_override(value), Some(false));
        }
        assert_eq!(parse_bool_override("enabled"), None);
    }

    #[test]
    fn recognizes_macos_app_bundle_executables() {
        assert!(runs_from_app_bundle(Path::new(
            "/Applications/Inference Gateway Desktop.app/Contents/MacOS/inference-gateway-desktop"
        )));
        assert!(runs_from_app_bundle(Path::new(
            "/tmp/debug/Inference Gateway Desktop.app/Contents/MacOS/inference-gateway-desktop"
        )));
        assert!(!runs_from_app_bundle(Path::new(
            "/workspace/target/debug/inference-gateway-desktop"
        )));
    }

    #[test]
    fn environment_and_project_config_take_precedence() {
        let root = std::env::temp_dir().join(format!(
            "inference-gateway-permissions-test-{}",
            std::process::id()
        ));
        let agent_dir = root.join("project");
        let home = root.join("home");
        std::fs::create_dir_all(agent_dir.join(".infer")).unwrap();
        std::fs::create_dir_all(home.join(".infer")).unwrap();
        std::fs::write(
            home.join(".infer").join(COMPUTER_USE_CONFIG_FILE),
            "enabled: true\n",
        )
        .unwrap();

        assert!(computer_use_enabled(&agent_dir, &home, None));

        std::fs::write(
            agent_dir.join(".infer").join(COMPUTER_USE_CONFIG_FILE),
            "enabled: false\n",
        )
        .unwrap();

        assert!(!computer_use_enabled(&agent_dir, &home, None));
        assert!(computer_use_enabled(&agent_dir, &home, Some("true")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
