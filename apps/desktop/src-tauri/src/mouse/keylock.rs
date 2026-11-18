//! Keyboard cleaning mode: every key press is held back from macOS and every app for a set
//! time, so the keyboard can be wiped without typing anything.
//!
//! While locked:
//! - a session-level event tap swallows key presses and releases, modifier changes, and the
//!   media, brightness and volume keys (macOS's "system-defined" events), where macOS hands
//!   them to apps at all; the power button and Touch ID never reach an app and stay working;
//! - macOS's own system shortcuts are turned off and back on exactly as while a shortcut is
//!   recorded (`recorder::hold_system_shortcuts`: saved first, so a crash restores them at the
//!   next launch), and the mouse settings pause;
//! - the first mouse or trackpad click anywhere unlocks (that click is swallowed, so it
//!   presses nothing), and so does the time running out, or Kleari quitting.
//!
//! Nothing a key press carries is read, kept or sent: the tap only drops it. It needs
//! Accessibility, like the mouse settings. One lock at a time, never while a shortcut is
//! being recorded.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

/// Sent to every window when the keyboard locks and when it unlocks.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const LOCK_EVENT: &str = "keyboard-lock";

/// The lengths the page offers, in seconds; any other is refused.
pub const DURATIONS: [u64; 4] = [30, 60, 120, 300];

/// The lock running now; each start and each end moves it on, so an older lock's thread sees
/// it has been replaced and lets the keyboard go.
static CURRENT: AtomicU64 = AtomicU64::new(0);
/// The lock whose tap is installed, or 0.
static RUNNING: AtomicU64 = AtomicU64::new(0);
/// When the running lock ends, in milliseconds since 1970; 0 while unlocked.
static ENDS_AT_MS: AtomicU64 = AtomicU64::new(0);

/// The keyboard is locked: the mouse settings pass every event untouched meanwhile.
pub fn active() -> bool {
    let running = RUNNING.load(Ordering::Relaxed);
    running != 0 && running == CURRENT.load(Ordering::Relaxed)
}

/// Ends lock `number` if it is the current one; `true` if it was.
fn end(number: u64) -> bool {
    CURRENT
        .compare_exchange(number, number + 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

/// Ends whatever lock is running (Kleari quitting).
pub fn stop() {
    let running = RUNNING.load(Ordering::Relaxed);
    if running != 0 {
        let _ = end(running);
    }
}

/// `stop`, then waits up to `limit` for the lock's thread to let the keyboard go and turn the
/// system shortcuts back on: at quit, before the process ends under it.
pub fn stop_and_wait(limit: Duration) {
    stop();
    let started = std::time::Instant::now();
    while RUNNING.load(Ordering::Relaxed) != 0 && started.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Where the lock stands, for a page that opens while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LockState {
    Locked { ends_at_ms: u64 },
    Unlocked,
}

pub fn state() -> LockState {
    let ends_at_ms = ENDS_AT_MS.load(Ordering::Relaxed);
    if active() && ends_at_ms != 0 {
        LockState::Locked { ends_at_ms }
    } else {
        LockState::Unlocked
    }
}

/// Why the lock ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unlock {
    Click,
    Time,
    Stopped,
}

/// What `LOCK_EVENT` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LockEvent {
    Locked { ends_at_ms: u64 },
    Unlocked { reason: Unlock },
}

/// How starting the lock went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LockStart {
    Locked {
        ends_at_ms: u64,
    },
    /// Accessibility isn't allowed, or macOS refused the tap.
    NotAllowed,
    /// A lock or a shortcut recording already has the keyboard.
    Busy,
    /// Only macOS has the event tap.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported,
}

/// Whether `seconds` is one of the lengths offered.
pub fn valid_duration(seconds: u64) -> bool {
    DURATIONS.contains(&seconds)
}

/// When a lock of `seconds` started at `now_ms` ends.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const fn ends_at(now_ms: u64, seconds: u64) -> u64 {
    now_ms.saturating_add(seconds.saturating_mul(1_000))
}

/// Locks the keyboard for `seconds` (one of `DURATIONS`, checked by the command) and waits
/// until it is locked.
#[cfg(target_os = "macos")]
pub fn start(app: tauri::AppHandle, seconds: u64) -> LockStart {
    if !super::tap::allowed() {
        return LockStart::NotAllowed;
    }
    if active() || super::recorder::active() {
        return LockStart::Busy;
    }
    let number = CURRENT.fetch_add(1, Ordering::Relaxed) + 1;
    let started = macos::run(app, number, Duration::from_secs(seconds));
    if !matches!(started, LockStart::Locked { .. }) {
        // Not locked in time: a tap that comes up late lets go straight away.
        let _ = end(number);
    }
    started
}

#[cfg(not(target_os = "macos"))]
pub fn start(_app: tauri::AppHandle, _seconds: u64) -> LockStart {
    LockStart::Unsupported
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::ptr::NonNull;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRetained, CFRunLoop};
    use objc2_core_graphics::{
        CGEvent, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
        CGEventType,
    };
    use tauri::{AppHandle, Emitter};

    use super::{
        end, ends_at, LockEvent, LockStart, Unlock, CURRENT, ENDS_AT_MS, LOCK_EVENT, RUNNING,
    };
    use crate::mouse::recorder;

    /// How often the lock's thread checks whether it should end.
    const CHECK_SECONDS: f64 = 0.1;
    /// How long `run` waits for macOS to accept or refuse the tap.
    const READY: Duration = Duration::from_secs(1);
    /// `NSEventTypeSystemDefined`: the media, brightness and volume keys.
    const SYSTEM_DEFINED: CGEventType = CGEventType(14);

    struct Context {
        number: u64,
        port: std::cell::RefCell<Option<CFRetained<CFMachPort>>>,
        /// Set when a click ended the lock, so the thread reports why.
        clicked: std::cell::Cell<bool>,
    }

    const fn bit(kind: CGEventType) -> u64 {
        1 << kind.0
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }

    /// Locks the keyboard on a new thread for lock `number`; `Locked` once it is.
    pub fn run(app: AppHandle, number: u64, length: Duration) -> LockStart {
        let (ready, installed) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("kleari-keyboard-lock".to_owned())
            .spawn(move || lock(&app, number, length, &ready));
        if spawned.is_err() {
            return LockStart::NotAllowed;
        }
        installed
            .recv_timeout(READY)
            .unwrap_or(LockStart::NotAllowed)
    }

    fn lock(app: &AppHandle, number: u64, length: Duration, ready: &mpsc::Sender<LockStart>) {
        let context = Box::into_raw(Box::new(Context {
            number,
            port: std::cell::RefCell::new(None),
            clicked: std::cell::Cell::new(false),
        }));
        let mask = bit(CGEventType::KeyDown)
            | bit(CGEventType::KeyUp)
            | bit(CGEventType::FlagsChanged)
            | bit(SYSTEM_DEFINED)
            | bit(CGEventType::LeftMouseDown)
            | bit(CGEventType::RightMouseDown)
            | bit(CGEventType::OtherMouseDown);
        // SAFETY: `callback` matches the tap callback type and only reads `context`, which lives
        // until after the tap is invalidated below.
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(callback),
                context.cast(),
            )
        };
        let mut outcome = LockStart::NotAllowed;
        let mut reason = Unlock::Stopped;
        let mut locked = false;
        let port_missing = port.is_none();
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
                // `start` gave up waiting, or the lock was already ended: never take the
                // keyboard or touch the system shortcuts.
                if CURRENT.load(Ordering::Relaxed) == number {
                    match recorder::hold_system_shortcuts(app) {
                        None => {
                            crate::log::note(
                                "keylock",
                                "not started: a recording holds the shortcuts",
                            );
                            outcome = LockStart::Busy;
                        }
                        Some(_held) => {
                            crate::log::note_number(
                                "keylock",
                                "keyboard locked, seconds",
                                i64::try_from(length.as_secs()).unwrap_or(i64::MAX),
                            );
                            let ends_at_ms = ends_at(now_ms(), length.as_secs());
                            ENDS_AT_MS.store(ends_at_ms, Ordering::Relaxed);
                            CGEvent::tap_enable(&port, true);
                            RUNNING.store(number, Ordering::Relaxed);
                            locked = true;
                            let _ = app.emit(LOCK_EVENT, LockEvent::Locked { ends_at_ms });
                            // `run` may have stopped waiting.
                            let _ = ready.send(LockStart::Locked { ends_at_ms });
                            let began = Instant::now();
                            while CURRENT.load(Ordering::Relaxed) == number {
                                if began.elapsed() >= length {
                                    reason = Unlock::Time;
                                    let _ = end(number);
                                    break;
                                }
                                CFRunLoop::run_in_mode(mode, CHECK_SECONDS, false);
                            }
                            CGEvent::tap_enable(&port, false);
                            // SAFETY: the tap is off; the callback ran only on this thread.
                            if unsafe { &*context }.clicked.get() {
                                reason = Unlock::Click;
                            }
                            // `_held` drops here: the system shortcuts are back on.
                        }
                    }
                }
            }
            port.invalidate();
        }
        if !locked {
            if port_missing {
                crate::log::note("keylock", "macOS refused the keyboard tap");
            }
            let _ = ready.send(outcome);
        }
        ENDS_AT_MS.store(0, Ordering::Relaxed);
        // Only this lock's own mark is cleared.
        let _ = RUNNING.compare_exchange(number, 0, Ordering::Relaxed, Ordering::Relaxed);
        // SAFETY: the tap is invalidated (or was never made), so the callback can't run again.
        drop(unsafe { Box::from_raw(context) });
        if locked {
            crate::log::note(
                "keylock",
                match reason {
                    Unlock::Click => "keyboard unlocked: click",
                    Unlock::Time => "keyboard unlocked: time ran out",
                    Unlock::Stopped => "keyboard unlocked: stopped (quit or replaced)",
                },
            );
            let _ = app.emit(LOCK_EVENT, LockEvent::Unlocked { reason });
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
        // SAFETY: `user` is the `Context` `lock` made, alive while the tap is.
        let context = unsafe { &*user.cast::<Context>() };
        if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput
        {
            if let Some(port) = context.port.borrow().as_ref() {
                CGEvent::tap_enable(port, true);
            }
            return pass;
        }
        // A lock that has ended lets everything through.
        if CURRENT.load(Ordering::Relaxed) != context.number {
            return pass;
        }
        let swallow = catch_unwind(AssertUnwindSafe(|| {
            if kind == CGEventType::LeftMouseDown
                || kind == CGEventType::RightMouseDown
                || kind == CGEventType::OtherMouseDown
            {
                // The unlocking click presses nothing.
                context.clicked.set(true);
                let _ = end(context.number);
            }
            true
        }));
        // A failure here lets the event through rather than keep it.
        if swallow.unwrap_or(false) {
            std::ptr::null_mut()
        } else {
            pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_offered_lengths_are_accepted() {
        for seconds in [30, 60, 120, 300] {
            assert!(valid_duration(seconds));
        }
        for seconds in [0, 1, 59, 301, 3_600, u64::MAX] {
            assert!(!valid_duration(seconds), "{seconds}");
        }
    }

    #[test]
    fn a_lock_ends_its_length_after_it_starts() {
        assert_eq!(ends_at(1_000, 30), 31_000);
        assert_eq!(ends_at(u64::MAX - 5, 300), u64::MAX);
    }

    #[test]
    fn an_end_or_stop_never_ends_a_newer_lock() {
        // Numbers far from any other test's; both set before each check.
        CURRENT.store(5_003, Ordering::Relaxed);
        RUNNING.store(5_003, Ordering::Relaxed);
        assert!(active());
        assert!(!end(5_002), "an older lock's end changes nothing");
        assert!(active());
        stop();
        assert!(!active(), "stopped: the keyboard works again");
        assert!(!end(5_003), "ending it twice changes nothing more");
        RUNNING.store(0, Ordering::Relaxed);
    }

    #[test]
    fn the_page_is_told_the_state_in_tagged_form() {
        let locked =
            serde_json::to_string(&LockEvent::Locked { ends_at_ms: 9 }).unwrap_or_default();
        assert_eq!(locked, r#"{"state":"locked","endsAtMs":9}"#);
        let unlocked = serde_json::to_string(&LockEvent::Unlocked {
            reason: Unlock::Click,
        })
        .unwrap_or_default();
        assert_eq!(unlocked, r#"{"state":"unlocked","reason":"click"}"#);
        let start = serde_json::to_string(&LockStart::Busy).unwrap_or_default();
        assert_eq!(start, r#"{"kind":"busy"}"#);
        assert_eq!(
            serde_json::to_string(&LockState::Unlocked).unwrap_or_default(),
            r#"{"state":"unlocked"}"#
        );
    }
}
