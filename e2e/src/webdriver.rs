use crate::home::{self, PROCESS_MATCH, Seeds};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const DRIVER_START_TIMEOUT: Duration = Duration::from_secs(10);
const AFTER_PKILL_SETTLE: Duration = Duration::from_millis(500);
const INPUT_FOCUS_SETTLE: Duration = Duration::from_millis(500);
const CLICK_TIMEOUT: Duration = Duration::from_secs(5);
const DRIVER_URL: &str = "http://127.0.0.1:4444";
const NATIVE_DRIVER: &str = "WebKitWebDriver";
const DRAG_STEPS: u32 = 20;
const DRAG_STEP_MS: u32 = 30;

const KEY_ENTER: &str = "\u{E007}";
const KEY_TAB: &str = "\u{E004}";
const KEY_ESCAPE: &str = "\u{E00C}";
const KEY_UP: &str = "\u{E013}";
const KEY_DOWN: &str = "\u{E015}";

/// The page half of the driver, run through WebDriver's execute. It finds
/// elements the way the macOS run reads WebKit's accessibility tree: a name is
/// the `aria-label`, else the rendered text without its `aria-hidden` parts,
/// and adjacent text runs read as one, so a pool row reads `media/logo.png 36 KB`.
const PAGE_JS: &str = r##"
const [op, arg] = arguments;
const BUTTONS = 'button, [role="button"], input[type="button"], input[type="submit"], input[type="checkbox"], input[type="radio"], [role="checkbox"], [role="switch"], [role="radio"], [role="tab"], [role="menuitem"], select, summary';
const norm = (s) => (s ?? "").replace(/\s+/g, " ").trim();
const shown = (el) => el.getClientRects().length > 0 && !el.closest('[aria-hidden="true"]');
const ownText = (el) => [...el.childNodes].filter((c) => c.nodeType === Node.TEXT_NODE).map((c) => c.nodeValue).join("");
const textOf = (node) => [...node.childNodes].map((c) => {
  if (c.nodeType === Node.TEXT_NODE) return c.nodeValue;
  if (c.nodeType !== Node.ELEMENT_NODE || c.getAttribute("aria-hidden") === "true" || !c.getClientRects().length) return "";
  return " " + (c.getAttribute("aria-label") ?? (c.tagName === "IMG" ? c.getAttribute("alt") ?? "" : textOf(c))) + " ";
}).join("");
const nameOf = (el) => {
  const ids = el.getAttribute("aria-labelledby");
  if (ids) return norm(ids.split(/\s+/).map((id) => document.getElementById(id)?.innerText ?? "").join(" "));
  const label = el.getAttribute("aria-label");
  if (label) return norm(label);
  if (el.labels?.length) return norm([...el.labels].map((l) => l.innerText).join(" "));
  return norm(textOf(el)) || norm(el.getAttribute("title") ?? el.getAttribute("alt"));
};
const findButton = (name) => [...document.querySelectorAll(BUTTONS)].find((el) => shown(el) && nameOf(el) === name);
const findNamed = (name) => {
  for (const el of document.querySelectorAll("body *")) {
    if (!shown(el)) continue;
    if (norm(el.getAttribute("aria-label")) === name) return el;
    if (el.matches(BUTTONS) && nameOf(el).startsWith(name)) return el;
    const text = norm(ownText(el));
    if (text && text.startsWith(name)) return el;
  }
  return null;
};
const textExists = (needle) => [...document.querySelectorAll("body *")].some(
  (el) => shown(el) && (norm(ownText(el)).includes(needle) || (el.matches(BUTTONS) && nameOf(el).includes(needle))),
);
const centre = (el) => {
  el.scrollIntoView({ block: "nearest", inline: "nearest" });
  const r = el.getBoundingClientRect();
  return [r.left + r.width / 2, r.top + r.height / 2];
};
const textArea = () => [...document.querySelectorAll("textarea")].find(shown);
switch (op) {
  case "ready": return !!document.querySelector("#app button") && !!document.getElementById("model-select")?.value;
  case "click": {
    const el = findButton(arg);
    if (!el) return false;
    el.click();
    return true;
  }
  case "button_exists": return !!findButton(arg);
  case "text_exists": return textExists(arg);
  case "centre": {
    const el = findNamed(arg);
    return el ? centre(el) : null;
  }
  case "button_above_composer": {
    const el = findButton(arg);
    const ta = textArea();
    return el && ta ? [el.getBoundingClientRect().bottom, ta.getBoundingClientRect().top] : null;
  }
  case "set_composer": {
    const ta = textArea();
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(ta, arg);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
    return ta.value;
  }
  case "focus_text_area": textArea().focus(); return true;
  case "composer_value": return textArea().value;
  case "model_value": return document.getElementById("model-select").selectedOptions[0]?.text ?? "";
}
throw new Error("unknown page op " + op);
"##;

/// The launched app, driven over WebDriver through `tauri-driver` and
/// WebKitWebDriver - the Linux run inside the e2e container. Its methods
/// mirror the macOS driver's, so the YAML specs run unchanged. `mock_home` is
/// the wiped HOME of a mock run, whose infer daemon the driver stops.
pub struct AppDriver {
    tauri_driver: Child,
    session: String,
    repo_root: PathBuf,
    mock_home: Option<PathBuf>,
    artifacts: PathBuf,
}

impl AppDriver {
    /// Kill stale instances, start `tauri-driver` with the run's environment
    /// (WebKitWebDriver hands it to the app it launches), open a session on the
    /// prebuilt binary and wait until the page has rendered.
    pub fn launch(
        repo_root: &Path,
        artifacts: &Path,
        log_name: &str,
        mock: bool,
        scenarios: &Path,
        infer_bin: Option<&Path>,
        seeds: Seeds,
    ) -> Result<Self> {
        stop_app();
        let home = artifacts.join("home");
        if mock {
            home::stop_daemon(&home);
        }
        let bin = std::env::var_os("DESKTOP_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.join("target/debug/inference-gateway-desktop"));
        if !bin.exists() {
            bail!(
                "app executable missing at {} - run without --no-build",
                bin.display()
            );
        }

        std::fs::create_dir_all(artifacts)?;
        let resolved_infer = infer_bin.map(Path::to_path_buf).or_else(home::which_infer);
        if mock {
            home::prepare(&home, repo_root, seeds, resolved_infer.as_deref())?;
        }

        let log = std::fs::File::create(artifacts.join(format!("{log_name}.log")))?;
        let mut cmd = Command::new("tauri-driver");
        cmd.current_dir(repo_root.join("backend"))
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
        let tauri_driver = cmd.spawn().context("spawning tauri-driver")?;
        wait_for_driver()?;
        let session = call(
            "POST",
            "/session",
            Some(
                &json!({"capabilities": {"alwaysMatch": {"tauri:options": {"application": bin}}}}),
            ),
        )
        .context("opening a WebDriver session on the app")?["sessionId"]
            .as_str()
            .context("WebDriver returned no session id")?
            .to_string();

        let driver = Self {
            tauri_driver,
            session,
            repo_root: repo_root.to_path_buf(),
            artifacts: artifacts.to_path_buf(),
            mock_home: mock.then_some(home),
        };
        driver.wait_ready()?;
        Ok(driver)
    }

    fn wait_ready(&self) -> Result<()> {
        if self.poll(LAUNCH_TIMEOUT, || {
            Ok(self.page("ready", "")?.as_bool() == Some(true))
        })? {
            return Ok(());
        }
        bail!("the app's page never rendered with a model selected")
    }

    fn page(&self, op: &str, arg: &str) -> Result<Value> {
        call(
            "POST",
            &format!("/session/{}/execute/sync", self.session),
            Some(&json!({"script": PAGE_JS, "args": [op, arg]})),
        )
    }

    fn actions(&self, actions: Value) -> Result<()> {
        let path = format!("/session/{}/actions", self.session);
        call("POST", &path, Some(&json!({ "actions": [actions] })))?;
        call("DELETE", &path, None).map(|_| ())
    }

    fn keys(&self, keys: &[&str]) -> Result<()> {
        let strokes: Vec<Value> = keys
            .iter()
            .flat_map(|k| {
                [
                    json!({"type": "keyDown", "value": k}),
                    json!({"type": "keyUp", "value": k}),
                ]
            })
            .collect();
        self.actions(json!({"type": "key", "id": "keyboard", "actions": strokes}))
    }

    /// Click "New chat", the step every test starts with.
    pub fn new_chat(&self) -> Result<()> {
        self.click("New chat")
    }

    /// Set the composer's text, verify it stuck, then click Send. While the
    /// session is running Send is swapped for Stop, so fall back to committing
    /// with Return (the queued-prompt path).
    pub fn send(&self, text: &str) -> Result<()> {
        let got = self.page("set_composer", text)?;
        if got.as_str().map(str::trim) != Some(text) {
            bail!("the composer did not take the prompt (got {got})");
        }
        self.click("Send").or_else(|_| {
            self.page("focus_text_area", "")?;
            self.keys(&[KEY_ENTER])
                .map_err(|e| anyhow!("clicking Send and committing with Return both failed: {e}"))
        })
    }

    /// Type into the focused element, then commit with Return. The caller
    /// verifies the result with a `wait_for`.
    pub fn type_text(&self, text: &str) -> Result<()> {
        std::thread::sleep(INPUT_FOCUS_SETTLE);
        let chars: Vec<String> = text.chars().map(String::from).collect();
        let mut keys: Vec<&str> = chars.iter().map(String::as_str).collect();
        keys.push(KEY_ENTER);
        self.keys(&keys)
            .map_err(|e| anyhow!("typing {text:?}: {e}"))
    }

    /// Click the named button, waiting a moment for it to render - a lazily
    /// loaded view shows its buttons a beat after the click that opened it.
    pub fn click(&self, button: &str) -> Result<()> {
        if self.poll(CLICK_TIMEOUT, || {
            Ok(self.page("click", button)?.as_bool() == Some(true))
        })? {
            return Ok(());
        }
        bail!("clicking {button:?}: button not found")
    }

    /// Drags the element named `from` onto the element named `to`, centre to
    /// centre, with the press moved `from_dx` pixels sideways.
    pub fn drag(&self, from: &str, from_dx: f64, to: &str) -> Result<()> {
        let (fx, fy) = self.centre_of(from)?;
        let fx = fx + from_dx;
        let (tx, ty) = self.centre_of(to)?;
        let point = |x: f64, y: f64, duration: u32| json!({"type": "pointerMove", "duration": duration, "origin": "viewport", "x": x.round() as i64, "y": y.round() as i64});
        let mut steps = vec![
            point(fx, fy, 0),
            json!({"type": "pointerDown", "button": 0}),
        ];
        for i in 1..=DRAG_STEPS {
            let t = f64::from(i) / f64::from(DRAG_STEPS);
            steps.push(point(fx + (tx - fx) * t, fy + (ty - fy) * t, DRAG_STEP_MS));
        }
        steps.push(json!({"type": "pointerUp", "button": 0}));
        self.actions(json!({"type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"}, "actions": steps}))
            .map_err(|e| anyhow!("dragging {from:?} onto {to:?}: {e}"))
    }

    /// Whether the element named `above` sits higher on the page than `below`.
    pub fn is_above(&self, above: &str, below: &str) -> Result<bool> {
        Ok(self.centre_of(above)?.1 < self.centre_of(below)?.1)
    }

    fn centre_of(&self, name: &str) -> Result<(f64, f64)> {
        let value = self.page("centre", name)?;
        match (value[0].as_f64(), value[1].as_f64()) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => bail!("finding {name:?}: element not found"),
        }
    }

    /// Layout guard: the named button's bottom edge must sit above the
    /// composer textarea's top edge.
    pub fn button_above_composer(&self, button: &str) -> Result<bool> {
        let value = self.page("button_above_composer", button)?;
        match (value[0].as_f64(), value[1].as_f64()) {
            (Some(bottom), Some(top)) => Ok(bottom <= top),
            _ => bail!("button {button:?} or the composer not found"),
        }
    }

    pub fn button_exists(&self, button: &str) -> Result<bool> {
        Ok(self.page("button_exists", button)?.as_bool() == Some(true))
    }

    pub fn text_exists(&self, needle: &str) -> Result<bool> {
        Ok(self.page("text_exists", needle)?.as_bool() == Some(true))
    }

    pub fn model_value(&self) -> Result<String> {
        Ok(self
            .page("model_value", "")?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// The composer textarea's current text.
    pub fn composer_value(&self) -> Result<String> {
        Ok(self
            .page("composer_value", "")?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// The overlay check measures the macOS menu bar and Dock, which the
    /// container's display has neither of.
    pub fn overlay_matches_primary_screen(&self) -> Result<bool> {
        bail!("the overlay bounds check needs the macOS run")
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

    /// Focus the composer and send one key - a named key or any single ASCII
    /// character - as the macOS run does after clicking the text area.
    pub fn keypress(&self, key: &str) -> Result<()> {
        let stroke = match key {
            "up" => KEY_UP,
            "down" => KEY_DOWN,
            "tab" => KEY_TAB,
            "escape" => KEY_ESCAPE,
            k if k.chars().count() == 1 && k.is_ascii() => k,
            _ => bail!("unsupported keypress key: {key:?}"),
        };
        self.page("focus_text_area", "")?;
        self.keys(&[stroke])
            .map_err(|e| anyhow!("keypress {key:?}: {e}"))
    }

    /// Capture the app's page into artifacts/<name>.png.
    pub fn screenshot(&self, name: &str) -> Result<PathBuf> {
        let encoded = call(
            "GET",
            &format!("/session/{}/screenshot", self.session),
            None,
        )?;
        let encoded = encoded
            .as_str()
            .context("WebDriver returned no screenshot")?;
        let path = self.artifacts.join(format!("{name}.png"));
        std::fs::write(&path, decode_base64(encoded)?)?;
        Ok(path)
    }
}

impl Drop for AppDriver {
    fn drop(&mut self) {
        let _ = call("DELETE", &format!("/session/{}", self.session), None);
        let _ = self.tauri_driver.kill();
        let _ = self.tauri_driver.wait();
        stop_app();
        if let Some(home) = &self.mock_home {
            home::stop_daemon(home);
        }
    }
}

fn stop_app() {
    for process in [PROCESS_MATCH, NATIVE_DRIVER] {
        let _ = Command::new("pkill").args(["-f", process]).status();
    }
    std::thread::sleep(AFTER_PKILL_SETTLE);
}

fn wait_for_driver() -> Result<()> {
    let deadline = Instant::now() + DRIVER_START_TIMEOUT;
    while Instant::now() < deadline {
        if call("GET", "/status", None).is_ok() {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    bail!("tauri-driver did not start listening on {DRIVER_URL}")
}

/// One WebDriver request over curl, returning the response's `value`.
fn call(method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let mut child = Command::new("curl")
        .args(["-s", "-X", method, "-H", "content-type: application/json"])
        .args(body.map(|_| ["--data-binary", "@-"]).unwrap_or_default())
        .arg(format!("{DRIVER_URL}{path}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawning curl")?;
    if let Some(body) = body {
        child
            .stdin
            .take()
            .context("curl stdin")?
            .write_all(body.to_string().as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{method} {path}: curl exited {}", out.status);
    }
    let response: Value = serde_json::from_slice(&out.stdout).with_context(|| {
        format!(
            "{method} {path}: not JSON: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })?;
    let value = response["value"].clone();
    if let Some(error) = value.get("error") {
        bail!("{method} {path}: {error}: {}", value["message"]);
    }
    Ok(value)
}

fn decode_base64(encoded: &str) -> Result<Vec<u8>> {
    let mut child = Command::new("base64")
        .arg("-d")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("spawning base64")?;
    child
        .stdin
        .take()
        .context("base64 stdin")?
        .write_all(encoded.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("decoding the screenshot failed");
    }
    Ok(out.stdout)
}
