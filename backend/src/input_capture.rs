//! Keyboard and mouse input capture, shared by workflow capture
//! (`screen_records`) and agent recordings (`agent`): every key press and
//! mouse click reaches a caller-supplied sink as an [`InputEvent`] whose `t` is
//! seconds since [`InputCapture::start`].
//!
//! macOS listens on a `CGEventTap` on its own run-loop thread, Windows on
//! `WH_KEYBOARD_LL` / `WH_MOUSE_LL` hooks on their own message-loop thread and
//! Linux X11 on an `XRecord` context streamed over its own connection.
//! Wayland has no hook (like the CLI's `x11grab` capture), so a refused or
//! unavailable hook only means no events.

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

type Sink = Box<dyn Fn(InputEvent) + Send + Sync>;

/// Modifier bits in [`keys_string`] priority order: `ctrl`, `alt`, `cmd`, `shift`.
const MASK_CTRL: u64 = 1;
const MASK_ALT: u64 = 2;
const MASK_CMD: u64 = 4;
const MASK_SHIFT: u64 = 8;

/// `ctrl+alt+cmd+shift` prefix (when set) plus the lowercased key text, so a
/// cmd+shift+p press logs `keys: "cmd+shift+p"`. No keycode table needed.
fn keys_string(mask: u64, text: &str) -> String {
    let mut keys = String::new();
    for (bit, name) in [
        (MASK_CTRL, "ctrl"),
        (MASK_ALT, "alt"),
        (MASK_CMD, "cmd"),
        (MASK_SHIFT, "shift"),
    ] {
        if mask & bit != 0 {
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

pub(crate) fn write_event<W: Write>(writer: &mut W, event: &InputEvent) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")
}

pub(crate) struct InputCapture {
    #[cfg(target_os = "macos")]
    tap: Option<imp::Tap>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    hooks: Option<imp::Capture>,
}

impl InputCapture {
    /// Start delivering input to `sink` from a background thread. Never fails:
    /// a hook the OS refuses (missing Accessibility / Input Monitoring on
    /// macOS, no X11 display on Linux) only means no events.
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
            let hooks = imp::Capture::start(Box::new(sink))
                .map_err(|e| eprintln!("input capture: {e}"))
                .ok();
            InputCapture { hooks }
        }
    }

    pub(crate) fn stop(self) {
        #[cfg(target_os = "macos")]
        if let Some(tap) = self.tap {
            tap.stop();
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if let Some(hooks) = self.hooks {
            hooks.stop();
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
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        hooks: None,
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{InputEvent, MASK_ALT, MASK_CMD, MASK_CTRL, MASK_SHIFT, Sink, keys_string};
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

    /// CG event flags to the shared [`keys_string`] mask bits.
    fn capture_mask(flags: u64) -> u64 {
        let mut mask = 0;
        if flags & FLAG_CONTROL != 0 {
            mask |= MASK_CTRL;
        }
        if flags & FLAG_ALTERNATE != 0 {
            mask |= MASK_ALT;
        }
        if flags & FLAG_COMMAND != 0 {
            mask |= MASK_CMD;
        }
        if flags & FLAG_SHIFT != 0 {
            mask |= MASK_SHIFT;
        }
        mask
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
                let keys = keys_string(capture_mask(flags), &text);
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

#[cfg(target_os = "windows")]
mod imp {
    use super::{InputEvent, MASK_ALT, MASK_CMD, MASK_CTRL, MASK_SHIFT, Sink, keys_string};
    use std::cell::Cell;
    use std::ptr;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, ToUnicode};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
        PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
        WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };

    /// Virtual keys that act as modifiers, mapped to the shared mask bits.
    fn vk_modifier(vk: u32) -> Option<u64> {
        match vk {
            0x10 | 0xA0 | 0xA1 => Some(MASK_SHIFT),
            0x11 | 0xA2 | 0xA3 => Some(MASK_CTRL),
            0x12 | 0xA4 | 0xA5 => Some(MASK_ALT),
            0x5B | 0x5C => Some(MASK_CMD),
            _ => None,
        }
    }

    /// The printed character of a virtual key under the current keyboard state,
    /// the layout lookup `ToUnicode` performs.
    fn key_text(vk: u32, scan_code: u32) -> String {
        let mut state = [0u8; 256];
        if unsafe { GetKeyboardState(state.as_mut_ptr()) } == 0 {
            return String::new();
        }
        let mut buffer = [0u16; 32];
        let written = unsafe { ToUnicode(vk, scan_code, state.as_ptr(), &mut buffer, 0) };
        if written <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..written as usize])
        }
    }

    struct HookContext {
        start: Instant,
        sink: Sink,
        modifiers: Cell<u64>,
    }

    /// The hook procedures carry no user data, so the one live capture exposes
    /// its context address here. Low-level hook callbacks arrive only on the
    /// installing thread, serialized with its message loop.
    static CURRENT: Mutex<Option<usize>> = Mutex::new(None);

    /// The live capture's context, if one is installed.
    fn current() -> Option<&'static HookContext> {
        let current = CURRENT.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: the address is the box the capture thread owns; it unhooks
        // both hooks before freeing it, so no callback can run after that.
        let context = current.map(|address| unsafe { &*(address as *const HookContext) });
        drop(current);
        context
    }

    pub(super) struct Capture {
        thread: JoinHandle<()>,
        thread_id: mpsc::Receiver<u32>,
    }

    impl Capture {
        pub(super) fn start(sink: Sink) -> Result<Capture, String> {
            let mut current = CURRENT.lock().unwrap_or_else(|p| p.into_inner());
            if current.is_some() {
                return Err("another input capture is already running".into());
            }
            let (thread_id_tx, thread_id) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("input-capture-hooks".into())
                .spawn(move || loop_input(Box::new(sink), thread_id_tx))
                .map_err(|e| format!("spawning the input capture thread: {e}"))?;
            Ok(Capture { thread, thread_id })
        }

        pub(super) fn stop(self) {
            if let Some(thread_id) = self.thread_id.recv().ok() {
                let _ = unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
            }
            let _ = self.thread.join();
        }
    }

    fn loop_input(context: Box<HookContext>, thread_id: mpsc::Sender<u32>) {
        // The message queue for this thread must exist before the thread id is
        // announced through the channel, or a WM_QUIT posted right after
        // `start` would fail with no queue to receive it.
        let mut probe: MSG = unsafe { std::mem::zeroed() };
        let _ = unsafe { PeekMessageW(&mut probe, ptr::null_mut(), 0, 0, PM_NOREMOVE) };
        let _ = thread_id.send(unsafe { GetCurrentThreadId() });
        let module = unsafe { GetModuleHandleW(ptr::null()) };
        let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0) };
        let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0) };
        if keyboard.is_null() || mouse.is_null() {
            eprintln!("input capture: SetWindowsHookExW failed; input will not be logged");
            unsafe {
                if !keyboard.is_null() {
                    UnhookWindowsHookEx(keyboard);
                }
                if !mouse.is_null() {
                    UnhookWindowsHookEx(mouse);
                }
            }
            return;
        }
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(&*context as *const HookContext as usize);
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {}
        unsafe {
            UnhookWindowsHookEx(keyboard);
            UnhookWindowsHookEx(mouse);
        }
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = None;
        drop(context);
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0
            && let Some(context) = current()
            && !lparam.is_null()
        {
            let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            handle_keyboard(context, wparam as u32, info);
        }
        unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0
            && let Some(context) = current()
            && !lparam.is_null()
        {
            let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            handle_mouse(context, wparam as u32, info);
        }
        unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
    }

    fn handle_keyboard(context: &HookContext, message: u32, info: &KBDLLHOOKSTRUCT) {
        let pressed = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let released = message == WM_KEYUP || message == WM_SYSKEYUP;
        if let Some(mask) = vk_modifier(info.vkCode) {
            let was = context.modifiers.get();
            let now = if pressed {
                was | mask
            } else if released {
                was & !mask
            } else {
                return;
            };
            if now == was {
                return;
            }
            context.modifiers.set(now);
            (context.sink)(InputEvent::Key {
                t: context.start.elapsed().as_secs_f64(),
                keys: keys_string(now, ""),
                text: String::new(),
            });
            return;
        }
        if !pressed {
            return;
        }
        let text = key_text(info.vkCode, info.scanCode);
        (context.sink)(InputEvent::Key {
            t: context.start.elapsed().as_secs_f64(),
            keys: keys_string(context.modifiers.get(), &text),
            text,
        });
    }

    fn handle_mouse(context: &HookContext, message: u32, info: &MSLLHOOKSTRUCT) {
        let button = match message {
            WM_LBUTTONDOWN => "left",
            WM_RBUTTONDOWN => "right",
            _ => return,
        };
        (context.sink)(InputEvent::Click {
            t: context.start.elapsed().as_secs_f64(),
            button,
            x: info.pt.x,
            y: info.pt.y,
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn vk_modifier_maps_the_five_modifier_groups() {
            assert_eq!(vk_modifier(0x10), Some(MASK_SHIFT));
            assert_eq!(vk_modifier(0xA1), Some(MASK_SHIFT));
            assert_eq!(vk_modifier(0x11), Some(MASK_CTRL));
            assert_eq!(vk_modifier(0x12), Some(MASK_ALT));
            assert_eq!(vk_modifier(0x5B), Some(MASK_CMD));
            assert_eq!(vk_modifier(0x41), None);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{InputEvent, MASK_ALT, MASK_CMD, MASK_CTRL, MASK_SHIFT, Sink, keys_string};
    use std::thread::JoinHandle;
    use std::time::Instant;
    use x11rb::connection::Connection;
    use x11rb::protocol::record::{CS, ConnectionExt as _, Range, Range8};
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    /// Core X event types reported in the record stream.
    const KEY_PRESS: u8 = 2;
    const BUTTON_PRESS: u8 = 4;

    /// Modifier state bits of core X events: Shift, Control, Mod1 (Alt) and
    /// Mod4 (Super on the usual X keyboard setup).
    const X_SHIFT: u16 = 1 << 0;
    const X_CONTROL: u16 = 1 << 2;
    const X_MOD1: u16 = 1 << 5;
    const X_MOD4: u16 = 1 << 6;

    pub(super) struct Capture {
        thread: JoinHandle<()>,
        control: RustConnection,
        context: u32,
    }

    impl Capture {
        pub(super) fn start(sink: Sink) -> Result<Capture, String> {
            let (control, _) = RustConnection::connect(None)
                .map_err(|e| format!("opening the X11 control connection: {e:?}"))?;
            let context = control
                .generate_id()
                .map_err(|e| format!("allocating an XRecord context id: {e:?}"))?;
            let range = Range {
                device_events: Range8 {
                    first: KEY_PRESS,
                    last: BUTTON_PRESS,
                },
                ..Default::default()
            };
            let cookie = control
                .record_create_context(context, 0, &[CS::ALL_CLIENTS.into()], &[range])
                .map_err(|e| format!("creating the XRecord context: {e:?}"))?;
            cookie
                .check()
                .map_err(|e| format!("creating the XRecord context: {e:?}"))?;
            let thread = std::thread::Builder::new()
                .name("input-capture-xrecord".into())
                .spawn(move || record_loop(sink, context))
                .map_err(|e| format!("spawning the input capture thread: {e}"))?;
            Ok(Capture {
                thread,
                control,
                context,
            })
        }

        pub(super) fn stop(self) {
            let Capture {
                thread,
                control,
                context,
            } = self;
            if let Ok(cookie) = control.record_disable_context(context) {
                let _ = cookie.check();
            }
            // Closing also frees the context: X resources of a disconnecting
            // client go away, so the stream ends even if the disable above
            // did not reach the server.
            drop(control);
            let _ = thread.join();
        }
    }

    fn record_loop(sink: Sink, context: u32) {
        let (data, _) = match RustConnection::connect(None) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("input capture: opening the X11 data connection: {e:?}");
                return;
            }
        };
        let keymap = KeysymTable::load(&data);
        if keymap.is_none() {
            eprintln!("input capture: reading the X11 keymap failed; key text stays empty");
        }
        let start = Instant::now();
        let cookie = match data.record_enable_context(context) {
            Ok(cookie) => cookie,
            Err(e) => {
                eprintln!("input capture: enabling the XRecord context: {e:?}");
                return;
            }
        };
        for reply in cookie {
            match reply {
                Ok(reply) => {
                    for event in record_events(&reply.data) {
                        deliver(keymap.as_ref(), start, &sink, event);
                    }
                }
                Err(e) => {
                    eprintln!("input capture: XRecord stream ended: {e:?}");
                    break;
                }
            }
        }
    }

    /// Input inside one EnableContext reply, mapped to the event shapes the
    /// sink expects (left and right buttons only, like the other platforms).
    #[derive(Debug, PartialEq)]
    enum RecordEvent {
        Key {
            keycode: u8,
            state: u16,
        },
        Button {
            button: &'static str,
            x: i32,
            y: i32,
        },
    }

    fn deliver(keymap: Option<&KeysymTable>, start: Instant, sink: &Sink, event: RecordEvent) {
        let t = start.elapsed().as_secs_f64();
        match event {
            RecordEvent::Key { keycode, state } => {
                let text = keymap
                    .map(|table| table.text_for(keycode, state))
                    .unwrap_or_default();
                (sink)(InputEvent::Key {
                    t,
                    keys: keys_string(x11_mask(state), &text),
                    text,
                });
            }
            RecordEvent::Button { button, x, y } => {
                (sink)(InputEvent::Click { t, button, x, y });
            }
        }
    }

    /// Each reply batch starts with a CARD32 sequence number followed by
    /// complete 32-byte core events (`element_header: 0`). Offset 0 covers a
    /// stream that carries no sequence prefix.
    fn record_events(data: &[u8]) -> Vec<RecordEvent> {
        for offset in [4, 0] {
            let Some((chunks, _)) = data.get(offset..).map(|body| body.as_chunks::<32>()) else {
                continue;
            };
            let events: Vec<_> = chunks.iter().filter_map(|event| parse_event(event)).collect();
            if !events.is_empty() {
                return events;
            }
        }
        Vec::new()
    }

    /// Core X event layout: 1 type, 1 detail, 2 sequence, 4 timestamp, 4 root,
    /// 4 event, 4 child, 2 root-x (s16), 2 root-y (s16), 2 event-x, 2 event-y,
    /// 2 modifier state (u16).
    fn parse_event(event: &[u8]) -> Option<RecordEvent> {
        match event[0] {
            KEY_PRESS => Some(RecordEvent::Key {
                keycode: event[1],
                state: u16::from_le_bytes([event[28], event[29]]),
            }),
            BUTTON_PRESS => match event[1] {
                1 | 3 => Some(RecordEvent::Button {
                    button: if event[1] == 1 { "left" } else { "right" },
                    x: i16::from_le_bytes([event[20], event[21]]) as i32,
                    y: i16::from_le_bytes([event[22], event[23]]) as i32,
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// Modifier state bits to the shared `keys_string` bits.
    fn x11_mask(state: u16) -> u64 {
        let mut mask = 0;
        if state & X_SHIFT != 0 {
            mask |= MASK_SHIFT;
        }
        if state & X_CONTROL != 0 {
            mask |= MASK_CTRL;
        }
        if state & X_MOD1 != 0 {
            mask |= MASK_ALT;
        }
        if state & X_MOD4 != 0 {
            mask |= MASK_CMD;
        }
        mask
    }

    /// Keycode to keysym lookup, kept from the data connection's keymap.
    struct KeysymTable {
        first: u8,
        per_keycode: usize,
        keysyms: Vec<u32>,
    }

    impl KeysymTable {
        fn load(conn: &RustConnection) -> Option<KeysymTable> {
            let setup = conn.setup();
            let first = setup.min_keycode;
            let count = setup.max_keycode.saturating_sub(first).saturating_add(1);
            let reply = conn.get_keyboard_mapping(first, count).ok()?.reply().ok()?;
            Some(KeysymTable {
                first,
                per_keycode: reply.keysyms_per_keycode as usize,
                keysyms: reply.keysyms,
            })
        }

        /// The printed character of a keycode under `state` (the shift bit
        /// picks the shifted keysym).
        fn text_for(&self, keycode: u8, state: u16) -> String {
            if self.per_keycode == 0 || keycode < self.first {
                return String::new();
            }
            let row = (keycode - self.first) as usize;
            let index = usize::from(state & X_SHIFT != 0 && self.per_keycode > 1);
            let Some(keysym) = self.keysyms.get(row * self.per_keycode + index).copied() else {
                return String::new();
            };
            keysym_text(keysym)
        }
    }

    /// Characters a keysym prints: Latin-1 keysyms map 1:1 (letters, digits,
    /// space) and the few control keys with a printed character to their
    /// control character; everything else (arrows, function keys) prints none.
    fn keysym_text(keysym: u32) -> String {
        match keysym {
            0x20..=0xff => String::from(keysym as u8 as char),
            0xff08 => "\u{8}".into(),
            0xff09 => "\t".into(),
            0xff0a => "\n".into(),
            0xff0d => "\r".into(),
            _ => String::new(),
        }
    }

    #[cfg(test)]
    mod tests {
        const KEY_RELEASE: u8 = 3;

        use super::*;

        fn event_bytes(type_code: u8, detail: u8, x: i16, y: i16, state: u16) -> Vec<u8> {
            let mut event = vec![0u8; 32];
            event[0] = type_code;
            event[1] = detail;
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[28..30].copy_from_slice(&state.to_le_bytes());
            event
        }

        #[test]
        fn parses_key_and_button_press_events_from_a_record_batch() {
            let mut batch = 0u32.to_le_bytes().to_vec();
            batch.extend(event_bytes(KEY_PRESS, 38, 100, 200, X_CONTROL));
            batch.extend(event_bytes(BUTTON_PRESS, 3, -5, 7, 0));
            assert_eq!(
                record_events(&batch),
                vec![
                    RecordEvent::Key {
                        keycode: 38,
                        state: X_CONTROL
                    },
                    RecordEvent::Button {
                        button: "right",
                        x: -5,
                        y: 7
                    }
                ]
            );
        }

        #[test]
        fn parses_events_that_carry_no_sequence_prefix() {
            let batch = event_bytes(KEY_PRESS, 38, 0, 0, 0);
            assert_eq!(
                record_events(&batch),
                vec![RecordEvent::Key {
                    keycode: 38,
                    state: 0
                }]
            );
        }

        #[test]
        fn ignores_key_release_events() {
            let batch = event_bytes(KEY_RELEASE, 38, 0, 0, 0);
            assert_eq!(record_events(&batch), Vec::new());
        }

        #[test]
        fn latin1_keysyms_print_their_character_and_arrows_print_none() {
            assert_eq!(keysym_text(0x61), "a");
            assert_eq!(keysym_text(0x20), " ");
            assert_eq!(keysym_text(0xff0d), "\r");
            assert_eq!(keysym_text(0xff09), "\t");
            assert_eq!(keysym_text(0xff52), "");
        }

        #[test]
        fn x11_modifier_bits_map_to_the_shared_names() {
            assert_eq!(x11_mask(X_CONTROL), MASK_CTRL);
            assert_eq!(x11_mask(X_MOD1), MASK_ALT);
            assert_eq!(x11_mask(X_MOD4), MASK_CMD);
            assert_eq!(x11_mask(X_SHIFT), MASK_SHIFT);
        }
    }
}
