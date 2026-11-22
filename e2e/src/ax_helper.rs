use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!("axhelper.swift");
/// Longest a request may take: a drag posts its events with pauses, and a
/// composer write waits for the value to settle before reading it back.
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
/// Replies waiting to be read. One request is in flight at a time, so only a
/// late reply to a request that timed out can queue behind it.
const REPLY_QUEUE: usize = 4;

/// The macOS accessibility helper: a Swift process that answers one JSON line
/// per request through the AXUIElement API, instead of an `osascript` walk per
/// lookup. A reader thread owns its stdout and hands each reply over a bounded
/// channel, so every request times out instead of hanging the suite, and ids
/// pair replies with requests so a late reply is never taken for the next.
pub(crate) struct AxHelper {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<String>,
    reader: Option<JoinHandle<()>>,
    last_id: Cell<u64>,
}

impl AxHelper {
    /// Starts the helper for the app whose executable name holds
    /// `process_match`, building it first when this source is new.
    pub(crate) fn start(
        artifacts: &Path,
        process_match: &str,
        window_title: &str,
        overlay_title: &str,
    ) -> Result<Self> {
        let bin = build(artifacts)?;
        let mut child = Command::new(&bin)
            .args([process_match, window_title, overlay_title])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", bin.display()))?;
        let stdin = child.stdin.take().context("the helper has no stdin")?;
        let stdout = child.stdout.take().context("the helper has no stdout")?;
        let (sender, replies) = sync_channel(REPLY_QUEUE);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            replies,
            reader: Some(reader),
            last_id: Cell::new(0),
        })
    }

    /// Sends `op` with the fields of `args` and returns the reply's value.
    ///
    /// # Errors
    ///
    /// The helper's own error, a reply that does not arrive within
    /// `REPLY_TIMEOUT`, or a helper that exited.
    pub(crate) fn ask(&self, op: &str, args: Value) -> Result<Value> {
        let id = self.last_id.get() + 1;
        self.last_id.set(id);
        let mut request = match args {
            Value::Object(fields) => fields,
            _ => serde_json::Map::new(),
        };
        request.insert("op".into(), json!(op));
        request.insert("id".into(), json!(id));
        let mut stdin = self
            .stdin
            .as_ref()
            .context("the accessibility helper is closed")?;
        writeln!(stdin, "{}", Value::Object(request))
            .context("writing to the accessibility helper")?;

        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.replies.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    bail!("the accessibility helper did not answer {op} within {REPLY_TIMEOUT:?}")
                }
                Err(RecvTimeoutError::Disconnected) => bail!("the accessibility helper exited"),
            };
            let mut reply: Value = serde_json::from_str(&line)
                .with_context(|| format!("the accessibility helper answered {line:?}"))?;
            if reply["id"] != json!(id) {
                continue;
            }
            if reply["ok"] == json!(true) {
                return Ok(reply["value"].take());
            }
            bail!("{}", reply["error"].as_str().unwrap_or("the helper failed"));
        }
    }
}

impl Drop for AxHelper {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Compiles the helper into `artifacts` once per version of its source, so a
/// run pays for `swiftc` only after axhelper.swift changed. The binary lands
/// under a temporary name first, so an interrupted build is never reused.
fn build(artifacts: &Path) -> Result<PathBuf> {
    let mut hasher = DefaultHasher::new();
    SOURCE.hash(&mut hasher);
    let bin = artifacts.join(format!("axhelper-{:016x}", hasher.finish()));
    if bin.exists() {
        return Ok(bin);
    }
    std::fs::create_dir_all(artifacts)?;
    let source = artifacts.join("axhelper.swift");
    std::fs::write(&source, SOURCE)?;
    let partial = bin.with_extension("partial");
    let status = Command::new("swiftc")
        .args(["-O", "-o"])
        .arg(&partial)
        .arg(&source)
        .status()
        .context("running swiftc for the accessibility helper")?;
    if !status.success() {
        bail!("swiftc could not build the accessibility helper");
    }
    std::fs::rename(&partial, &bin)?;
    Ok(bin)
}
