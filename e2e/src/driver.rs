use crate::home::{self, PROCESS_MATCH, Seeds};
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(300);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const AFTER_PKILL_SETTLE: Duration = Duration::from_millis(500);
const INPUT_FOCUS_SETTLE: Duration = Duration::from_millis(500);
const RETRY_PAUSE: Duration = Duration::from_millis(300);

const MAIN_WINDOW_TITLE: &str = "Inference Gateway Desktop";
const OVERLAY_WINDOW_TITLE: &str = "Computer Use Overlay";

/// Recursive AXButton finder. `entire contents` is flaky (-1700) so every
/// button lookup walks `UI elements` instead. Matches AXCheckBox and
/// AXRadioButton too (toggle buttons) and pop-up/menu buttons with their
/// AXMenuItem entries. The ceiling matches the text finder's: a content
/// project docks the chat one level deeper than a code project's transcript.
const FIND_BUTTON_FN: &str = r#"
on findButton(el, btnName, depth)
	tell application "System Events"
		if depth > 10 then return missing value
		try
			if (role of el is "AXButton" or role of el is "AXCheckBox" or role of el is "AXRadioButton" or role of el is "AXPopUpButton" or role of el is "AXMenuButton" or role of el is "AXMenuItem") and name of el is btnName then return el
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

/// Recursive text finder. WebKit folds a button's text into the button's name
/// and exposes no AXStaticText child, so text inside a button (a media pool
/// row, a timeline clip) is matched on the button-like roles' names too.
const FIND_TEXT_FN: &str = r#"
on findText(el, needle, depth)
	tell application "System Events"
		if depth > 10 then return false
		try
			if role of el is "AXStaticText" or role of el is "AXButton" or role of el is "AXCheckBox" or role of el is "AXRadioButton" then
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

/// Recursive finder for any element whose description is `needle` or whose name
/// starts with it: a drag runs between rows and lanes that are not buttons,
/// WebKit exposes a `div`'s `aria-label` as its description, and it folds a
/// button's text into its name, so a pool row reads `logo.png 36 KB`.
const FIND_NAMED_FN: &str = r#"
on findNamed(el, needle, depth)
	tell application "System Events"
		if depth > 12 then return missing value
		try
			if name of el starts with needle then return el
		end try
		try
			if description of el is needle then return el
		end try
		try
			repeat with c in (UI elements of el)
				set r to my findNamed(c, needle, depth + 1)
				if r is not missing value then return r
			end repeat
		end try
		return missing value
	end tell
end findNamed
"#;

/// Drags with real mouse events from one screen point to another, in steps,
/// so the page sees a press, pointer moves past its drag threshold, and a release.
const DRAG_SWIFT: &str = r#"
import CoreGraphics
import Foundation
func post(_ type: CGEventType, _ x: Double, _ y: Double) {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left)!.post(tap: .cghidEventTap)
    usleep(30_000)
}
let (fx, fy, tx, ty): (Double, Double, Double, Double) = (FROM_X, FROM_Y, TO_X, TO_Y)
post(.mouseMoved, fx, fy)
post(.leftMouseDown, fx, fy)
for i in 1...20 {
    let t = Double(i) / 20
    post(.leftMouseDragged, fx + (tx - fx) * t, fy + (ty - fy) * t)
}
post(.leftMouseUp, tx, ty)
"#;

// The main window is addressed by title: "window 1" is ambiguous now that the
// app also has the computer-use monitor and overlay windows.
fn ax_root() -> String {
    format!(
        "set matches to every process whose name contains \"{PROCESS_MATCH}\"\n\
         if (count of matches) is 0 then set matches to every process whose name is \"{MAIN_WINDOW_TITLE}\"\n\
         set appProcess to item 1 of matches\n\
         set root to UI element 1 of scroll area 1 of group 1 of group 1 of window \"{MAIN_WINDOW_TITLE}\" of appProcess"
    )
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The launched app and the paths its run reads, driven through the macOS
/// accessibility tree (the recipe and its quirks are documented in AGENTS.md,
/// "Verifying the UI"). `mock_home` is the wiped HOME of a mock run, whose
/// infer daemon the driver stops.
pub struct AppDriver {
    child: Child,
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

        let driver = Self {
            child,
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

    /// AX-click "New chat". Also serves as the mandatory webview warm-up:
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
        self.click("Send").or_else(|_| {
            let script = format!(
                "tell application \"System Events\"\n{root}\nset ta to text area 1 of root\nclick ta\nend tell\ntell application \"System Events\"\nkey code 36\nend tell",
                root = ax_root(),
            );
            osascript(&script)
                .map(|_| ())
                .map_err(|e| anyhow!("clicking Send and committing with Return both failed: {e}"))
        })
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

    /// Drags the element named `from` onto the element named `to`, centre to
    /// centre, with the press moved `from_dx` points sideways.
    pub fn drag(&self, from: &str, from_dx: f64, to: &str) -> Result<()> {
        let (fx, fy) = self.centre_of(from)?;
        let fx = fx + from_dx;
        let (tx, ty) = self.centre_of(to)?;
        let script = DRAG_SWIFT
            .replace("FROM_X", &fx.to_string())
            .replace("FROM_Y", &fy.to_string())
            .replace("TO_X", &tx.to_string())
            .replace("TO_Y", &ty.to_string());
        run_with_stdin("swift", &["-"], &script)
            .map(|_| ())
            .map_err(|e| anyhow!("dragging {from:?} onto {to:?}: {e}"))
    }

    /// Whether the element named `above` sits higher on screen than `below`.
    pub fn is_above(&self, above: &str, below: &str) -> Result<bool> {
        Ok(self.centre_of(above)?.1 < self.centre_of(below)?.1)
    }

    fn centre_of(&self, name: &str) -> Result<(f64, f64)> {
        let script = format!(
            "{find}\ntell application \"System Events\"\n{root}\nset e to my findNamed(root, \"{name}\", 0)\nif e is missing value then error \"element not found\"\nset p to position of e\nset s to size of e\nreturn ((item 1 of p) + (item 1 of s) / 2 as text) & \" \" & ((item 2 of p) + (item 2 of s) / 2 as text)\nend tell",
            find = FIND_NAMED_FN,
            root = ax_root(),
            name = escape(name),
        );
        let out = osascript(&script).map_err(|e| anyhow!("finding {name:?}: {e}"))?;
        let mut nums = out
            .split_whitespace()
            .map(|n| n.replace(',', ".").parse::<f64>());
        match (nums.next(), nums.next()) {
            (Some(Ok(x)), Some(Ok(y))) => Ok((x, y)),
            _ => bail!("could not parse the centre of {name:?} from {out:?}"),
        }
    }

    /// Layout guard: the named button's bottom edge must sit above the
    /// composer textarea's top edge. Catches transcript content overflowing
    /// the conversation area across the composer (regression of PR #127).
    pub fn button_above_composer(&self, button: &str) -> Result<bool> {
        let script = format!(
            "{find}\ntell application \"System Events\"\n{root}\nset b to my findButton(root, \"{name}\", 0)\nif b is missing value then error \"button not found\"\nset bp to position of b\nset bs to size of b\nset ta to text area 1 of root\nset tp to position of ta\nset bBottom to (item 2 of bp) + (item 2 of bs)\nset taTop to item 2 of tp\nreturn (bBottom as text) & \" \" & (taTop as text)\nend tell",
            find = FIND_BUTTON_FN,
            root = ax_root(),
            name = escape(button),
        );
        let out = osascript(&script)?;
        let mut nums = out.split_whitespace().map(|p| p.parse::<f64>());
        match (nums.next(), nums.next()) {
            (Some(Ok(bottom)), Some(Ok(top))) => Ok(bottom <= top),
            _ => bail!("could not parse element positions from {out:?}"),
        }
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

    /// The composer textarea's current text - used by assert_composer, e.g. to
    /// prove that ArrowUp recalled the previous prompt.
    pub fn composer_value(&self) -> Result<String> {
        let script = format!(
            "tell application \"System Events\"\n{root}\nreturn value of text area 1 of root as text\nend tell",
            root = ax_root(),
        );
        osascript(&script)
    }

    /// The overlay starts below the primary screen's menu bar, spans its full
    /// width, and reaches the physical bottom edge behind the Dock.
    pub fn overlay_matches_primary_screen(&self) -> Result<bool> {
        let script = format!(
            r#"
import AppKit
import CoreGraphics

let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID)
    as! [[String: Any]]
guard let overlay = windows.first(where: {{ window in
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    let title = window[kCGWindowName as String] as? String ?? ""
    return owner.contains("{process}") && title == "{title}"
}}), let bounds = overlay[kCGWindowBounds as String] as? [String: CGFloat],
   let screen = NSScreen.screens.first else {{
    exit(0)
}}

let frame = screen.frame
let visible = screen.visibleFrame
let expectedTop = frame.maxY - visible.maxY
print(bounds["X"]!, bounds["Y"]!, bounds["Width"]!, bounds["Height"]!,
      frame.minX, expectedTop, frame.width, frame.height - expectedTop)
"#,
            process = PROCESS_MATCH,
            title = OVERLAY_WINDOW_TITLE,
        );
        let out = run_with_stdin("swift", &["-"], &script)?;
        let values = out
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
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

    /// Send a single keystroke to the focused element - a named key by AX key
    /// code, or any single ASCII character. Used by e2e tests that need to
    /// exercise keyboard shortcuts (ArrowUp/Down, Tab, Escape, letter keys).
    pub fn keypress(&self, key: &str) -> Result<()> {
        let stroke = match key {
            "up" => "keystroke (key code 126)".to_string(),
            "down" => "keystroke (key code 125)".to_string(),
            "tab" => "keystroke (key code 48)".to_string(),
            "escape" => "keystroke (key code 53)".to_string(),
            k if k.chars().count() == 1 && k.is_ascii() => format!("keystroke \"{}\"", escape(k)),
            _ => bail!("unsupported keypress key: {key:?}"),
        };
        let script = format!(
            "tell application \"System Events\"\n{root}\nset ta to text area 1 of root\nclick ta\ndelay 0.2\n{stroke}\nreturn \"ok\"\nend tell",
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
    let normalizedOwner = owner.lowercased()
    if (normalizedOwner.contains("inference-gateway") || normalizedOwner.contains("inference gateway")),
       let b = w["kCGWindowBounds"] as? [String: Int] {
        let name = w["kCGWindowName"] as? String ?? ""
        if name != "" && name != "Inference Gateway Desktop" { continue }
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
        let _ = Command::new("pkill").args(["-f", PROCESS_MATCH]).status();
        if let Some(home) = &self.mock_home {
            home::stop_daemon(home);
        }
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
