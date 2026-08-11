//! The only platform-aware module: launches the app binary and drives it
//! through the macOS accessibility tree by shelling out to osascript,
//! screencapture, and swift. The AX recipe and its quirks are documented in
//! AGENTS.md ("Verifying the UI").

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);

const PROCESS_MATCH: &str = "inference-gateway-desktop";

/// Recursive AXButton finder; `entire contents` is flaky (-1700) so every
/// button lookup walks `UI elements` instead.
const FIND_BUTTON_FN: &str = r#"
on findButton(el, btnName, depth)
	tell application "System Events"
		if depth > 8 then return missing value
		try
			if role of el is "AXButton" and name of el is btnName then return el
		end try
		try
			repeat with c in (UI elements of el)
				set r to my findButton(c, btnName, depth + 1)
				if r is not missing value then return r
			end repeat
		end try
		return missing value
	end tell
end findButton
"#;

const FIND_TEXT_FN: &str = r#"
on findText(el, needle, depth)
	tell application "System Events"
		if depth > 10 then return false
		try
			if role of el is "AXStaticText" then
				if (name of el as text) contains needle then return true
			end if
		end try
		try
			repeat with c in (UI elements of el)
				if my findText(c, needle, depth + 1) then return true
			end repeat
		end try
		return false
	end tell
end findText
"#;

fn ax_root() -> String {
    format!(
        "set root to UI element 1 of scroll area 1 of group 1 of group 1 of window 1 of (first process whose name contains \"{}\")",
        PROCESS_MATCH
    )
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub struct AppDriver {
    child: Child,
    repo_root: PathBuf,
    artifacts: PathBuf,
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
    ) -> Result<Self> {
        let _ = Command::new("pkill").args(["-f", PROCESS_MATCH]).status();
        std::thread::sleep(Duration::from_millis(500));

        let src_tauri = repo_root.join("src-tauri");
        let bin = src_tauri.join("target/debug/inference-gateway-desktop");
        if !bin.exists() {
            bail!(
                "app binary missing at {} - run without --no-build",
                bin.display()
            );
        }

        std::fs::create_dir_all(artifacts)?;
        let log = std::fs::File::create(artifacts.join(format!("{log_name}.log")))?;

        let mut cmd = Command::new(&bin);
        cmd.current_dir(&src_tauri)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        if mock {
            cmd.env("DESKTOP_MOCK", "true")
                .env("INFER_GATEWAY_MOCK_SCENARIOS", scenarios);
        }
        let child = cmd.spawn().context("spawning app binary")?;

        let driver = Self {
            child,
            repo_root: repo_root.to_path_buf(),
            artifacts: artifacts.to_path_buf(),
        };
        driver.wait_ready()?;
        Ok(driver)
    }

    fn wait_ready(&self) -> Result<()> {
        let deadline = Instant::now() + LAUNCH_TIMEOUT;
        loop {
            match osascript(&format!(
                "tell application \"System Events\"\n{}\nend tell\nreturn \"ok\"",
                ax_root()
            )) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("-25211") || msg.contains("assistive access") {
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

    /// AX-click "+ New chat". Also serves as the mandatory webview warm-up:
    /// AXValue sets silently no-op until one AX click has landed inside the
    /// page (see AGENTS.md).
    pub fn new_chat(&self) -> Result<()> {
        self.click("+ New chat")
    }

    /// Set the textarea value, verify it stuck (read-back), then click Send.
    pub fn send(&self, text: &str) -> Result<()> {
        let set_and_read = format!(
            "tell application \"System Events\"\n{root}\nset ta to text area 1 of root\nset value of ta to \"{msg}\"\ndelay 0.3\nreturn value of ta as text\nend tell",
            root = ax_root(),
            msg = escape(text),
        );
        let mut got = osascript(&set_and_read)?;
        if got.trim() != text {
            let recover = format!(
                "tell application \"System Events\"\n{root}\nset ta to text area 1 of root\nclick ta\ndelay 0.5\nset value of ta to \"{msg}\"\ndelay 0.3\nreturn value of ta as text\nend tell",
                root = ax_root(),
                msg = escape(text),
            );
            got = osascript(&recover)?;
        }
        if got.trim() != text {
            bail!(
                "AXValue did not propagate (got {:?}) - the webview was not warmed up; \
                 a test must start with new_chat",
                got.trim()
            );
        }
        self.click("Send")
    }

    pub fn click(&self, button: &str) -> Result<()> {
        let script = format!(
            "{find}\ntell application \"System Events\"\n{root}\nset b to my findButton(root, \"{name}\", 0)\nif b is missing value then error \"button not found\"\nclick b\nreturn \"clicked\"\nend tell",
            find = FIND_BUTTON_FN,
            root = ax_root(),
            name = escape(button),
        );
        osascript(&script)
            .map(|_| ())
            .map_err(|e| anyhow!("clicking {:?}: {}", button, e))
    }

    pub fn button_exists(&self, button: &str) -> Result<bool> {
        let script = format!(
            "{find}\ntell application \"System Events\"\n{root}\nset b to my findButton(root, \"{name}\", 0)\nif b is missing value then return \"no\"\nreturn \"yes\"\nend tell",
            find = FIND_BUTTON_FN,
            root = ax_root(),
            name = escape(button),
        );
        Ok(osascript(&script)?.trim() == "yes")
    }

    pub fn text_exists(&self, needle: &str) -> Result<bool> {
        let script = format!(
            "{find}\ntell application \"System Events\"\n{root}\nif my findText(root, \"{name}\", 0) then return \"yes\"\nreturn \"no\"\nend tell",
            find = FIND_TEXT_FN,
            root = ax_root(),
            name = escape(needle),
        );
        Ok(osascript(&script)?.trim() == "yes")
    }

    pub fn model_value(&self) -> Result<String> {
        let script = format!(
            "tell application \"System Events\"\n{root}\nreturn value of pop up button 1 of group 1 of root as text\nend tell",
            root = ax_root(),
        );
        osascript(&script)
    }

    /// Resolve a cleanup/assert path against the repo root (the infer child's
    /// cwd - the app hops out of src-tauri/, see `resolve_agent_cwd`).
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

    /// Send a single keystroke to the focused element by AX key code.
    /// Used by e2e tests that need to exercise keyboard shortcuts (ArrowUp/Down).
    pub fn keypress(&self, key: &str) -> Result<()> {
        let code = match key {
            "up" => "126",
            "down" => "125",
            _ => bail!("unsupported keypress key: {key:?}"),
        };
        let script = format!(
            "tell application \"System Events\"\n{root}\nset ta to text area 1 of root\nclick ta\ndelay 0.2\nkeystroke (key code {code})\nreturn \"ok\"\nend tell",
            root = ax_root(),
        );
        osascript(&script)
            .map(|_| ())
            .map_err(|e| anyhow!("keypress {:?}: {}", key, e))
    }

    /// Capture the app's main window (by window id - a screen-rect capture
    /// grabs whatever Space is visible) into artifacts/<name>.png.
    pub fn screenshot(&self, name: &str) -> Result<PathBuf> {
        const WINDOW_ID_SWIFT: &str = r#"
import CoreGraphics
import Foundation
let info = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
var best: (id: Any, area: Int)? = nil
for w in info {
    let owner = w["kCGWindowOwnerName"] as? String ?? ""
    if owner.contains("inference-gateway"), let b = w["kCGWindowBounds"] as? [String: Int] {
        let area = b["Width"]! * b["Height"]!
        if best == nil || area > best!.area {
            best = (w["kCGWindowNumber"]!, area)
        }
    }
}
if let best { print(best.id) }
"#;
        let out = run_with_stdin("swift", &["-"], WINDOW_ID_SWIFT)?;
        let id = out
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u64>().ok())
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
            std::thread::sleep(Duration::from_millis(300));
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

fn run_with_stdin(cmd: &str, args: &[&str], stdin: &str) -> Result<String> {
    use std::io::Write;
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {cmd}"))?;
    child.stdin.take().unwrap().write_all(stdin.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{cmd} exited nonzero");
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
