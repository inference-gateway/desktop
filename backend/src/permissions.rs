use crate::env::{agent_cwd, home_dir, mock_mode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const COMPUTER_USE_CONFIG_FILE: &str = "computer_use.yaml";
const COMPUTER_USE_ENABLED_ENV: &str = "INFER_COMPUTER_USE_ENABLED";

// Mock mode (DESKTOP_MOCK=true) simulates the grant flow so dev sessions and
// e2e can exercise the Settings UI without touching TCC: statuses start
// not_granted and flip to granted when the request commands run.
static MOCK_ACCESSIBILITY_GRANTED: AtomicBool = AtomicBool::new(false);
static MOCK_SCREEN_RECORDING_GRANTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OsPermissionState {
    Granted,
    NotGranted,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
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

#[cfg(any(target_os = "macos", test))]
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
    use std::ffi::c_void;
    use std::ptr;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
        static kCFBooleanTrue: *const c_void;
    }

    pub(super) fn permission_status() -> (bool, bool) {
        let accessibility = unsafe { AXIsProcessTrusted() != 0 };
        let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
        (accessibility, screen_recording)
    }

    pub(super) fn request_accessibility() {
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                ptr::null(),
                ptr::null(),
            );
            AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
        }
    }

    pub(super) fn request_screen_recording() {
        unsafe {
            CGRequestScreenCaptureAccess();
        }
    }
}

fn write_computer_use_enabled(path: &Path, enabled: bool) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, format!("enabled: {enabled}\n")).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_computer_use_enabled(enabled: bool) -> Result<(), String> {
    write_computer_use_enabled(
        &effective_computer_use_config(&agent_cwd(), &home_dir()),
        enabled,
    )
}

#[tauri::command]
pub(crate) fn computer_use_permission_status() -> ComputerUsePermissionStatus {
    let computer_use_enabled = computer_use_enabled(
        &agent_cwd(),
        &home_dir(),
        std::env::var(COMPUTER_USE_ENABLED_ENV).ok().as_deref(),
    );
    if computer_use_enabled && mock_mode() {
        return ComputerUsePermissionStatus {
            computer_use_enabled,
            accessibility: MOCK_ACCESSIBILITY_GRANTED.load(Ordering::SeqCst).into(),
            screen_recording: MOCK_SCREEN_RECORDING_GRANTED.load(Ordering::SeqCst).into(),
        };
    }
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

#[tauri::command]
pub(crate) fn request_accessibility_permission() {
    if mock_mode() {
        MOCK_ACCESSIBILITY_GRANTED.store(true, Ordering::SeqCst);
    } else {
        #[cfg(target_os = "macos")]
        macos::request_accessibility();
    }
}

#[tauri::command]
pub(crate) fn request_screen_recording_permission() {
    if mock_mode() {
        MOCK_SCREEN_RECORDING_GRANTED.store(true, Ordering::SeqCst);
    } else {
        #[cfg(target_os = "macos")]
        macos::request_screen_recording();
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
    fn writes_enabled_flag_readable_by_parser() {
        let path = std::env::temp_dir()
            .join(format!("inference-gateway-cu-write-{}", std::process::id()))
            .join(COMPUTER_USE_CONFIG_FILE);
        write_computer_use_enabled(&path, true).unwrap();
        assert!(enabled_from_yaml(&std::fs::read_to_string(&path).unwrap()));
        write_computer_use_enabled(&path, false).unwrap();
        assert!(!enabled_from_yaml(&std::fs::read_to_string(&path).unwrap()));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn parses_cli_boolean_overrides() {
        for value in ["1", "true", "TRUE", " True "] {
            assert_eq!(parse_bool_override(value), Some(true));
        }
        for value in ["0", "false", "FALSE", " False "] {
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
