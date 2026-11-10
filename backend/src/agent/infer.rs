use crate::config::read_config;
use crate::env::{agent_cwd, home_dir, infer_bin_path, infer_env};

/// Resolve the local gateway URL from infer's config, defaulting to localhost:8080.
pub(crate) fn gateway_url() -> String {
    read_config().gateway_url
}

/// Run `infer <args>` to completion and return stdout, erroring on non-zero exit.
/// The subprocess runs on the blocking pool so the async runtime keeps serving
/// other commands while `infer` works.
pub(crate) async fn run_infer(args: &[&str]) -> Result<String, String> {
    run_infer_in(None, args).await
}

/// Like [`run_infer`], but with an explicit working directory. The CLI stores
/// conversations per-cwd, so project-scoped conversations must be addressed
/// from their project directory.
// ponytail: a vanished project dir is recreated so its cwd-slug store stays
// addressable; a `--project` flag on `infer conversations` would remove this.
pub(crate) async fn run_infer_in(cwd: Option<String>, args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || run_infer_blocking(cwd, &args))
        .await
        .map_err(|e| format!("infer task failed: {}", e))?
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

/// Synchronous form of `run_infer_in` for paths without a runtime, such as
/// the Tauri exit handler.
pub(crate) fn run_infer_blocking(cwd: Option<String>, args: &[String]) -> Result<String, String> {
    let dir = cwd
        .map(std::path::PathBuf::from)
        .filter(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or_else(agent_cwd);
    let output = infer_command(&dir)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run infer: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "infer {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[cfg(unix)]
    #[test]
    fn run_infer_in_recreates_a_vanished_project_dir() {
        let dir = TempDir::new("vanished");
        std::fs::remove_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("INFER_BIN", "/usr/bin/true") };
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_infer_in(
                Some(dir.to_string_lossy().into_owned()),
                &["conversations", "delete", "x"],
            ));
        unsafe { std::env::remove_var("INFER_BIN") };
        assert!(result.is_ok(), "{result:?}");
        assert!(dir.is_dir());
    }
}
