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

    let mut failed = Vec::new();
    for file in &files {
        let test = spec::load(file)?;
        println!("\n=== {} ({})", test.name, file.display());
        match run_test(&test, &repo_root, &artifacts, mock, &scenarios) {
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

fn run_test(
    test: &spec::Test,
    repo_root: &Path,
    artifacts: &Path,
    mock: bool,
    scenarios: &Path,
) -> Result<()> {
    let slug: String = test
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    let app = AppDriver::launch(repo_root, artifacts, &slug, mock, scenarios)?;
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
        Step::Send { send } => app.send(send),
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
        Step::Send { send } => format!("send {send:?}"),
        Step::Keypress { keypress } => format!("keypress {keypress:?}"),
        Step::Click { click } => format!("click {:?}", click.button),
        Step::Screenshot { screenshot } => format!("screenshot {screenshot}"),
        Step::AssertModel { assert_model } => format!("assert_model {assert_model}"),
        Step::AssertAbsent { assert_absent } => {
            format!("assert_absent {}", assert_absent.file.display())
        }
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
