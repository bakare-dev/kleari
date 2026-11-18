//! Recording a keyboard shortcut for a mouse button, system shortcuts included (⌘⇥, ⌘Space,
//! ⌃↑…), which macOS would otherwise act on before any app sees them.
//!
//! While recording:
//! - macOS's own system shortcuts (`SYSTEM_SHORTCUTS`: the app switcher, Spotlight, Mission
//!   Control, Spaces, screenshots…) are turned off, so their keys arrive as ordinary key
//!   presses. Only those that were on are turned off, and exactly those are turned back on
//!   when recording ends, however it ends. The list is saved first, so if Kleari quits or
//!   crashes mid-recording they are turned back on at its next launch (`restore_after_crash`).
//!   This uses a private macOS call (`CGSSetSymbolicHotKeyEnabled`, as other app switchers
//!   do): it needs no permission, but a future macOS could change it.
//! - a keyboard event tap at the session level swallows every key press and release, and
//!   the mouse settings pause (`active`). Each press is reported to the page as the shortcut
//!   it makes, or as one Kleari can't send. Nothing is stored here but the list above; the
//!   page saves the shortcut once the user confirms it.
//!
//! Recording ends when the page stops it (confirm, cancel, leaving the window or the page),
//! on Escape alone, or after `MAX_RECORDING` at most, so the keyboard can never stay captured.
//! Each recording has its own number: a stop for an older one never ends a newer one. It
//! needs Accessibility, as the mouse settings do; without it `start` says so and the page
//! records inside its own window instead (where system shortcuts can't be caught).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

use super::config::{Modifiers, Shortcut, KEYS};
use crate::persistence::Database;

/// Sent to the page for each key press while recording, and when recording ends by itself.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const RECORD_EVENT: &str = "mouse-shortcut-record";

/// Recording ends by itself after this long.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAX_RECORDING: Duration = Duration::from_secs(30);

/// Where the system shortcuts turned off for a recording are listed until they are back on.
const SUSPENDED_KEY: &str = "mouse.recorder.suspendedShortcuts";

/// macOS's system shortcuts turned off while recording, by their number in macOS's
/// `com.apple.symbolichotkeys` list: ⌘⇥ and ⌘⇧⇥ (1, 2), ⌘` (27), the screenshot shortcuts
/// (28–31, 184), Mission Control and App windows (32, 33), Show desktop (36), the previous and
/// next Space (79, 81), Spotlight and its Finder search (64, 65), and the input-source switch
/// (60, 61).
///
/// Checked on macOS 27 with `CGSGetSymbolicHotKeyValue`, which reported each number's default
/// key: 1 ⌘⇥, 2 ⌘⇧⇥, 27 ⌘`, 28 ⌘⇧3, 29 ⌃⌘⇧3, 30 ⌘⇧4, 31 ⌃⌘⇧4, 32 ⌃↑, 33 ⌃↓, 36 F11, 60 ⌃Space,
/// 61 ⌃⌥Space, 64 ⌘Space, 65 ⌘⌥Space, 79 ⌃←, 81 ⌃→, 184 ⌘⇧5. (6, once listed here, is ⌘⌥⇧⎋,
/// Force Quit, and is never touched.) A number names the system action, so one the user has
/// moved to another key is still turned off, and that key can then be recorded too.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SYSTEM_SHORTCUTS: [i32; 17] = [
    1, 2, 27, 28, 29, 30, 31, 32, 33, 36, 60, 61, 64, 65, 79, 81, 184,
];

/// Each of `SYSTEM_SHORTCUTS` with the key and modifiers macOS gives it by default (as
/// `CGSGetSymbolicHotKeyValue` reports them: a virtual key code, and ⌘ ⇧ ⌥ ⌃ as
/// `MODIFIER_BITS`). A number is turned off only while macOS still binds it to exactly this,
/// so on a macOS that numbers its shortcuts differently, a number now meaning something else
/// is never touched. The cost: a shortcut the user moved to another key stays on, so that key
/// can't be recorded.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const EXPECTED_KEYS: [(i32, u16, u32); 17] = [
    (1, 48, CMD),
    (2, 48, CMD | SHIFT),
    (27, 50, CMD),
    (28, 20, CMD | SHIFT),
    (29, 20, CMD | SHIFT | CTRL),
    (30, 21, CMD | SHIFT),
    (31, 21, CMD | SHIFT | CTRL),
    (32, 126, CTRL),
    (33, 125, CTRL),
    (36, 103, 0),
    (60, 49, CTRL),
    (61, 49, OPT | CTRL),
    (64, 49, CMD),
    (65, 49, CMD | OPT),
    (79, 123, CTRL),
    (81, 124, CTRL),
    (184, 23, CMD | SHIFT),
];
const CMD: u32 = 0x10_0000;
const SHIFT: u32 = 0x2_0000;
const OPT: u32 = 0x8_0000;
const CTRL: u32 = 0x4_0000;
/// The modifier bits compared; others (Fn, the keypad flag arrows carry) are ignored.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MODIFIER_BITS: u32 = CMD | SHIFT | OPT | CTRL;

/// Whether macOS binding `number` to `key_code` with `modifiers` is the binding Kleari
/// expects for it, so it is safe to turn off.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bound_as_expected(number: i32, key_code: u16, modifiers: u32) -> bool {
    EXPECTED_KEYS.iter().any(|&(expected, code, mods)| {
        expected == number && code == key_code && mods == modifiers & MODIFIER_BITS
    })
}

/// The recording running now; each start and each end moves it on, so an older recording's
/// thread sees it has been replaced and lets the keyboard go.
static CURRENT: AtomicU64 = AtomicU64::new(0);
/// The recording whose keyboard tap is installed, or 0.
static RUNNING: AtomicU64 = AtomicU64::new(0);

/// A shortcut is being recorded: the mouse settings pass every event untouched meanwhile.
pub fn active() -> bool {
    let running = RUNNING.load(Ordering::Relaxed);
    running != 0 && running == CURRENT.load(Ordering::Relaxed)
}

/// Ends recording `number` if it is the current one; an older (or unknown) number changes
/// nothing, so a late stop from the page can't end a recording started after it.
pub fn stop(number: u64) {
    let _ = CURRENT.compare_exchange(number, number + 1, Ordering::Relaxed, Ordering::Relaxed);
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

/// The shortcut a key press makes: the key's name from `KEYS`, or `Unsupported`.
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

/// The suspended shortcuts as saved: only known numbers, each once; anything else is ignored
/// (a shortcut Kleari never turned off is never turned on by it).
fn parse_suspended(text: &str) -> Vec<i32> {
    let mut numbers: Vec<i32> = serde_json::from_str::<Vec<i32>>(text)
        .unwrap_or_default()
        .into_iter()
        .filter(|number| SYSTEM_SHORTCUTS.contains(number))
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// What `SUSPENDED_KEY` holds now.
fn saved_suspended(db: &Database) -> Vec<i32> {
    db.settings()
        .ok()
        .and_then(|rows| rows.into_iter().find(|(key, _)| key == SUSPENDED_KEY))
        .map(|(_, text)| parse_suspended(&text))
        .unwrap_or_default()
}

/// Records which shortcuts are off; `false` if that couldn't be saved.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn save_suspended(db: &Database, numbers: &[i32]) -> bool {
    serde_json::to_string(numbers)
        .ok()
        .is_some_and(|text| db.set_setting(SUSPENDED_KEY, &text).is_ok())
}

/// At launch: turns back on any system shortcut a recording turned off and never got to turn
/// back on (Kleari quit or crashed mid-recording, or macOS refused last time).
pub fn restore_after_crash(db: &Database) {
    #[cfg(target_os = "macos")]
    restore_with(db, |numbers| macos::set_enabled(numbers, true));
    #[cfg(not(target_os = "macos"))]
    restore_with(db, |_| Vec::new());
}

/// `restore_after_crash`, turning them on with `turn_on` (so tests never touch macOS).
fn restore_with(db: &Database, turn_on: impl FnOnce(&[i32]) -> Vec<i32>) {
    let left_off = saved_suspended(db);
    if !left_off.is_empty() {
        crate::log::note_number(
            "shortcuts",
            "turning back on system shortcuts an earlier run left off",
            i64::try_from(left_off.len()).unwrap_or(i64::MAX),
        );
        restore_list(db, &left_off, turn_on);
    }
}

/// Whether a shortcut read back as `now` failed to become `wanted`. Only the read-back state
/// counts, never the call's return code.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const fn missed(now: bool, wanted: bool) -> bool {
    now != wanted
}

/// Turns `numbers` back on with `turn_on`, which returns those macOS didn't turn on; only
/// those stay listed, so each launch tries them again until they are on. If the list can't be
/// saved, it keeps what it held before, which is only ever more to turn on: never fewer.
fn restore_list(db: &Database, numbers: &[i32], turn_on: impl FnOnce(&[i32]) -> Vec<i32>) {
    let still_off = turn_on(numbers);
    let count = |list: usize| i64::try_from(list).unwrap_or(i64::MAX);
    crate::log::note_number(
        "shortcuts",
        "system shortcuts turned back on",
        count(numbers.len() - still_off.len().min(numbers.len())),
    );
    if !still_off.is_empty() {
        crate::log::note_number(
            "shortcuts",
            "system shortcuts macOS kept off; tried again at the next launch",
            count(still_off.len()),
        );
    }
    let _ = save_suspended(db, &still_off);
}

/// Starts recording (ending any recording before it) and waits until the keyboard is
/// captured; returns the recording's number, for `stop`.
#[cfg(target_os = "macos")]
pub fn start(app: tauri::AppHandle) -> Result<u64, StartError> {
    if !super::tap::allowed() {
        return Err(StartError::NotAllowed);
    }
    let number = CURRENT.fetch_add(1, Ordering::Relaxed) + 1;
    if macos::run(app, number) {
        Ok(number)
    } else {
        // Not captured in time: this recording is over, so a tap that comes up late lets go
        // straight away instead of swallowing keys the page isn't listening for.
        stop(number);
        Err(StartError::NotAllowed)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn start(_app: tauri::AppHandle) -> Result<u64, StartError> {
    Err(StartError::Unsupported)
}

/// The keyboard lock turns the same system shortcuts off, one holder at a time.
#[cfg(target_os = "macos")]
pub(super) use macos::hold as hold_system_shortcuts;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::ptr::NonNull;
    use std::sync::atomic::Ordering;
    use std::sync::{mpsc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRetained, CFRunLoop};
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventTapProxy, CGEventType,
    };
    use tauri::{AppHandle, Emitter, Manager};

    use super::{
        bound_as_expected, missed, recorded, restore_list, save_suspended, stop, Recorded, CURRENT,
        MAX_RECORDING, RECORD_EVENT, RUNNING, SYSTEM_SHORTCUTS,
    };
    use crate::history::History;
    use crate::mouse::config::Modifiers;

    /// How often the recording's thread checks whether it should end.
    const CHECK_SECONDS: f64 = 0.1;
    /// How long `run` waits for macOS to accept or refuse the tap.
    const READY: Duration = Duration::from_secs(1);
    /// `kVK_Escape`.
    const ESCAPE: u16 = 53;

    /// Held for a whole recording, so a new one starts only once the one before has turned the
    /// system shortcuts back on (otherwise it would find them off, and they'd be turned on
    /// under it).
    static SESSION: Mutex<()> = Mutex::new(());

    // Private CoreGraphics calls (stable since macOS 10.x, used by app switchers such as
    // AltTab) that turn one of macOS's system shortcuts off or on for this login session.
    extern "C" {
        fn CGSIsSymbolicHotKeyEnabled(hot_key: i32) -> bool;
        fn CGSSetSymbolicHotKeyEnabled(hot_key: i32, enabled: bool) -> i32;
        fn CGSGetSymbolicHotKeyValue(
            hot_key: i32,
            key_equivalent: *mut u16,
            virtual_key_code: *mut u16,
            modifiers: *mut u32,
        ) -> i32;
    }

    /// Whether macOS binds `number` to the key Kleari expects (`bound_as_expected`).
    fn expected_binding(number: i32) -> bool {
        let (mut character, mut code, mut modifiers) = (0_u16, 0_u16, 0_u32);
        // SAFETY: the out pointers are to locals of the types the call writes.
        let error =
            unsafe { CGSGetSymbolicHotKeyValue(number, &mut character, &mut code, &mut modifiers) };
        error == 0 && bound_as_expected(number, code, modifiers)
    }

    /// Turns each of `numbers` on or off; returns those that didn't end up that way. The state
    /// macOS reads back afterwards decides, whatever the call returned: an error may still have
    /// changed it, and success may not have.
    pub fn set_enabled(numbers: &[i32], enabled: bool) -> Vec<i32> {
        numbers
            .iter()
            .copied()
            .filter(|&number| {
                // SAFETY: plain values; an unknown number is refused with an error code. The
                // code itself isn't needed: the read-back below says where it ended up.
                let _ = unsafe { CGSSetSymbolicHotKeyEnabled(number, enabled) };
                // SAFETY: a plain value.
                missed(unsafe { CGSIsSymbolicHotKeyEnabled(number) }, enabled)
            })
            .collect()
    }

    /// Turns off the system shortcuts that are on, saving the list first; returns it.
    fn suspend(app: &AppHandle) -> Vec<i32> {
        let on: Vec<i32> = SYSTEM_SHORTCUTS
            .iter()
            .copied()
            // On, and still bound to the key it has by default: a number that means something
            // else on this macOS, or a shortcut the user moved, is left alone.
            // SAFETY: a plain value.
            .filter(|&number| unsafe { CGSIsSymbolicHotKeyEnabled(number) })
            .filter(|&number| expected_binding(number))
            .collect();
        let db = &app.state::<History>().db;
        // Unsaved, a crash could leave them off: then they stay on, and only shortcuts macOS
        // doesn't act on first can be recorded.
        if on.is_empty() {
            return Vec::new();
        }
        if !save_suspended(db, &on) {
            crate::log::note(
                "shortcuts",
                "list couldn't be saved; system shortcuts left on",
            );
            return Vec::new();
        }
        let still_on = set_enabled(&on, false);
        let count = |list: usize| i64::try_from(list).unwrap_or(i64::MAX);
        crate::log::note_number(
            "shortcuts",
            "system shortcuts turned off",
            count(on.len() - still_on.len()),
        );
        if !still_on.is_empty() {
            crate::log::note_number(
                "shortcuts",
                "system shortcuts macOS kept on",
                count(still_on.len()),
            );
        }
        // Those macOS kept on needn't be turned back on; the list says exactly what is off.
        let off: Vec<i32> = on
            .into_iter()
            .filter(|number| !still_on.contains(number))
            .collect();
        if !still_on.is_empty() {
            // Unsaved, the list still names more than is off: turning those on changes nothing.
            let _ = save_suspended(db, &off);
        }
        off
    }

    /// Turns back on exactly what `suspend` turned off, and keeps listed only what macOS
    /// wouldn't turn on (tried again at the next launch). Runs when its guard is dropped, so a
    /// panic on the recording's thread still restores them.
    struct Suspended {
        app: AppHandle,
        numbers: Vec<i32>,
    }

    impl Drop for Suspended {
        fn drop(&mut self) {
            if self.numbers.is_empty() {
                return;
            }
            let db = &self.app.state::<History>().db;
            restore_list(db, &self.numbers, |numbers| set_enabled(numbers, true));
        }
    }

    /// macOS's system shortcuts held off for the keyboard lock (`super::super::keylock`): they
    /// are turned back on when this drops, before a recording can start again.
    pub struct Held {
        // Dropped in this order: the shortcuts come back on, then a recording may start.
        _suspended: Suspended,
        _session: std::sync::MutexGuard<'static, ()>,
    }

    /// Turns the system shortcuts off for the keyboard lock, as a recording does (saved first,
    /// so a crash restores them at the next launch); `None` while a recording has them.
    pub fn hold(app: &AppHandle) -> Option<Held> {
        let session = match SESSION.try_lock() {
            Ok(session) => session,
            // Only `()` inside: a panic in another holder leaves nothing half-done here.
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(Held {
            _suspended: Suspended {
                numbers: suspend(app),
                app: app.clone(),
            },
            _session: session,
        })
    }

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
        // Only `()` inside: a panic in another recording leaves nothing half-done here.
        let _session = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
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
                CGEventTapLocation::SessionEventTap,
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
                // `start` gave up waiting, or the page already stopped this recording: never
                // capture the keyboard or touch the system shortcuts.
                if CURRENT.load(Ordering::Relaxed) == number {
                    let _suspended = Suspended {
                        numbers: suspend(&app),
                        app: app.clone(),
                    };
                    CGEvent::tap_enable(&port, true);
                    captured = true;
                    crate::log::note("recorder", "shortcut recording started");
                    RUNNING.store(number, Ordering::Relaxed);
                    // `run` may have stopped waiting.
                    let _ = ready.send(true);
                    let began = Instant::now();
                    while CURRENT.load(Ordering::Relaxed) == number {
                        if began.elapsed() >= MAX_RECORDING {
                            timed_out = true;
                            stop(number);
                            break;
                        }
                        CFRunLoop::run_in_mode(mode, CHECK_SECONDS, false);
                    }
                    CGEvent::tap_enable(&port, false);
                    // `_suspended` drops here: the system shortcuts are back on.
                }
            }
            port.invalidate();
        }
        if captured {
            crate::log::note(
                "recorder",
                if timed_out {
                    "shortcut recording ended: time ran out"
                } else {
                    "shortcut recording ended"
                },
            );
        } else {
            crate::log::note(
                "recorder",
                "shortcut recording couldn't capture the keyboard",
            );
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
            stop(context.number);
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

    // The two tests below share `CURRENT`/`RUNNING`; they use numbers far apart and set both
    // before each check, so running in parallel can't change what they see.
    #[test]
    fn a_stop_ends_only_its_own_recording() {
        CURRENT.store(1_007, Ordering::Relaxed);
        RUNNING.store(1_007, Ordering::Relaxed);
        assert!(active());
        // A late stop for an earlier recording (the page's cleanup arriving after a new start)
        // changes nothing.
        stop(1_006);
        assert!(active(), "a stale stop must not end the newer recording");
        stop(1_007);
        assert!(!active(), "stopped: the mouse settings work again");
        // Stopping it again, or a recording that never existed, changes nothing more.
        let after = CURRENT.load(Ordering::Relaxed);
        stop(1_007);
        stop(999_999);
        assert_eq!(CURRENT.load(Ordering::Relaxed), after);
    }

    #[test]
    fn suspended_shortcuts_are_read_back_only_if_kleari_could_have_turned_them_off() {
        assert_eq!(parse_suspended("[64,1,1,2]"), vec![1, 2, 64]);
        // Numbers outside the list are never turned on by Kleari.
        assert_eq!(parse_suspended("[1,9999,-3]"), vec![1]);
        assert!(parse_suspended("not json").is_empty());
        assert!(parse_suspended("[]").is_empty());
    }

    #[test]
    fn a_saved_list_is_cleared_on_the_next_launch() {
        let db = Database::in_memory();
        assert!(save_suspended(&db, &[1, 2]));
        assert_eq!(saved_suspended(&db), vec![1, 2]);
        let mut turned_on = Vec::new();
        restore_with(&db, |numbers| {
            turned_on.extend_from_slice(numbers);
            Vec::new()
        });
        assert_eq!(turned_on, vec![1, 2]);
        assert!(saved_suspended(&db).is_empty());
        // Nothing saved: nothing is turned on.
        restore_with(&db, |_| panic!("nothing to turn on"));
    }

    #[test]
    fn a_shortcut_macos_wouldnt_turn_back_on_stays_listed_for_the_next_launch() {
        let db = Database::in_memory();
        // macOS turns 64 back on but refuses 1 and 32.
        restore_list(&db, &[1, 32, 64], |_| vec![1, 32]);
        assert_eq!(saved_suspended(&db), vec![1, 32]);
        // At the next launch it is tried again, and this time works: the list is cleared.
        let mut tried = Vec::new();
        restore_with(&db, |numbers| {
            tried.extend_from_slice(numbers);
            Vec::new()
        });
        assert_eq!(tried, vec![1, 32]);
        assert!(saved_suspended(&db).is_empty());
    }

    #[test]
    fn only_the_state_read_back_decides_whether_a_shortcut_changed() {
        // Turned off as asked (whatever the call returned): not listed as still on.
        assert!(!missed(false, false));
        // Still on after asking for off: kept out of the list of what is off.
        assert!(missed(true, false));
        // Turning back on: on is done, off stays listed for the next launch.
        assert!(!missed(true, true));
        assert!(missed(false, true));
    }

    #[test]
    fn only_a_shortcut_bound_as_expected_is_turned_off() {
        // ⌘⇥ as macOS binds it by default (Fn and keypad bits are ignored).
        assert!(bound_as_expected(1, 48, CMD));
        assert!(bound_as_expected(32, 126, CTRL | 0x80_0000));
        // The user moved Spotlight to ⌥Space: left on.
        assert!(!bound_as_expected(64, 49, OPT));
        // A number this list doesn't know, or one bound to another key on some macOS.
        assert!(!bound_as_expected(6, 53, CMD | SHIFT | OPT));
        assert!(!bound_as_expected(1, 50, CMD));
        // Every listed number has exactly one expected binding.
        for number in SYSTEM_SHORTCUTS {
            let count = EXPECTED_KEYS
                .iter()
                .filter(|(n, _, _)| *n == number)
                .count();
            assert_eq!(count, 1, "number {number}");
        }
    }

    #[test]
    fn what_the_page_is_sent_is_tagged() {
        let text = serde_json::to_string(&Recorded::Ended).unwrap_or_default();
        assert_eq!(text, r#"{"kind":"ended"}"#);
        let unsupported = serde_json::to_string(&Recorded::Unsupported).unwrap_or_default();
        assert_eq!(unsupported, r#"{"kind":"unsupported"}"#);
    }
}
