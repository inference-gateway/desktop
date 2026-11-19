use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

const IMAGE: &str = "desktop-e2e";
const TARGET_VOLUME: &str = "desktop-e2e-target";
const REGISTRY_VOLUME: &str = "desktop-e2e-cargo-registry";
const DISPLAY_SIZE: &str = "1280x800x24";

/// Run the suite inside a Podman container: the Linux build under Xvfb, driven
/// over WebDriver, so a run never touches this machine's screen, mouse or app.
/// The repo is mounted at /src, so the specs, scenarios and artifacts are the
/// host's, and the Linux build keeps its own target volume.
pub fn run(repo_root: &Path, args: &[String]) -> Result<bool> {
    let built = Command::new("podman")
        .args(["build", "--quiet", "-t", IMAGE, "-f"])
        .arg(repo_root.join("e2e/Containerfile"))
        .arg(repo_root.join("e2e"))
        .status()
        .context("running podman build (is podman installed and its machine started?)")?;
    if !built.success() {
        bail!("podman build failed");
    }
    let status = Command::new("podman")
        .args(["run", "--rm", "--init", "-v"])
        .arg(format!("{}:/src", repo_root.display()))
        .args(["-v", &format!("{TARGET_VOLUME}:/target")])
        .args([
            "-v",
            &format!("{REGISTRY_VOLUME}:/usr/local/cargo/registry"),
        ])
        .args(["-e", "CARGO_TARGET_DIR=/target"])
        .args(["-e", "DESKTOP_BIN=/target/debug/inference-gateway-desktop"])
        .args(["-w", "/src/e2e", IMAGE, "sh", "-c"])
        .arg(format!(
            "Xvfb :99 -screen 0 {DISPLAY_SIZE} >/dev/null 2>&1 & exec cargo run --quiet -- \"$@\""
        ))
        .arg("e2e")
        .args(args)
        .status()
        .context("running podman")?;
    Ok(status.success())
}
