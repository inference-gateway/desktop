//! YAML-driven e2e runner for the desktop app. See tests/*.yaml for the
//! spec format and AGENTS.md ("Verifying the UI") for the AX foundations.
//!
//! Usage: e2e [--no-build] [--no-mock] [files...]

mod driver;
mod spec;

use anyhow::{Context, Result, bail};
use driver::AppDriver;
use spec::{BareStep, Step};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<bool> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .context("locating repo root")?
        .to_path_buf();
    let artifacts = manifest_dir.join("artifacts");
    let scenarios = manifest_dir.join("scenarios.yaml");

    let mut build = true;
    let mut mock = true;
    let mut files: Vec<PathBuf> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-build" => build = false,
            "--no-mock" => mock = false,
            _ if arg.starts_with("--") => bail!("unknown flag {arg}"),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        let mut entries: Vec<_> = std::fs::read_dir(manifest_dir.join("tests"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
            .collect();
        entries.sort();
        files = entries;
    }

    if build {
        println!("building app...");
        let status = Command::new("cargo")
            .arg("build")
            .current_dir(repo_root.join("backend"))
            .status()
            .context("running cargo build (is the flox env active?)")?;
        if !status.success() {
            bail!("cargo build failed");
        }
    }

    let infer_bin = resolve_infer(&artifacts);

    let mut failed = Vec::new();
    for file in &files {
        let test = spec::load(file)?;
        println!("\n=== {} ({})", test.name, file.display());
        match run_test(
            &test,
            &repo_root,
            &artifacts,
            mock,
            &scenarios,
            infer_bin.as_deref(),
        ) {
            Ok(()) => println!("PASS {}", test.name),
            Err(e) => {
                println!("FAIL {}: {e:#}", test.name);
                failed.push(test.name.clone());
            }
        }
    }

    println!(
        "\n{} passed, {} failed",
        files.len() - failed.len(),
        failed.len()
    );
    for name in &failed {
        println!("  failed: {name}");
    }
    Ok(failed.is_empty())
}

/// Resolve the infer binary for the whole suite: an explicit INFER_BIN wins,
/// otherwise download the latest release once (the version users actually
/// run - a pinned dev binary goes stale the day a new CLI ships), falling
/// back to the dev environment's PATH when the download fails (offline).
fn resolve_infer(artifacts: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("INFER_BIN") {
        println!("using INFER_BIN override: {}", PathBuf::from(&p).display());
        return Some(PathBuf::from(p));
    }
    let asset = match std::env::consts::ARCH {
        "aarch64" => "infer-darwin-arm64",
        _ => "infer-darwin-amd64",
    };
    let url = format!("https://github.com/inference-gateway/cli/releases/latest/download/{asset}");
    let dir = artifacts.join("infer-latest");
    let bin = dir.join("infer");
    let downloaded = std::fs::create_dir_all(&dir).is_ok()
        && Command::new("curl")
            .args(["-fsSL", "--retry", "2", "-o"])
            .arg(&bin)
            .arg(&url)
            .status()
            .is_ok_and(|s| s.success())
        && Command::new("chmod")
            .arg("+x")
            .arg(&bin)
            .status()
            .is_ok_and(|s| s.success());
    if downloaded {
        let version = Command::new(&bin)
            .arg("--version")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        println!("using latest released infer: {version}");
        return Some(bin);
    }
    println!(
        "warning: could not download the latest infer release - falling back to the dev environment's (possibly stale) binary"
    );
    None
}

fn run_test(
    test: &spec::Test,
    repo_root: &Path,
    artifacts: &Path,
    mock: bool,
    scenarios: &Path,
    infer_bin: Option<&Path>,
) -> Result<()> {
    let slug: String = test
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    let app = AppDriver::launch(repo_root, artifacts, &slug, mock, scenarios, infer_bin)?;
    clean(&app, &test.cleanup);

    let result = run_steps(test, &app);
    if result.is_err() {
        match app.screenshot(&format!("{slug}-fail")) {
            Ok(p) => println!("  screenshot: {}", p.display()),
            Err(e) => println!("  (no failure screenshot: {e})"),
        }
    }
    clean(&app, &test.cleanup);
    result
}

fn clean(app: &AppDriver, paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(app.resolve(p));
    }
}

fn run_steps(test: &spec::Test, app: &AppDriver) -> Result<()> {
    for (i, step) in test.steps.iter().enumerate() {
        let label = describe(step);
        step_run(app, step).with_context(|| format!("step {} ({label})", i + 1))?;
        println!("  ok  {label}");
    }
    Ok(())
}

fn step_run(app: &AppDriver, step: &Step) -> Result<()> {
    match step {
        Step::Bare(BareStep::NewChat) => app.new_chat(),
        Step::Bare(BareStep::AssertOverlayBounds) => {
            let found = app.poll(Duration::from_secs(5), || {
                app.overlay_matches_primary_screen()
            })?;
            if !found {
                bail!(
                    "computer-use overlay does not span the primary screen from the menu bar to the bottom edge"
                );
            }
            Ok(())
        }
        Step::Send { send } => app.send(send),
        Step::Type { text } => app.type_text(text),
        Step::Keypress { keypress } => app.keypress(keypress),
        Step::Click { click } => app.click(&click.button),
        Step::Screenshot { screenshot } => app.screenshot(screenshot).map(|_| ()),
        Step::AssertModel { assert_model } => {
            let got = app.model_value()?;
            if got != *assert_model {
                bail!("model is {got:?}, expected {assert_model:?}");
            }
            Ok(())
        }
        Step::AssertAbsent { assert_absent } => {
            let path = app.resolve(&assert_absent.file);
            if path.exists() {
                bail!("{} exists but should not", path.display());
            }
            Ok(())
        }
        Step::AssertAboveComposer {
            assert_above_composer,
        } => {
            if !app.button_above_composer(&assert_above_composer.button)? {
                bail!(
                    "button {:?} overflows below the composer - transcript content is \
                     floating outside the conversation area",
                    assert_above_composer.button
                );
            }
            Ok(())
        }
        Step::WaitFor { wait_for } => {
            let timeout = Duration::from_secs(wait_for.timeout);
            let found = if let Some(button) = &wait_for.button {
                app.poll(timeout, || app.button_exists(button))?
            } else if let Some(text) = &wait_for.text {
                app.poll(timeout, || app.text_exists(text))?
            } else if let Some(file) = &wait_for.file {
                let path = app.resolve(file);
                app.poll(timeout, || Ok(path.exists()))?
            } else {
                bail!("wait_for needs one of button/text/file");
            };
            if !found {
                bail!("timed out after {}s", wait_for.timeout);
            }
            Ok(())
        }
    }
}

fn describe(step: &Step) -> String {
    match step {
        Step::Bare(BareStep::NewChat) => "new_chat".into(),
        Step::Bare(BareStep::AssertOverlayBounds) => "assert_overlay_bounds".into(),
        Step::Send { send } => format!("send {send:?}"),
        Step::Type { text } => format!("type {text:?}"),
        Step::Keypress { keypress } => format!("keypress {keypress:?}"),
        Step::Click { click } => format!("click {:?}", click.button),
        Step::Screenshot { screenshot } => format!("screenshot {screenshot}"),
        Step::AssertModel { assert_model } => format!("assert_model {assert_model}"),
        Step::AssertAbsent { assert_absent } => {
            format!("assert_absent {}", assert_absent.file.display())
        }
        Step::AssertAboveComposer {
            assert_above_composer,
        } => format!(
            "assert_above_composer button {:?}",
            assert_above_composer.button
        ),
        Step::WaitFor { wait_for } => {
            let target = wait_for
                .button
                .as_deref()
                .map(|b| format!("button {b:?}"))
                .or_else(|| wait_for.text.as_deref().map(|t| format!("text {t:?}")))
                .or_else(|| {
                    wait_for
                        .file
                        .as_ref()
                        .map(|f| format!("file {}", f.display()))
                })
                .unwrap_or_else(|| "?".into());
            format!("wait_for {target} (≤{}s)", wait_for.timeout)
        }
    }
}
