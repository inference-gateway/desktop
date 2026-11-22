#[cfg(not(target_os = "linux"))]
mod ax_helper;
#[cfg(not(target_os = "linux"))]
mod driver;
#[cfg(target_os = "linux")]
#[path = "webdriver.rs"]
mod driver;
mod home;
mod podman;
mod spec;

use anyhow::{Context, Result, bail};
use driver::AppDriver;
use home::Seeds;
use spec::{BareStep, Step};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long the overlay bounds assertion waits for the overlay to appear.
const OVERLAY_BOUNDS_TIMEOUT: Duration = Duration::from_secs(5);

/// YAML-driven e2e runner: `e2e [--podman] [--no-build] [--no-mock] [--record] [files...]`.
fn main() {
    install_sigint_cleanup();
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}

/// Ctrl+C skips `Drop`, so the spawned app (and its infer children) would leak,
/// bundle runs especially, whose app lives outside our process group. Reap them
/// explicitly, then exit. A `record:` run's screencapture gets its own SIGINT
/// via the process group and finalizes the .mov on its own.
fn install_sigint_cleanup() {
    let _ = ctrlc::set_handler(|| {
        let _ = Command::new("pkill")
            .args(["-f", home::PROCESS_MATCH])
            .status();
        std::process::exit(130);
    });
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
    let mut record_all = false;
    let mut in_podman = false;
    let mut forwarded: Vec<String> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for arg in std::env::args().skip(1) {
        if arg == "--podman" {
            in_podman = true;
            continue;
        }
        forwarded.push(arg.clone());
        match arg.as_str() {
            "--no-build" => build = false,
            "--no-mock" => mock = false,
            "--record" => record_all = true,
            _ if arg.starts_with("--") => bail!("unknown flag {arg}"),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if in_podman {
        return podman::run(&repo_root, &forwarded);
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

    let total = files.len();
    let suite_started = Instant::now();
    let mut failed = Vec::new();
    let mut skipped = 0;
    for (index, file) in files.iter().enumerate() {
        let test = spec::load(file)?;
        println!(
            "\n=== [{}/{total}] {} ({})",
            index + 1,
            test.name,
            file.display()
        );
        if let Some(platform) = test.platform.filter(|_| !test.runs_on_this_system()) {
            println!("SKIP {} ({} only)", test.name, platform.label());
            skipped += 1;
            continue;
        }
        let started = Instant::now();
        let outcome = run_test(
            &test,
            &repo_root,
            &artifacts,
            mock,
            &scenarios,
            infer_bin.as_deref(),
            record_all || test.record,
        );
        let took = elapsed(started.elapsed());
        match outcome {
            Ok(()) => println!("PASS {} ({took})", test.name),
            Err(e) => {
                println!("FAIL {} ({took}): {e:#}", test.name);
                failed.push(test.name.clone());
            }
        }
    }

    let skipped_note = if skipped > 0 {
        format!(", {skipped} skipped")
    } else {
        String::new()
    };
    println!(
        "\n{total} tests in {}: {} passed, {} failed{skipped_note}",
        elapsed(suite_started.elapsed()),
        total - failed.len() - skipped,
        failed.len()
    );
    for name in &failed {
        println!("  failed: {name}");
    }
    Ok(failed.is_empty())
}

/// A run's length as seconds under a minute, else minutes and seconds.
fn elapsed(took: Duration) -> String {
    let secs = took.as_secs();
    if secs < 60 {
        format!("{:.1}s", took.as_secs_f64())
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// Resolve the infer binary for the whole suite: an explicit INFER_BIN wins,
/// otherwise download the release `.cli-version` names once - the one a fresh
/// install of this desktop takes - falling back to the dev environment's PATH
/// when the download fails (offline).
fn resolve_infer(artifacts: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("INFER_BIN") {
        println!("using INFER_BIN override: {}", PathBuf::from(&p).display());
        return Some(PathBuf::from(p));
    }
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    let asset = format!("infer-{os}-{arch}");
    let tag = include_str!("../../.cli-version").trim();
    let url =
        format!("https://github.com/inference-gateway/cli/releases/download/{tag}/{asset}.tar.gz");
    let dir = artifacts.join(format!("infer-tested-{os}"));
    let tarball = dir.join(format!("{asset}.tar.gz"));
    let bin = dir.join("infer");
    let downloaded = std::fs::create_dir_all(&dir).is_ok()
        && Command::new("curl")
            .args(["-fsSL", "--retry", "2", "-o"])
            .arg(&tarball)
            .arg(&url)
            .status()
            .is_ok_and(|s| s.success())
        && Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&dir)
            .status()
            .is_ok_and(|s| s.success());
    if downloaded {
        let version = Command::new(&bin)
            .arg("--version")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        println!("using the tested infer release: {version}");
        return Some(bin);
    }
    println!(
        "warning: could not download the tested infer release - falling back to the dev environment's (possibly stale) binary"
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
    record: bool,
) -> Result<()> {
    let slug: String = test
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    let _recording = start_recording(record, artifacts, &slug)?;
    let app = AppDriver::launch(
        repo_root,
        artifacts,
        &slug,
        mock,
        scenarios,
        infer_bin,
        Seeds {
            content_project: test.content_project.as_deref(),
            workspace_agent: test.workspace_agent.as_deref(),
            model_agent: test.model_agent.as_deref(),
            project_agent: test.project_agent.as_deref(),
            voice_sample: test.voice_sample.as_deref(),
            speech_model: test.speech_model.as_deref(),
            avatar: test.avatar.as_deref(),
            hub_model: test.hub_model.as_deref(),
        },
    )?;
    clean(&app, &test.cleanup);

    let result = run_steps(test, &app);
    if result.is_err() {
        match app.screenshot(&format!("{slug}-fail")) {
            Ok(p) => println!("  screenshot: {}", p.display()),
            Err(e) => println!("  (no failure screenshot: {e})"),
        }
        #[cfg(not(target_os = "linux"))]
        match app.dump_tree(&format!("{slug}-ax")) {
            Ok(p) => println!("  accessibility tree: {}", p.display()),
            Err(e) => println!("  (no accessibility tree: {e})"),
        }
    }
    clean(&app, &test.cleanup);
    result
}

/// A `record: true` run is wrapped in `screencapture -v`, which finalizes
/// artifacts/<slug>.mov only on SIGINT - SIGKILL (`Child::kill`) leaves no
/// file. Drop sends it, covering the failure paths.
struct Recording(Option<Child>);

impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = Command::new("kill")
                .args(["-INT", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }
}

fn start_recording(record: bool, artifacts: &Path, slug: &str) -> Result<Recording> {
    if !record {
        return Ok(Recording(None));
    }
    if cfg!(target_os = "linux") {
        println!("  not recording: screen recording needs the macOS run");
        return Ok(Recording(None));
    }
    let path = artifacts.join(format!("{slug}.mov"));
    let _ = std::fs::remove_file(&path);
    println!("  recording -> {}", path.display());
    Ok(Recording(Some(
        Command::new("screencapture")
            .arg("-v")
            .arg(path)
            .stdin(Stdio::null())
            .spawn()
            .context("starting screencapture -v")?,
    )))
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
        let note = test
            .narration
            .get(i)
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(label.as_str());
        println!("  ok  {}/{} {note}", i + 1, test.steps.len());
    }
    Ok(())
}

fn step_run(app: &AppDriver, step: &Step) -> Result<()> {
    match step {
        Step::Bare(BareStep::NewChat) => app.new_chat(),
        Step::Bare(BareStep::AssertOverlayBounds) => {
            let found = app.poll(OVERLAY_BOUNDS_TIMEOUT, || {
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
        Step::Drag { drag } => app.drag(&drag.from, drag.from_dx, &drag.to),
        Step::AssertOrder { assert_order } => {
            if !app.is_above(&assert_order.above, &assert_order.below)? {
                bail!(
                    "{:?} is not above {:?}",
                    assert_order.above,
                    assert_order.below
                );
            }
            Ok(())
        }
        Step::Screenshot { screenshot } => app.screenshot(screenshot).map(|_| ()),
        Step::AssertModel { assert_model } => {
            let got = app.model_value()?;
            if got != *assert_model {
                bail!("model is {got:?}, expected {assert_model:?}");
            }
            Ok(())
        }
        Step::AssertComposer { assert_composer } => {
            let mut got = String::new();
            let matched = app.poll(Duration::from_secs(assert_composer.timeout), || {
                got = app.composer_value()?;
                Ok(got.trim() == assert_composer.text.trim())
            })?;
            if !matched {
                bail!("composer is {got:?}, expected {:?}", assert_composer.text);
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
        Step::WaitGone { wait_gone } => {
            let never_shown = || -> Result<()> {
                match &wait_gone.never {
                    Some(never) if app.button_exists(never)? => {
                        bail!("button {never:?} showed up")
                    }
                    _ => Ok(()),
                }
            };
            let gone = app.poll(Duration::from_secs(wait_gone.timeout), || {
                never_shown()?;
                Ok(!app.button_exists(&wait_gone.button)?)
            })?;
            if !gone {
                bail!("still there after {}s", wait_gone.timeout);
            }
            never_shown()
        }
        Step::WaitFor { wait_for } => {
            let timeout = Duration::from_secs(wait_for.timeout);
            let found = if let Some(button) = &wait_for.button {
                app.poll(timeout, || app.button_exists(button))?
            } else if let Some(text) = &wait_for.text {
                app.poll(timeout, || app.text_exists(text))?
            } else if let Some(file) = &wait_for.file {
                let path = app.resolve(file);
                app.poll(timeout, || {
                    Ok(match &wait_for.contains {
                        None => path.exists(),
                        Some(needle) => std::fs::read_to_string(&path)
                            .is_ok_and(|text| text.contains(needle.as_str())),
                    })
                })?
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
        Step::Drag { drag } => format!("drag {:?} onto {:?}", drag.from, drag.to),
        Step::AssertOrder { assert_order } => {
            format!(
                "assert_order {:?} above {:?}",
                assert_order.above, assert_order.below
            )
        }
        Step::Screenshot { screenshot } => format!("screenshot {screenshot}"),
        Step::AssertModel { assert_model } => format!("assert_model {assert_model}"),
        Step::AssertComposer { .. } => "assert_composer".into(),
        Step::WaitGone { wait_gone } => format!("wait_gone button {:?}", wait_gone.button),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_reads_in_seconds_under_a_minute_and_in_minutes_after() {
        assert_eq!(elapsed(Duration::from_millis(5_240)), "5.2s");
        assert_eq!(elapsed(Duration::from_secs(2_892)), "48m12s");
        assert_eq!(elapsed(Duration::from_secs(60)), "1m00s");
    }
}
