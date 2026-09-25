//! Keyboard and mouse input capture, shared by workflow capture
//! (`screen_records`) and agent recordings (`agent`): every key press and
//! mouse click reaches a caller-supplied sink as an [`InputEvent`] whose `t` is
//! seconds since [`InputCapture::start`].
//!
//! macOS uses a listen-only `CGEventTap` on its own run-loop thread.
//! ponytail: macOS only - Windows (low-level hooks) and Linux X11 (XRecord)
//! capture are tracked in inference-gateway/desktop#321; until then `start`
//! is a no-op there and the event log stays empty.

use crate::env::mock_mode;
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum InputEvent {
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

#[cfg(target_os = "macos")]
type Sink = Box<dyn Fn(InputEvent) + Send + Sync>;

pub(crate) fn write_event<W: Write>(writer: &mut W, event: &InputEvent) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")
}

pub(crate) struct InputCapture {
    #[cfg(target_os = "macos")]
    tap: Option<imp::Tap>,
}

impl InputCapture {
    /// Start delivering input to `sink` from a background thread. Never fails:
    /// a tap the OS refuses (missing Accessibility / Input Monitoring) only
    /// means no events.
    pub(crate) fn start(sink: impl Fn(InputEvent) + Send + Sync + 'static) -> Self {
        if mock_mode() {
            return mock_capture(sink);
        }
        #[cfg(target_os = "macos")]
        {
            let tap = imp::Tap::start(Box::new(sink))
                .map_err(|e| eprintln!("input capture: {e}"))
                .ok();
            InputCapture { tap }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = sink;
            InputCapture {}
        }
    }

    pub(crate) fn stop(self) {
        #[cfg(target_os = "macos")]
        if let Some(tap) = self.tap {
            tap.stop();
        }
    }
}

/// Mock mode: no OS hook - two canned events so the e2e harness sees the
/// agent recording's input list fill in.
fn mock_capture(sink: impl Fn(InputEvent)) -> InputCapture {
    sink(InputEvent::Click {
        t: 0.5,
        button: "left",
        x: 640,
        y: 380,
    });
    sink(InputEvent::Key {
        t: 1.2,
        keys: "cmd+s".into(),
        text: "s".into(),
    });
    InputCapture {
        #[cfg(target_os = "macos")]
        tap: None,
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{InputEvent, Sink};
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread::JoinHandle;
    use std::time::Instant;

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
        sink: Sink,
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

    /// SAFETY: `context` is a `Box::into_raw` TapContext owned by the tap; the
    /// tap callback dereferences it only on the tap thread, and `Tap::stop`
    /// joins that thread before reclaiming the box.
    pub(super) struct Tap {
        thread: JoinHandle<()>,
        context: *mut TapContext,
        run_loop: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
    }

    unsafe impl Send for Tap {}

    impl Tap {
        pub(super) fn start(sink: Sink) -> Result<Tap, String> {
            let context = Box::into_raw(Box::new(TapContext {
                start: Instant::now(),
                sink,
            }));
            let run_loop = Arc::new(AtomicU64::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let context_addr = context as usize;
            let thread = std::thread::Builder::new()
                .name("input-capture-tap".into())
                .spawn({
                    let run_loop = Arc::clone(&run_loop);
                    let stop = Arc::clone(&stop);
                    move || unsafe { tap_loop(context_addr as *mut TapContext, stop, run_loop) }
                });
            match thread {
                Ok(thread) => Ok(Tap {
                    thread,
                    context,
                    run_loop,
                    stop,
                }),
                Err(e) => {
                    drop(unsafe { Box::from_raw(context) });
                    Err(format!("spawning event tap thread: {e}"))
                }
            }
        }

        pub(super) fn stop(self) {
            self.stop.store(true, Ordering::SeqCst);
            let rl = self.run_loop.load(Ordering::SeqCst) as *mut c_void;
            if !rl.is_null() {
                unsafe { CFRunLoopStop(rl) };
            }
            let _ = self.thread.join();
            drop(unsafe { Box::from_raw(self.context) });
        }
    }

    /// SAFETY: `context` must stay valid until this thread exits; `Tap::stop`
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
            eprintln!("input capture: CGEventTapCreate failed; input will not be logged");
            return;
        }
        let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
        if source.is_null() {
            eprintln!("input capture: CFMachPortCreateRunLoopSource failed");
            unsafe { CFRelease(tap) };
            return;
        }
        let run_loop_ref = unsafe { CFRunLoopGetCurrent() };
        unsafe { CFRunLoopAddSource(run_loop_ref, source, kCFRunLoopCommonModes) };
        run_loop.store(run_loop_ref as u64, Ordering::SeqCst);
        // Re-check the stop flag: `Tap::stop` may have read the run loop slot
        // before this thread stored it; its CFRunLoopStop call is a no-op
        // then, so exiting here prevents a runaway loop.
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
                (context.sink)(InputEvent::Key { t, keys, text });
            }
            K_CG_EVENT_LEFT_MOUSE_DOWN | K_CG_EVENT_RIGHT_MOUSE_DOWN => {
                let location = unsafe { CGEventGetLocation(event) };
                (context.sink)(InputEvent::Click {
                    t,
                    button: if event_type == K_CG_EVENT_LEFT_MOUSE_DOWN {
                        "left"
                    } else {
                        "right"
                    },
                    x: location.x as i32,
                    y: location.y as i32,
                });
            }
            _ => {}
        }
        ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_writer_emits_one_valid_json_object_per_line() {
        let mut out = Vec::new();
        write_event(
            &mut out,
            &InputEvent::Key {
                t: 12.4,
                keys: "cmd+shift+p".into(),
                text: "P".into(),
            },
        )
        .unwrap();
        write_event(
            &mut out,
            &InputEvent::Click {
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
}
