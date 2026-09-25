//! Screen recording for workflow capture (macOS only).
//!
//! One directory per recording under `/tmp/infer/captures/<start-timestamp>/`
//! (the CLI sandbox blocks every path under `.infer/`, `/tmp` is allowed):
//! `frames/NNNNNN.jpg` (one per second via `screencapture`) plus `events.jsonl`
//! (timestamped key presses and mouse clicks from `input_capture`).
//! On stop the directory is returned so the UI can reference it in the composer
//! and the agent can turn the workflow into a skill. Frame capture stops after
//! `MAX_FRAMES` (3 minutes at 1 fps) so a forgotten recording cannot fill the disk;
//! event logging continues until Stop.

use crate::env::mock_mode;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct RecordingHandle {
    dir: PathBuf,
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    recorder: Option<imp::Recorder>,
}

/// ponytail: fixed 1 fps JPEG, 3 minute cap (the top bar stops the recording
/// at the same mark) - add interval/duration settings only if asked.
#[cfg(target_os = "macos")]
const MAX_FRAMES: u32 = 180;

pub(crate) fn records_dir() -> PathBuf {
    PathBuf::from("/tmp").join("infer").join("captures")
}

/// `YYYY-MM-DDTHH-MM-SS` (UTC), so lexical order is chronological.
fn timestamp_dir_name(unix_secs: u64) -> String {
    let (y, m, d) = civil_from_days((unix_secs / 86_400) as i64);
    let (h, mi, s) = (
        (unix_secs / 3600) % 24,
        (unix_secs / 60) % 60,
        unix_secs % 60,
    );
    format!("{y:04}-{m:02}-{d:02}T{h:02}-{mi:02}-{s:02}")
}

/// Howard Hinnant's days-to-civil-date algorithm (day 0 = 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let year_of_era = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = doe - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((if month <= 2 { year + 1 } else { year }), month, day)
}

/// Delete the oldest recording directories until `keep - 1` remain (the new
/// recording then makes `keep` in total). Files are ignored.
fn prune_records(root: &Path, keep: u32) -> Result<(), String> {
    let target = keep.max(1) as usize - 1;
    let mut dirs: Vec<String> = std::fs::read_dir(root)
        .map_err(|e| format!("reading {}: {e}", root.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry
                .file_type()
                .ok()?
                .is_dir()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    dirs.sort();
    for name in dirs.iter().take(dirs.len().saturating_sub(target)) {
        std::fs::remove_dir_all(root.join(name))
            .map_err(|e| format!("removing old recording {name}: {e}"))?;
    }
    Ok(())
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn create_recording_dir(keep: u32) -> Result<PathBuf, String> {
    let root = records_dir();
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    prune_records(&root, keep)?;
    let dir = root.join(timestamp_dir_name(now_unix_secs()));
    std::fs::create_dir_all(dir.join("frames")).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Start a recording: permissions first, then prune, then create the directory
/// and spawn the capture threads. Returns the recording directory.
fn start_recording(keep: u32) -> Result<RecordingHandle, String> {
    if mock_mode() {
        return mock_recording(keep);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = keep;
        Err("Screen recording is only supported on macOS".into())
    }
    #[cfg(target_os = "macos")]
    {
        crate::permissions::ensure_recording_permissions()?;
        let dir = create_recording_dir(keep)?;
        let stop = Arc::new(AtomicBool::new(false));
        let recorder = Some(imp::Recorder::start(&dir, Arc::clone(&stop))?);
        Ok(RecordingHandle {
            dir,
            stop,
            recorder,
        })
    }
}

/// Mock mode: no `screencapture`, no event tap - one placeholder frame and an
/// empty `events.jsonl` so the e2e harness can exercise the button.
fn mock_recording(keep: u32) -> Result<RecordingHandle, String> {
    let dir = create_recording_dir(keep)?;
    std::fs::write(dir.join("frames").join("000001.jpg"), PLACEHOLDER_JPEG)
        .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("events.jsonl"), "").map_err(|e| e.to_string())?;
    Ok(RecordingHandle {
        dir,
        stop: Arc::new(AtomicBool::new(false)),
        #[cfg(target_os = "macos")]
        recorder: None,
    })
}

/// A tiny valid 1x1 grayscale JPEG; mock mode captures no real frames.
const PLACEHOLDER_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
    0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12,
    0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20,
    0x22, 0x2C, 0x23, 0x1C, 0x1E, 0x23, 0x1F, 0x27, 0x29, 0x2A, 0x2D, 0x2F, 0x2C, 0x2F, 0x21, 0x27,
    0x26, 0x2D, 0x31, 0x33, 0x39, 0x33, 0x31, 0x2D, 0x2D, 0x36, 0x2E, 0x3D, 0x43, 0x41, 0x36, 0x2D,
    0x36, 0x44, 0x45, 0x52, 0x4D, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01,
    0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03,
    0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12,
    0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
    0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
    0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
    0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79,
    0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98,
    0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
    0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4,
    0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01,
    0x00, 0x00, 0x3F, 0x00, 0xFB, 0xD2, 0x8A, 0x28, 0xA2, 0x8A, 0x28, 0x00, 0xFF, 0xD9,
];

/// Stop the recording, flush the event log, and return the directory path.
fn stop_recording(handle: RecordingHandle) -> String {
    handle.stop.store(true, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    if let Some(recorder) = handle.recorder {
        recorder.stop();
    }
    handle.dir.display().to_string()
}

#[tauri::command]
pub(crate) fn start_screen_recording(
    keep: u32,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let mut slot = state
        .screen_recording
        .lock()
        .map_err(|e| format!("screen recording state mutex poisoned: {e}"))?;
    if slot.is_some() {
        return Err("A screen recording is already in progress".into());
    }
    let handle = start_recording(keep.max(1))?;
    let dir = handle.dir.display().to_string();
    *slot = Some(handle);
    Ok(dir)
}

#[tauri::command]
pub(crate) fn stop_screen_recording(
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let mut slot = state
        .screen_recording
        .lock()
        .map_err(|e| format!("screen recording state mutex poisoned: {e}"))?;
    let handle = slot.take().ok_or("No screen recording in progress")?;
    Ok(stop_recording(handle))
}

#[tauri::command]
pub(crate) fn screen_recording_status(state: tauri::State<'_, crate::AppState>) -> bool {
    state
        .screen_recording
        .lock()
        .map(|slot| slot.is_some())
        .unwrap_or(false)
}

/// Finalize an in-progress recording on app exit so it is complete on disk.
pub(crate) fn stop_on_exit(state: &crate::AppState) {
    if let Ok(mut slot) = state.screen_recording.lock()
        && let Some(handle) = slot.take()
    {
        eprintln!(
            "finalized screen recording {} on exit",
            stop_recording(handle)
        );
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::input_capture::{InputCapture, write_event};
    use std::io::Write;
    use std::sync::Mutex;
    use std::thread::JoinHandle;
    use std::time::Duration;

    pub(super) struct Recorder {
        frames: JoinHandle<()>,
        input: InputCapture,
    }

    impl Recorder {
        pub(super) fn start(dir: &Path, stop: Arc<AtomicBool>) -> Result<Recorder, String> {
            let frames_dir = dir.join("frames");
            let frames = std::thread::Builder::new()
                .name("screen-record-frames".into())
                .spawn(move || frame_loop(&frames_dir, &stop))
                .map_err(|e| format!("spawning frame thread: {e}"))?;

            let events = Mutex::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("events.jsonl"))
                    .map_err(|e| format!("opening events.jsonl: {e}"))?,
            );
            let input = InputCapture::start(move |event| {
                let mut file = events.lock().unwrap_or_else(|p| p.into_inner());
                let _ = write_event(&mut *file, &event);
                let _ = file.flush();
            });
            Ok(Recorder { frames, input })
        }

        pub(super) fn stop(self) {
            self.input.stop();
            let _ = self.frames.join();
        }
    }

    fn frame_loop(frames_dir: &Path, stop: &AtomicBool) {
        let mut index: u32 = 0;
        loop {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if index >= MAX_FRAMES {
                eprintln!("screen recording: reached {MAX_FRAMES} frames; frame capture stopped");
                return;
            }
            index += 1;
            let frame = frames_dir.join(format!("{index:06}.jpg"));
            if let Err(e) = std::process::Command::new("screencapture")
                .args(["-x", "-t", "jpg", "-C"])
                .arg(&frame)
                .status()
            {
                eprintln!("screen recording: screencapture failed: {e}");
            }
            // Sleep up to 1s in slices so Stop reacts within ~100ms.
            for _ in 0..10 {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("screen-records-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn prune_keeps_newest_directories_and_ignores_files() {
        let root = temp_root("prune");
        for name in [
            "2026-01-01T00-00-00",
            "2026-01-02T00-00-00",
            "2026-01-03T00-00-00",
            "2026-01-04T00-00-00",
        ] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        std::fs::write(root.join("not-a-recording.txt"), b"x").unwrap();

        prune_records(&root, 3).unwrap();

        // keep - 1 = 2 newest survive; the file is untouched.
        assert!(root.join("2026-01-03T00-00-00").is_dir());
        assert!(root.join("2026-01-04T00-00-00").is_dir());
        assert!(!root.join("2026-01-02T00-00-00").exists());
        assert!(!root.join("2026-01-01T00-00-00").exists());
        assert!(root.join("not-a-recording.txt").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prune_with_keep_one_removes_all_old_recordings() {
        let root = temp_root("prune-keep-1");
        std::fs::create_dir_all(root.join("2026-01-01T00-00-00")).unwrap();
        prune_records(&root, 1).unwrap();
        assert!(!root.join("2026-01-01T00-00-00").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dir_names_sort_chronologically() {
        assert_eq!(timestamp_dir_name(0), "1970-01-01T00-00-00");
        assert_eq!(timestamp_dir_name(1_789_209_000), "2026-09-12T10-30-00");
        assert!(timestamp_dir_name(1_789_209_000) < timestamp_dir_name(1_789_209_000 + 1));
    }
}
