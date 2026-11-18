use std::path::{Path, PathBuf};

/// Map the running platform to the CLI release asset, a tarball holding the
/// executable as `infer` (`infer.exe` on Windows). `None` for unsupported platforms.
pub(crate) fn asset_name() -> Option<&'static str> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_name_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("infer-linux-amd64.tar.gz"),
        ("linux", "aarch64") => Some("infer-linux-arm64.tar.gz"),
        ("macos", "x86_64") => Some("infer-darwin-amd64.tar.gz"),
        ("macos", "aarch64") => Some("infer-darwin-arm64.tar.gz"),
        ("windows", "x86_64") => Some("infer-windows-amd64.tar.gz"),
        ("windows", "aarch64") => Some("infer-windows-arm64.tar.gz"),
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

pub(crate) fn gateway_download_supported() -> bool {
    !cfg!(target_os = "windows")
}

pub(crate) fn home_dir() -> PathBuf {
    #[cfg(unix)]
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    #[cfg(windows)]
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".into());
    PathBuf::from(home)
}

/// The infer home directory's name: config, auth, skills, agents, avatars,
/// bin, history, tmp, schedules and the daemon file all live under
/// `~/.infer` (per-project stores root at the project cwd under the same
/// name).
pub(crate) const INFER_DIR_NAME: &str = ".infer";

/// The infer home directory rooted at an arbitrary base.
pub(crate) fn infer_dir_in(home: &Path) -> PathBuf {
    home.join(INFER_DIR_NAME)
}

/// The infer home directory, `~/.infer`.
pub(crate) fn infer_dir() -> PathBuf {
    infer_dir_in(&home_dir())
}

/// Path of the infer binary to spawn. INFER_BIN overrides the installed one
/// so test harnesses can point at a specific build without touching
/// ~/.infer/bin.
pub(crate) fn infer_bin_path() -> PathBuf {
    match std::env::var("INFER_BIN") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => bin_dir().join(binary_name()),
    }
}

/// A child process command as the platform wants it. From a windows GUI app
/// every spawned console program gets its own console window that flashes
/// open and closed for the child's lifetime, so CREATE_NO_WINDOW suppresses
/// the console for each spawned child.
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

/// Token-free e2e testing: DESKTOP_MOCK=true skips the desktop-owned gateway,
/// takes the model list from the scenarios file named by
/// INFER_GATEWAY_MOCK_SCENARIOS (see agent::models::mock_models), and spawns infer
/// children with INFER_GATEWAY_MOCK=true - the CLI's own mock mode, where
/// infer serves its embedded scenario gateway (see cli/internal/mockgateway).
pub(crate) fn mock_mode() -> bool {
    std::env::var("DESKTOP_MOCK").is_ok_and(|v| v == "true" || v == "1")
}

/// PATH for spawned infer children and PATH lookups in this process. On unix,
/// Finder-launched apps inherit launchd's minimal PATH (no /opt/homebrew/bin),
/// so tools like `gh` are unresolvable inside agent bash sessions - prepend
/// the dirs launchd omits. Windows GUI launches already inherit the user's
/// normal PATH, so pass it through.
pub(crate) fn composed_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    if cfg!(windows) {
        return base;
    }
    let home = home_dir();
    format!(
        "/opt/homebrew/bin:/usr/local/bin:{}:{}:{base}",
        home.join(".local/bin").display(),
        infer_dir_in(&home).join("bin").display()
    )
}

pub(crate) fn config_path() -> PathBuf {
    infer_dir().join("config.yaml")
}

pub(crate) const DAEMON_FILE: &str = "daemon.yaml";

/// Where the daemon's own configuration lives, `~/.infer/daemon.yaml`.
pub(crate) fn daemon_config_path() -> PathBuf {
    infer_dir().join(DAEMON_FILE)
}

/// Install directory for the CLI, the gateway binary and desktop tools (`~/.infer/bin`).
pub(crate) fn bin_dir() -> PathBuf {
    infer_dir().join("bin")
}

/// Pure core of `agent_cwd`, split out so the fallback is testable.
pub(crate) fn resolve_agent_cwd(current: Option<PathBuf>, home: PathBuf) -> PathBuf {
    match current {
        Some(dir) if dir.ends_with("backend") => dir.parent().map(Path::to_path_buf).unwrap_or(dir),
        Some(dir) if dir.as_path() != std::path::Path::new("/") => dir,
        _ => infer_dir_in(&home).join("workspace"),
    }
}

/// Working directory for spawned infer processes. `cargo tauri dev` runs the
/// app from backend/ regardless of invocation, but agent sessions should work
/// in the project root - one level up. Finder-launched `.app` bundles inherit
/// cwd `/` (read-only), so infer's cwd-relative `.infer` storage would land at
/// `/.infer` and panic. Fall back to ~/.infer/workspace there.
pub(crate) fn agent_cwd() -> PathBuf {
    let dir = resolve_agent_cwd(std::env::current_dir().ok(), home_dir());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Directory for user-uploaded/pasted images (`~/.infer/tmp/uploads`), created on first access.
pub(crate) fn uploads_dir() -> PathBuf {
    let dir = infer_dir().join("tmp").join("uploads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_name_mapping() {
        assert_eq!(
            asset_name_for("linux", "x86_64"),
            Some("infer-linux-amd64.tar.gz")
        );
        assert_eq!(
            asset_name_for("linux", "aarch64"),
            Some("infer-linux-arm64.tar.gz")
        );
        assert_eq!(
            asset_name_for("macos", "x86_64"),
            Some("infer-darwin-amd64.tar.gz")
        );
        assert_eq!(
            asset_name_for("macos", "aarch64"),
            Some("infer-darwin-arm64.tar.gz")
        );
        assert_eq!(
            asset_name_for("windows", "x86_64"),
            Some("infer-windows-amd64.tar.gz")
        );
        assert_eq!(
            asset_name_for("windows", "aarch64"),
            Some("infer-windows-arm64.tar.gz")
        );

        assert_eq!(asset_name_for("linux", "i686"), None);
        assert_eq!(asset_name_for("freebsd", "x86_64"), None);
    }

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

    #[cfg(unix)]
    #[test]
    fn composed_path_prepends_launchd_omitted_dirs() {
        let path = composed_path();
        assert!(path.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
        assert!(path.contains("/.local/bin"));
        assert!(path.contains("/.infer/bin"));
    }

    /// The console suppression exists only on windows, so run a real spawn on
    /// a windows runner: the child must report that it has no console window.
    #[cfg(windows)]
    #[test]
    fn windows_spawns_have_no_console_window() {
        const PROBE: &str = "Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class ConsoleProbe{[DllImport(\"kernel32.dll\")]public static extern IntPtr GetConsoleWindow();}'; [ConsoleProbe]::GetConsoleWindow().Equals([IntPtr]::Zero)";
        let output = command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", PROBE])
            .output()
            .expect("powershell probe should spawn");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "True",
            "child spawned through command() reported a console window: {:?}",
            output
        );
    }
}
