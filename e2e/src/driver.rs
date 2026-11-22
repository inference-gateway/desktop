use crate::ax_helper::AxHelper;
use crate::home::{self, PROCESS_MATCH, Seeds};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const AFTER_PKILL_SETTLE: Duration = Duration::from_millis(500);
const INPUT_FOCUS_SETTLE: Duration = Duration::from_millis(500);
const KEYPRESS_FOCUS_SETTLE: Duration = Duration::from_millis(200);
/// How long a step waits for the element it acts on: a lazily loaded view
/// shows its buttons, and a fresh window its composer, a beat after it opens.
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_PAUSE: Duration = Duration::from_millis(300);

const MAIN_WINDOW_TITLE: &str = "Inference Gateway Desktop";
/// The accessible name of the model picker in the bar above the composer.
const MODEL_PICKER: &str = "Model";
const OVERLAY_WINDOW_TITLE: &str = "Computer Use Overlay";
/// What the helper answers when the terminal running the suite lacks
/// Accessibility permission.
const NOT_TRUSTED: &str = "assistive access is not granted";

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The launched app and the paths its run reads, driven through the macOS
/// accessibility tree by the accessibility helper (the recipe and its quirks
/// are documented in AGENTS.md, "Verifying the UI"). `mock_home` is the wiped
/// HOME of a mock run, whose infer daemon the driver stops.
pub struct AppDriver {
    child: Child,
    ax: AxHelper,
    repo_root: PathBuf,
    mock_home: Option<PathBuf>,
    artifacts: PathBuf,
}

/// macOS keeps the webview's localStorage under `~/Library/WebKit/<app>`, keyed
/// by the executable or bundle id, NOT under the child's overridden HOME - so
/// it survives the per-test HOME wipe and leaks state across runs and into the
/// dev's real app. Wipe it too, using the runner's OWN home. Bundle runs
/// (`DESKTOP_APP`) use the bundle-id store instead.
fn wipe_webkit_store(bundle: bool) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let webkit = PathBuf::from(home).join("Library").join("WebKit");
    let app = if bundle {
        "com.inference-gateway.desktop"
    } else {
        "inference-gateway-desktop"
    };
    let _ = std::fs::remove_dir_all(webkit.join(app));
}

impl AppDriver {
    /// Kill stale instances, launch the prebuilt binary fresh (mock mode by
    /// default), and wait until the AX tree answers.
    pub fn launch(
        repo_root: &Path,
        artifacts: &Path,
        log_name: &str,
        mock: bool,
        scenarios: &Path,
        infer_bin: Option<&Path>,
        seeds: Seeds,
    ) -> Result<Self> {
        let _ = Command::new("pkill").args(["-f", PROCESS_MATCH]).status();
        std::thread::sleep(AFTER_PKILL_SETTLE);
        let home = artifacts.join("home");
        if mock {
            home::stop_daemon(&home);
        }

        let backend = repo_root.join("backend");
        let app_bundle = std::env::var_os("DESKTOP_APP").map(PathBuf::from);
        let bin = std::env::var_os("DESKTOP_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join("target/debug/inference-gateway-desktop"));
        let executable = app_bundle.as_deref().unwrap_or(&bin);
        if !executable.exists() {
            bail!(
                "app executable missing at {} - run without --no-build",
                executable.display()
            );
        }

        std::fs::create_dir_all(artifacts)?;
        let log_path = artifacts.join(format!("{log_name}.log"));
        let resolved_infer = infer_bin.map(Path::to_path_buf).or_else(home::which_infer);
        if mock {
            wipe_webkit_store(app_bundle.is_some());
            home::prepare(&home, repo_root, seeds, resolved_infer.as_deref())?;
        }

        let child = if let Some(app_bundle) = app_bundle {
            let _ = std::fs::File::create(&log_path)?;
            let mut cmd = Command::new("open");
            cmd.args(["-n", "-W", "--stdout"])
                .arg(&log_path)
                .arg("--stderr")
                .arg(&log_path);
            if mock {
                cmd.arg("--env")
                    .arg("DESKTOP_MOCK=true")
                    .arg("--env")
                    .arg("INFER_COMPUTER_USE_ENABLED=true")
                    .arg("--env")
                    .arg("INFER_CHAT_SUGGESTIONS_ENABLED=true")
                    .arg("--env")
                    .arg(format!(
                        "INFER_GATEWAY_MOCK_SCENARIOS={}",
                        scenarios.display()
                    ))
                    .arg("--env")
                    .arg(format!("HOME={}", home.display()));
            }
            if let Some(infer) = resolved_infer {
                cmd.arg("--env")
                    .arg(format!("INFER_BIN={}", infer.display()));
            }
            cmd.arg(app_bundle)
                .spawn()
                .context("launching app bundle")?
        } else {
            let log = std::fs::File::create(&log_path)?;
            let mut cmd = Command::new(&bin);
            cmd.current_dir(&backend)
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log));
            if mock {
                cmd.env("DESKTOP_MOCK", "true")
                    .env("INFER_COMPUTER_USE_ENABLED", "true")
                    .env("INFER_CHAT_SUGGESTIONS_ENABLED", "true")
                    .env("INFER_COMPUTER_USE_RECORDING_ENABLED", "true")
                    .env("INFER_GATEWAY_MOCK_SCENARIOS", scenarios)
                    .env("HOME", &home);
            }
            if let Some(infer) = resolved_infer {
                cmd.env("INFER_BIN", infer);
            }
            cmd.spawn().context("spawning app binary")?
        };

        let ax = AxHelper::start(
            artifacts,
            PROCESS_MATCH,
            MAIN_WINDOW_TITLE,
            OVERLAY_WINDOW_TITLE,
        )?;
        let driver = Self {
            child,
            ax,
            repo_root: repo_root.to_path_buf(),
            artifacts: artifacts.to_path_buf(),
            mock_home: mock.then_some(home),
        };
        driver.wait_ready()?;
        Ok(driver)
    }

    fn wait_ready(&self) -> Result<()> {
        let deadline = Instant::now() + LAUNCH_TIMEOUT;
        loop {
            match self.ax.ask("ready", json!({})) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if e.to_string().contains(NOT_TRUSTED) {
                        bail!(
                            "the terminal lacks Accessibility permission - grant it in \
                             System Settings > Privacy & Security > Accessibility"
                        );
                    }
                    if Instant::now() > deadline {
                        return Err(e.context("app window never became reachable over AX"));
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// AX-press "New chat". Also serves as the mandatory webview warm-up:
    /// AXValue sets silently no-op until one AX click has landed inside the
    /// page (see AGENTS.md).
    pub fn new_chat(&self) -> Result<()> {
        self.click("New chat")
    }

    /// Set the textarea value, verify it stuck (read-back), then click Send.
    /// While the session is running the Send button is swapped for Stop, so
    /// fall back to focusing the composer and committing with Return (the
    /// queued-prompt path).
    pub fn send(&self, text: &str) -> Result<()> {
        let mut got = once_rendered(RENDER_TIMEOUT, POLL_INTERVAL, || {
            self.set_composer(text, false)
        })?;
        if got.trim() != text {
            got = self.set_composer(text, true)?;
        }
        if got.trim() != text {
            bail!(
                "AXValue did not propagate (got {:?}) - the webview was not warmed up; \
                 a test must start with new_chat",
                got.trim()
            );
        }
        self.press("Send").or_else(|_| {
            self.ax.ask("composer_press", json!({}))?;
            osascript("tell application \"System Events\"\nkey code 36\nend tell")
                .map(|_| ())
                .map_err(|e| anyhow!("clicking Send and committing with Return both failed: {e}"))
        })
    }

    /// Writes `text` into the composer and returns what it reads back, after
    /// pressing the composer first when `press` is set.
    fn set_composer(&self, text: &str, press: bool) -> Result<String> {
        let value = self
            .ax
            .ask("composer_set", json!({ "text": text, "press": press }))?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// Sidebar prompt inputs (e.g. "New project") auto-focus when revealed but
    /// aren't reliably reachable by AX path across macOS versions (unlike the
    /// composer text area). Type into the focused element via keystroke, then
    /// commit with Return. The caller verifies the result with a `wait_for`.
    pub fn type_text(&self, text: &str) -> Result<()> {
        std::thread::sleep(INPUT_FOCUS_SETTLE);
        let script = format!(
            "tell application \"System Events\"\nkeystroke \"{msg}\"\nend tell",
            msg = escape(text),
        );
        osascript(&script).map_err(|e| anyhow!("typing {text:?}: {}", e))?;
        osascript("tell application \"System Events\"\nkey code 36\nend tell")
            .map(|_| ())
            .map_err(|e| anyhow!("committing {text:?}: {}", e))
    }

    /// Presses the named button, waiting up to `RENDER_TIMEOUT` for it to show.
    pub fn click(&self, button: &str) -> Result<()> {
        once_rendered(RENDER_TIMEOUT, POLL_INTERVAL, || self.press(button))
    }

    /// Presses the named button once, failing when it is not on screen.
    fn press(&self, button: &str) -> Result<()> {
        self.ax
            .ask("press", json!({ "name": button }))
            .map(|_| ())
            .map_err(|e| anyhow!("clicking {:?}: {}", button, e))
    }

    /// Drags the element named `from` onto the element named `to`, centre to
    /// centre, with the press moved `from_dx` points sideways.
    pub fn drag(&self, from: &str, from_dx: f64, to: &str) -> Result<()> {
        let (fx, fy) = self.centre_of(from)?;
        let (tx, ty) = self.centre_of(to)?;
        self.ax
            .ask(
                "drag",
                json!({ "from": [fx + from_dx, fy], "to": [tx, ty] }),
            )
            .map(|_| ())
            .map_err(|e| anyhow!("dragging {from:?} onto {to:?}: {e}"))
    }

    /// Whether the element named `above` sits higher on screen than `below`.
    pub fn is_above(&self, above: &str, below: &str) -> Result<bool> {
        Ok(self.centre_of(above)?.1 < self.centre_of(below)?.1)
    }

    fn centre_of(&self, name: &str) -> Result<(f64, f64)> {
        let centre = self
            .ax
            .ask("centre", json!({ "name": name }))
            .map_err(|e| anyhow!("finding {name:?}: {e}"))?;
        two_numbers(&centre).with_context(|| format!("the centre of {name:?}"))
    }

    /// Layout guard: the named button's bottom edge must sit above the
    /// composer textarea's top edge. Catches transcript content overflowing
    /// the conversation area across the composer (regression of PR #127).
    pub fn button_above_composer(&self, button: &str) -> Result<bool> {
        let edges = self.ax.ask("above_composer", json!({ "name": button }))?;
        let (bottom, top) =
            two_numbers(&edges).context("the button's bottom and the composer's top")?;
        Ok(bottom <= top)
    }

    pub fn button_exists(&self, button: &str) -> Result<bool> {
        let found = self.ax.ask("exists_button", json!({ "name": button }))?;
        Ok(found.as_bool().unwrap_or(false))
    }

    pub fn text_exists(&self, needle: &str) -> Result<bool> {
        let found = self.ax.ask("exists_text", json!({ "name": needle }))?;
        Ok(found.as_bool().unwrap_or(false))
    }

    pub fn model_value(&self) -> Result<String> {
        let value = self.ax.ask("value", json!({ "name": MODEL_PICKER }))?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// The composer textarea's current text - used by assert_composer, e.g. to
    /// prove that ArrowUp recalled the previous prompt.
    pub fn composer_value(&self) -> Result<String> {
        let value = self.ax.ask("composer_get", json!({}))?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// The overlay starts below the primary screen's menu bar, spans its full
    /// width, and reaches the physical bottom edge behind the Dock.
    pub fn overlay_matches_primary_screen(&self) -> Result<bool> {
        let bounds = self.ax.ask("overlay_bounds", json!({}))?;
        let values: Vec<f64> = bounds
            .as_array()
            .map(|all| all.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if values.len() != 8 {
            return Ok(false);
        }
        Ok(values[..4]
            .iter()
            .zip(&values[4..])
            .all(|(actual, expected)| (actual - expected).abs() <= 1.0))
    }

    /// Resolve a cleanup/assert path against the repo root (the infer child's
    /// cwd - the app hops out of backend/, see `resolve_agent_cwd`).
    pub fn resolve(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.repo_root.join(p)
        }
    }

    /// Wait until `cond` returns true or `timeout` elapses.
    pub fn poll(&self, timeout: Duration, mut cond: impl FnMut() -> Result<bool>) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if cond()? {
                return Ok(true);
            }
            if Instant::now() > deadline {
                return Ok(false);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Send a single keystroke to the composer - a named key by AX key code,
    /// or any single ASCII character. Used by e2e tests that need to exercise
    /// keyboard shortcuts (ArrowUp/Down, Tab, Escape, letter keys).
    pub fn keypress(&self, key: &str) -> Result<()> {
        let stroke = match key {
            "up" => "keystroke (key code 126)".to_string(),
            "down" => "keystroke (key code 125)".to_string(),
            "tab" => "keystroke (key code 48)".to_string(),
            "escape" => "keystroke (key code 53)".to_string(),
            k if k.chars().count() == 1 && k.is_ascii() => format!("keystroke \"{}\"", escape(k)),
            _ => bail!("unsupported keypress key: {key:?}"),
        };
        once_rendered(RENDER_TIMEOUT, POLL_INTERVAL, || {
            self.ax.ask("composer_press", json!({}))
        })
        .map_err(|e| anyhow!("focusing the composer for {key:?}: {e}"))?;
        std::thread::sleep(KEYPRESS_FOCUS_SETTLE);
        osascript(&format!(
            "tell application \"System Events\"\n{stroke}\nend tell"
        ))
        .map(|_| ())
        .map_err(|e| anyhow!("keypress {:?}: {}", key, e))
    }

    /// Capture the app's main window (by window id - a screen-rect capture
    /// grabs whatever Space is visible) into artifacts/<name>.png.
    pub fn screenshot(&self, name: &str) -> Result<PathBuf> {
        let id = self
            .ax
            .ask("window_id", json!({}))?
            .as_u64()
            .ok_or_else(|| anyhow!("app window not found for screenshot"))?;
        let path = self.artifacts.join(format!("{name}.png"));
        let status = Command::new("screencapture")
            .args(["-x", "-l", &id.to_string()])
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("screencapture failed");
        }
        Ok(path)
    }
}

impl Drop for AppDriver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("pkill").args(["-f", PROCESS_MATCH]).status();
        if let Some(home) = &self.mock_home {
            home::stop_daemon(home);
        }
    }
}

/// Runs `action` until it succeeds, pausing between tries, for a step that
/// can run before React has rendered the element it acts on. Past `timeout`
/// the last failure stands.
fn once_rendered<T>(
    timeout: Duration,
    pause: Duration,
    mut action: impl FnMut() -> Result<T>,
) -> Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        match action() {
            Ok(value) => return Ok(value),
            Err(e) if Instant::now() > deadline => return Err(e),
            Err(_) => std::thread::sleep(pause),
        }
    }
}

/// The two numbers of a helper reply such as a centre or a pair of edges.
fn two_numbers(value: &Value) -> Result<(f64, f64)> {
    match value.as_array().map(Vec::as_slice) {
        Some([a, b]) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => bail!("expected two numbers, got {value}"),
        },
        _ => bail!("expected two numbers, got {value}"),
    }
}

/// Run an AppleScript, retrying once on transient AX flakes (-1700 / nonzero).
fn osascript(script: &str) -> Result<String> {
    match osascript_once(script) {
        Ok(out) => Ok(out),
        Err(first) => {
            if first.to_string().contains("-25211") {
                return Err(first);
            }
            std::thread::sleep(RETRY_PAUSE);
            osascript_once(script).map_err(|_| first)
        }
    }
}

fn osascript_once(script: &str) -> Result<String> {
    let out = Command::new("osascript")
        .args(["-e", script])
        .output()
        .context("running osascript")?;
    if !out.status.success() {
        bail!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_retries_until_its_element_renders_and_keeps_the_last_failure_after() {
        let mut tries = 0;
        let rendered = once_rendered(Duration::from_secs(5), Duration::ZERO, || {
            tries += 1;
            if tries < 3 {
                bail!("not rendered yet")
            }
            Ok(tries)
        });
        assert_eq!(rendered.unwrap(), 3);

        let mut attempt = 0;
        let never = once_rendered(Duration::ZERO, Duration::ZERO, || -> Result<()> {
            attempt += 1;
            bail!("missing after try {attempt}")
        });
        assert!(
            never
                .unwrap_err()
                .to_string()
                .starts_with("missing after try")
        );
    }

    #[test]
    fn a_reply_reads_as_two_numbers_only_when_it_holds_exactly_two() {
        assert_eq!(two_numbers(&json!([1.5, 2])).unwrap(), (1.5, 2.0));
        assert!(two_numbers(&json!([1.5])).is_err());
        assert!(two_numbers(&json!(["a", 2])).is_err());
        assert!(two_numbers(&json!(null)).is_err());
    }
}
