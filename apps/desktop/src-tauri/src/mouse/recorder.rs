//! Recording a keyboard shortcut for a mouse button, system shortcuts included (⌘⇥, ⌘Space,
//! ⌃↑…), which macOS would otherwise act on before Kleari's window sees them.
//!
//! While recording, a keyboard event tap at the HID level (ahead of macOS's own shortcut
//! handling) swallows every key press and release, and the mouse settings pause (`active`).
//! Each press is reported to the page as the shortcut it makes, or as one Kleari can't send.
//! Nothing is stored here; the page saves the shortcut once the user confirms it.
//!
//! Recording ends when the page stops it (confirm, cancel, leaving the window or the page),
//! on Escape alone, or after `MAX_RECORDING` at most, so the keyboard can never stay captured.
//! It needs Accessibility, as the mouse settings do; without it `start` says so and the page
//! records inside its own window instead (where system shortcuts can't be caught).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

use super::config::{Modifiers, Shortcut, KEYS};

/// Sent to the page for each key press while recording, and when recording ends by itself.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const RECORD_EVENT: &str = "mouse-shortcut-record";

/// Recording ends by itself after this long.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAX_RECORDING: Duration = Duration::from_secs(30);

/// The recording running now; each start and stop moves it on, so an older recording's thread
/// sees it has been replaced and lets the keyboard go.
static CURRENT: AtomicU64 = AtomicU64::new(0);
/// Whether a recording is running (its number is `CURRENT`).
static RUNNING: AtomicU64 = AtomicU64::new(0);

/// A shortcut is being recorded: the mouse settings pass every event untouched meanwhile.
pub fn active() -> bool {
    let running = RUNNING.load(Ordering::Relaxed);
    running != 0 && running == CURRENT.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Recorded {
    /// A press that makes this shortcut.
    Shortcut { shortcut: Shortcut },
    /// A key Kleari can't send (a modifier alone never reaches here).
    Unsupported,
    /// Recording ended without the page asking: Escape, or the time ran out.
    Ended,
}

/// The shortcut a key press makes: the key's name from `KEYS`, or `None` for another key.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn recorded(code: u16, modifiers: Modifiers) -> Recorded {
    KEYS.iter()
        .find(|(_, known)| *known == code)
        .map_or(Recorded::Unsupported, |(name, _)| Recorded::Shortcut {
            shortcut: Shortcut {
                key: (*name).to_owned(),
                modifiers,
            },
        })
}

/// Why recording couldn't start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// Accessibility isn't allowed, or macOS refused the tap.
    NotAllowed,
    /// Only macOS has the event tap.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported,
}

/// Starts recording (ending any recording before it). Waits until the keyboard is captured.
#[cfg(target_os = "macos")]
pub fn start(app: tauri::AppHandle) -> Result<(), StartError> {
    if !super::tap::allowed() {
        return Err(StartError::NotAllowed);
    }
    let number = CURRENT.fetch_add(1, Ordering::Relaxed) + 1;
    if macos::run(app, number) {
        Ok(())
    } else {
        Err(StartError::NotAllowed)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn start(_app: tauri::AppHandle) -> Result<(), StartError> {
    Err(StartError::Unsupported)
}

/// Ends the recording, if one runs; the keyboard is let go within a tenth of a second.
pub fn stop() {
    CURRENT.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::ptr::NonNull;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRetained, CFRunLoop};
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventTapProxy, CGEventType,
    };
    use tauri::{AppHandle, Emitter};

    use super::{recorded, Recorded, CURRENT, MAX_RECORDING, RECORD_EVENT, RUNNING};
    use crate::mouse::config::Modifiers;

    /// How often the recording's thread checks whether it should end.
    const CHECK_SECONDS: f64 = 0.1;
    /// How long `run` waits for macOS to accept or refuse the tap.
    const READY: Duration = Duration::from_secs(1);
    /// `kVK_Escape`.
    const ESCAPE: u16 = 53;

    struct Context {
        app: AppHandle,
        number: u64,
        port: std::cell::RefCell<Option<CFRetained<CFMachPort>>>,
    }

    impl Context {
        fn current(&self) -> bool {
            CURRENT.load(Ordering::Relaxed) == self.number
        }
    }

    const fn bit(kind: CGEventType) -> u64 {
        1 << kind.0
    }

    /// Captures the keyboard on a new thread for recording `number`; `true` once captured.
    pub fn run(app: AppHandle, number: u64) -> bool {
        let (ready, installed) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("kleari-shortcut".to_owned())
            .spawn(move || record(app, number, &ready));
        spawned.is_ok() && installed.recv_timeout(READY).unwrap_or(false)
    }

    fn record(app: AppHandle, number: u64, ready: &mpsc::Sender<bool>) {
        let context = Box::into_raw(Box::new(Context {
            app: app.clone(),
            number,
            port: std::cell::RefCell::new(None),
        }));
        let mask = bit(CGEventType::KeyDown) | bit(CGEventType::KeyUp);
        // SAFETY: `callback` matches the tap callback type and only reads `context`, which lives
        // until after the tap is invalidated below.
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::HIDEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(callback),
                context.cast(),
            )
        };
        let mut captured = false;
        let mut timed_out = false;
        if let Some(port) = port {
            if let (Some(source), Some(run_loop)) = (
                CFMachPort::new_run_loop_source(None, Some(&port), 0),
                CFRunLoop::current(),
            ) {
                // SAFETY: a static macOS defines; read once here.
                let mode = unsafe { kCFRunLoopDefaultMode };
                run_loop.add_source(Some(&source), mode);
                // SAFETY: the callback reads `port` only on this thread, inside the run loop.
                unsafe { &*context }.port.replace(Some(port.clone()));
                CGEvent::tap_enable(&port, true);
                captured = true;
                RUNNING.store(number, Ordering::Relaxed);
                // `run` may have stopped waiting.
                let _ = ready.send(true);
                let began = Instant::now();
                while CURRENT.load(Ordering::Relaxed) == number {
                    if began.elapsed() >= MAX_RECORDING {
                        timed_out = true;
                        break;
                    }
                    CFRunLoop::run_in_mode(mode, CHECK_SECONDS, false);
                }
                CGEvent::tap_enable(&port, false);
            }
            port.invalidate();
        }
        if !captured {
            let _ = ready.send(false);
        }
        // Only this recording's own mark is cleared; a newer one keeps its own.
        let _ = RUNNING.compare_exchange(number, 0, Ordering::Relaxed, Ordering::Relaxed);
        // SAFETY: the tap is invalidated (or was never made), so the callback can't run again.
        drop(unsafe { Box::from_raw(context) });
        if timed_out {
            // A page that went away just misses it.
            let _ = app.emit(RECORD_EVENT, Recorded::Ended);
        }
    }

    fn modifiers(flags: CGEventFlags) -> Modifiers {
        Modifiers {
            command: flags.contains(CGEventFlags::MaskCommand),
            shift: flags.contains(CGEventFlags::MaskShift),
            option: flags.contains(CGEventFlags::MaskAlternate),
            control: flags.contains(CGEventFlags::MaskControl),
        }
    }

    unsafe extern "C-unwind" fn callback(
        _proxy: CGEventTapProxy,
        kind: CGEventType,
        event: NonNull<CGEvent>,
        user: *mut c_void,
    ) -> *mut CGEvent {
        let pass = event.as_ptr();
        if user.is_null() {
            return pass;
        }
        // SAFETY: `user` is the `Context` `record` made, alive while the tap is.
        let context = unsafe { &*user.cast::<Context>() };
        if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput
        {
            if let Some(port) = context.port.borrow().as_ref() {
                CGEvent::tap_enable(port, true);
            }
            return pass;
        }
        // A recording that has been replaced or stopped lets every key through.
        if !context.current() {
            return pass;
        }
        // SAFETY: macOS passes a valid event for the duration of the call.
        let event_ref = unsafe { event.as_ref() };
        // A failure in reporting never keeps a key: it goes through unchanged.
        let swallowed = catch_unwind(AssertUnwindSafe(|| handle(context, kind, event_ref)));
        if swallowed.unwrap_or(false) {
            std::ptr::null_mut()
        } else {
            pass
        }
    }

    /// Reports a key press; every press and release is swallowed while recording.
    fn handle(context: &Context, kind: CGEventType, event: &CGEvent) -> bool {
        if kind != CGEventType::KeyDown {
            return true;
        }
        let repeat =
            CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventAutorepeat);
        if repeat != 0 {
            return true;
        }
        let code = CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
        let Ok(code) = u16::try_from(code) else {
            return true;
        };
        let modifiers = modifiers(CGEvent::flags(Some(event)));
        if code == ESCAPE && !modifiers.any() {
            // Escape alone cancels; the keyboard is let go right after.
            CURRENT.fetch_add(1, Ordering::Relaxed);
            let _ = context.app.emit(RECORD_EVENT, Recorded::Ended);
            return true;
        }
        let _ = context.app.emit(RECORD_EVENT, recorded(code, modifiers));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND: Modifiers = Modifiers {
        command: true,
        shift: false,
        option: false,
        control: false,
    };

    #[test]
    fn a_press_records_the_shortcut_it_makes() {
        // ⌘⇥ is recorded like any other shortcut.
        assert_eq!(
            recorded(48, COMMAND),
            Recorded::Shortcut {
                shortcut: Shortcut {
                    key: "tab".to_owned(),
                    modifiers: COMMAND
                }
            }
        );
        // A key Kleari can't send (the keypad's 5) says so instead.
        assert_eq!(recorded(87, COMMAND), Recorded::Unsupported);
    }

    #[test]
    fn stopping_ends_the_recording_and_a_new_one_replaces_it() {
        // Recording 7 is running.
        CURRENT.store(7, Ordering::Relaxed);
        RUNNING.store(7, Ordering::Relaxed);
        assert!(active());
        stop();
        assert!(!active(), "stopped: the mouse settings work again");
        RUNNING.store(0, Ordering::Relaxed);
        assert!(!active());
    }

    #[test]
    fn what_the_page_is_sent_is_tagged() {
        let text = serde_json::to_string(&Recorded::Ended).unwrap_or_default();
        assert_eq!(text, r#"{"kind":"ended"}"#);
        let unsupported = serde_json::to_string(&Recorded::Unsupported).unwrap_or_default();
        assert_eq!(unsupported, r#"{"kind":"unsupported"}"#);
    }
}
