//! Screen recording for workflow capture (macOS only).
//!
//! One directory per recording under `~/.infer/screen-records/<start-timestamp>/`:
//! `frames/NNNNNN.jpg` (one per second via `screencapture`) plus `events.jsonl`
//! (timestamped key presses and mouse clicks from a listen-only `CGEventTap`).
//! On stop the directory is returned so the UI can reference it in the composer
//! and the agent can turn the workflow into a skill.

use crate::env::{home_dir, mock_mode};
#[cfg(any(target_os = "macos", test))]
use serde::Serialize;
#[cfg(any(target_os = "macos", test))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Event {
    Key {
        t: f64,
        keys: String,
        text: String,
    },
    Click {
        t: f64,
        button: &'static str,
        x: i32,
        y: i32,
    },
}

pub(crate) struct RecordingHandle {
    dir: PathBuf,
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    recorder: Option<imp::Recorder>,
}

pub(crate) fn records_dir() -> PathBuf {
    home_dir().join(".infer").join("screen-records")
}

#[cfg(any(target_os = "macos", test))]
fn write_event<W: Write>(writer: &mut W, event: &Event) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")
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
    use std::ffi::c_void;
    use std::fs::File;
    use std::ptr;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    // Listen-only tap: key down, flags (modifiers), left/right mouse down.
    // kCGEventLeftMouseDown = 1, RightMouseDown = 3, KeyDown = 10, FlagsChanged = 12.
    const EVENT_TAP_MASK: u64 = (1 << 1) | (1 << 3) | (1 << 10) | (1 << 12);
    const K_CG_SESSION_EVENT_TAP: i32 = 1;
    const K_CG_TAIL_APPEND_EVENT_TAP: i32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: i32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const K_CG_EVENT_KEY_DOWN: u32 = 10;
    const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;

    // CGEventFlagsType bit masks.
    const FLAG_SHIFT: u64 = 1 << 17;
    const FLAG_CONTROL: u64 = 1 << 18;
    const FLAG_ALTERNATE: u64 = 1 << 19;
    const FLAG_COMMAND: u64 = 1 << 20;

    type CGEventTapCallBack =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: i32,
            place: i32,
            options: i32,
            event_mask: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGEventKeyboardGetUnicodeString(
            event: *mut c_void,
            max_length: usize,
            actual_length: *mut usize,
            unicode_string: *mut u16,
        );
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: *const c_void,
            order: isize,
        ) -> *mut c_void;
        fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopRun();
        fn CFRunLoopStop(run_loop: *mut c_void);
        fn CFRelease(cf: *const c_void);
        static kCFRunLoopCommonModes: *const c_void;
    }

    struct TapContext {
        start: Instant,
        events: Mutex<File>,
    }

    fn log_event(context: &TapContext, event: Event) {
        let mut file = context
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = write_event(&mut *file, &event);
        let _ = file.flush();
    }

    /// `ctrl+alt+cmd+shift` prefix (when set) plus the lowercased key text, so
    /// a cmd+shift+p press logs `keys: "cmd+shift+p"`. No keycode table needed.
    fn keys_string(flags: u64, text: &str) -> String {
        let mut keys = String::new();
        for (mask, name) in [
            (FLAG_CONTROL, "ctrl"),
            (FLAG_ALTERNATE, "alt"),
            (FLAG_COMMAND, "cmd"),
            (FLAG_SHIFT, "shift"),
        ] {
            if flags & mask != 0 {
                if !keys.is_empty() {
                    keys.push('+');
                }
                keys.push_str(name);
            }
        }
        let key = text.to_lowercase();
        if !key.is_empty() {
            if !keys.is_empty() {
                keys.push('+');
            }
            keys.push_str(&key);
        }
        keys
    }

    /// SAFETY: `context` is a `Box::into_raw` TapContext owned by the recording
    /// handle; the tap callback dereferences it only on the tap thread, and
    /// `Recorder::stop` joins that thread before reclaiming the box.
    pub(super) struct Recorder {
        frames: JoinHandle<()>,
        tap: JoinHandle<()>,
        context: *mut TapContext,
        run_loop: Arc<AtomicU64>,
    }

    unsafe impl Send for Recorder {}

    impl Recorder {
        pub(super) fn start(dir: &Path, stop: Arc<AtomicBool>) -> Result<Recorder, String> {
            let frames_dir = dir.join("frames");
            let frames = std::thread::Builder::new()
                .name("screen-record-frames".into())
                .spawn({
                    let stop = Arc::clone(&stop);
                    move || frame_loop(&frames_dir, &stop)
                })
                .map_err(|e| format!("spawning frame thread: {e}"))?;

            let events = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("events.jsonl"))
                .map_err(|e| format!("opening events.jsonl: {e}"))?;
            let context = Box::into_raw(Box::new(TapContext {
                start: Instant::now(),
                events: Mutex::new(events),
            }));
            let run_loop = Arc::new(AtomicU64::new(0));
            let context_addr = context as usize;
            let tap = std::thread::Builder::new()
                .name("screen-record-tap".into())
                .spawn({
                    let run_loop = Arc::clone(&run_loop);
                    move || unsafe { tap_loop(context_addr as *mut TapContext, stop, run_loop) }
                })
                .map_err(|e| format!("spawning event tap thread: {e}"))?;
            Ok(Recorder {
                frames,
                tap,
                context,
                run_loop,
            })
        }

        pub(super) fn stop(self) {
            let rl = self.run_loop.load(Ordering::SeqCst) as *mut c_void;
            if !rl.is_null() {
                unsafe { CFRunLoopStop(rl) };
            }
            let _ = self.tap.join();
            drop(unsafe { Box::from_raw(self.context) });
            let _ = self.frames.join();
        }
    }

    fn frame_loop(frames_dir: &Path, stop: &AtomicBool) {
        // ponytail: fixed 1 fps JPEG, add a frame interval setting only if
        // disk use is reported.
        let mut index: u32 = 0;
        loop {
            if stop.load(Ordering::SeqCst) {
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

    /// SAFETY: `context` must stay valid until this thread exits; `Recorder::stop`
    /// joins the thread before freeing it.
    unsafe fn tap_loop(context: *mut TapContext, stop: Arc<AtomicBool>, run_loop: Arc<AtomicU64>) {
        let tap = unsafe {
            CGEventTapCreate(
                K_CG_SESSION_EVENT_TAP,
                K_CG_TAIL_APPEND_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                EVENT_TAP_MASK,
                tap_callback,
                context.cast(),
            )
        };
        if tap.is_null() {
            eprintln!("screen recording: CGEventTapCreate failed; key presses will not be logged");
            return;
        }
        let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
        if source.is_null() {
            eprintln!("screen recording: CFMachPortCreateRunLoopSource failed");
            unsafe { CFRelease(tap) };
            return;
        }
        let run_loop_ref = unsafe { CFRunLoopGetCurrent() };
        unsafe { CFRunLoopAddSource(run_loop_ref, source, kCFRunLoopCommonModes) };
        run_loop.store(run_loop_ref as u64, Ordering::SeqCst);
        // Re-check the stop flag: stop_screen_recording may have read the run
        // loop slot before this thread stored it; its CFRunLoopStop call is a
        // no-op then, so exiting here prevents a runaway loop.
        if !stop.load(Ordering::SeqCst) {
            unsafe { CFRunLoopRun() };
        }
        unsafe {
            CFRelease(source);
            CFRelease(tap);
        }
    }

    unsafe extern "C" fn tap_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void {
        let context = unsafe { &*(user_info.cast::<TapContext>()) };
        let t = context.start.elapsed().as_secs_f64();
        match event_type {
            K_CG_EVENT_KEY_DOWN | K_CG_EVENT_FLAGS_CHANGED => {
                let mut buffer = [0u16; 64];
                let mut length = 0usize;
                let flags = unsafe {
                    CGEventKeyboardGetUnicodeString(
                        event,
                        buffer.len(),
                        &mut length,
                        buffer.as_mut_ptr(),
                    );
                    CGEventGetFlags(event)
                };
                let text = String::from_utf16_lossy(&buffer[..length.min(buffer.len())]);
                let keys = keys_string(flags, &text);
                log_event(context, Event::Key { t, keys, text });
            }
            K_CG_EVENT_LEFT_MOUSE_DOWN | K_CG_EVENT_RIGHT_MOUSE_DOWN => {
                let location = unsafe { CGEventGetLocation(event) };
                log_event(
                    context,
                    Event::Click {
                        t,
                        button: if event_type == K_CG_EVENT_LEFT_MOUSE_DOWN {
                            "left"
                        } else {
                            "right"
                        },
                        x: location.x as i32,
                        y: location.y as i32,
                    },
                );
            }
            _ => {}
        }
        ptr::null_mut()
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
    fn events_writer_emits_one_valid_json_object_per_line() {
        let mut out = Vec::new();
        write_event(
            &mut out,
            &Event::Key {
                t: 12.4,
                keys: "cmd+shift+p".into(),
                text: "P".into(),
            },
        )
        .unwrap();
        write_event(
            &mut out,
            &Event::Click {
                t: 13.0,
                button: "left",
                x: 812,
                y: 344,
            },
        )
        .unwrap();

        let lines: Vec<String> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines.len(), 2);
        let key: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(key["kind"], "key");
        assert_eq!(key["keys"], "cmd+shift+p");
        assert_eq!(key["text"], "P");
        let click: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(click["kind"], "click");
        assert_eq!(click["button"], "left");
        assert_eq!(click["x"], 812);
        assert_eq!(click["y"], 344);
    }

    #[test]
    fn dir_names_sort_chronologically() {
        assert_eq!(timestamp_dir_name(0), "1970-01-01T00-00-00");
        assert_eq!(timestamp_dir_name(1_789_209_000), "2026-09-12T10-30-00");
        assert!(timestamp_dir_name(1_789_209_000) < timestamp_dir_name(1_789_209_000 + 1));
    }
}
