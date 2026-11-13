//! The Mouse page's button finder: while the page is open it is told which mouse button was
//! pressed and how the wheel scrolled, so the user can see which button is which before
//! setting them up. It uses an in-app event monitor (`NSEvent` local monitor), so it sees only
//! clicks and scrolls inside Kleari's own windows, needs no permission, and never sees
//! keystrokes. Nothing is stored; events go only to the page.
//!
//! While the pointer is over the finder's box, the page says it is testing: then the middle
//! and side buttons, the wheel and right click only light up the drawing (the monitor drops
//! them before Kleari's window acts on them), and the event tap leaves the mouse's real
//! buttons alone so the drawing shows them. Left click works as usual. Testing lapses on its
//! own `CAPTURE_MS` after the page last renewed it, so the mouse can't stay paused.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use serde::Serialize;

// The event, `Probe` and `direction` serve the macOS monitor; other targets (Linux CI) only
// test them.

/// Sent to the page for each press, release or scroll while it listens.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const PROBE_EVENT: &str = "mouse-probe";

/// Whether the page is listening; the monitor reports nothing otherwise.
static LISTENING: AtomicBool = AtomicBool::new(false);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Probe {
    /// A button, numbered as macOS does (0 left, 1 right, 2 middle, then 3, 4…).
    Down {
        button: u8,
    },
    Up {
        button: u8,
    },
    /// Which way the wheel physically turned (up/left +1, down/right −1, or 0), and whether
    /// it scrolls in steps (a mouse wheel) rather than smoothly (a trackpad or Magic Mouse).
    Scroll {
        vertical: i8,
        horizontal: i8,
        stepped: bool,
    },
}

/// The direction of one scroll delta, as the hand moved: `inverted` is macOS's natural
/// scrolling having flipped it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn direction(delta: f64, inverted: bool) -> i8 {
    let physical = if inverted { -delta } else { delta };
    if physical > 0.0 {
        1
    } else if physical < 0.0 {
        -1
    } else {
        0
    }
}

/// How long one "testing" from the page lasts; the page renews it every second while the
/// pointer stays over the box.
const CAPTURE_MS: u64 = 2_000;

/// Until when (Unix ms) the pointer is over the finder's box; 0 when it isn't.
static CAPTURE_UNTIL: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    crate::history::unix_time_ms()
}

pub fn listen(on: bool) {
    LISTENING.store(on, Ordering::Relaxed);
    if !on {
        CAPTURE_UNTIL.store(0, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn listening() -> bool {
    LISTENING.load(Ordering::Relaxed)
}

/// The pointer is over the finder's box (`true`, renewed every second) or has left it.
/// Ignored unless the page is listening.
pub fn capture(on: bool) {
    let until = if on && listening() {
        now_ms() + CAPTURE_MS
    } else {
        0
    };
    CAPTURE_UNTIL.store(until, Ordering::Relaxed);
}

/// Whether testing is still current at `now`: the page renewed it less than `CAPTURE_MS` ago.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const fn current(until: u64, now: u64) -> bool {
    now < until
}

/// Buttons are being tested in the finder's box now.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn capturing() -> bool {
    current(CAPTURE_UNTIL.load(Ordering::Relaxed), now_ms())
}

/// Buttons (as macOS numbers them, below 32) whose press Kleari's window dropped while
/// testing and whose release hasn't come yet. Only the in-app monitor sets one, so a press that
/// reached another app is never here; the release is then dropped too, wherever it arrives, to
/// keep press and release paired. A new press of the button clears it.
static DROPPED: AtomicU32 = AtomicU32::new(0);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const fn bit(button: u8) -> u32 {
    if button < 32 {
        1 << button
    } else {
        0
    }
}

/// The press of `button` was dropped in Kleari's window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn dropped_press(button: u8) {
    DROPPED.fetch_or(bit(button), Ordering::Relaxed);
}

/// Whether `button`'s press was dropped and its release hasn't come yet.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn press_dropped(button: u8) -> bool {
    bit(button) != 0 && DROPPED.load(Ordering::Relaxed) & bit(button) != 0
}

/// `button` was released (or pressed again): its dropped press, if any, is matched.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn clear_press(button: u8) {
    DROPPED.fetch_and(!bit(button), Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
    use tauri::{AppHandle, Emitter};

    use super::{
        capturing, clear_press, direction, dropped_press, listening, press_dropped, Probe,
        PROBE_EVENT,
    };

    /// While testing, these only light up the drawing; left click (choosing a part) still works.
    fn tested_only(kind: NSEventType) -> bool {
        [
            NSEventType::RightMouseDown,
            NSEventType::RightMouseUp,
            NSEventType::RightMouseDragged,
            NSEventType::OtherMouseDown,
            NSEventType::OtherMouseUp,
            NSEventType::OtherMouseDragged,
            NSEventType::ScrollWheel,
        ]
        .contains(&kind)
    }

    fn probe(event: &NSEvent) -> Option<Probe> {
        let button = || u8::try_from(event.buttonNumber()).ok();
        let kind = event.r#type();
        if kind == NSEventType::ScrollWheel {
            let inverted = event.isDirectionInvertedFromDevice();
            return Some(Probe::Scroll {
                vertical: direction(event.scrollingDeltaY(), inverted),
                horizontal: direction(event.scrollingDeltaX(), inverted),
                stepped: !event.hasPreciseScrollingDeltas(),
            });
        }
        if [
            NSEventType::LeftMouseDown,
            NSEventType::RightMouseDown,
            NSEventType::OtherMouseDown,
        ]
        .contains(&kind)
        {
            return button().map(|button| Probe::Down { button });
        }
        if [
            NSEventType::LeftMouseUp,
            NSEventType::RightMouseUp,
            NSEventType::OtherMouseUp,
        ]
        .contains(&kind)
        {
            return button().map(|button| Probe::Up { button });
        }
        None
    }

    /// Installs the in-app monitor for as long as Kleari runs. Call on the main thread.
    pub fn install(app: AppHandle) {
        let mask = NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::OtherMouseDragged
            | NSEventMask::ScrollWheel;
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a valid event for the duration of the call.
            let event_ref = unsafe { event.as_ref() };
            let kind = event_ref.r#type();
            let button = u8::try_from(event_ref.buttonNumber()).ok();
            let press = [NSEventType::RightMouseDown, NSEventType::OtherMouseDown].contains(&kind);
            let release = [NSEventType::RightMouseUp, NSEventType::OtherMouseUp].contains(&kind);
            let drag = [
                NSEventType::RightMouseDragged,
                NSEventType::OtherMouseDragged,
            ]
            .contains(&kind);
            let testing = listening() && capturing();
            if let Some(button) = button {
                if press {
                    // A new press starts afresh.
                    clear_press(button);
                } else if (release || drag) && !testing && press_dropped(button) {
                    // Its press was dropped while testing: drop its drag and release too.
                    if release {
                        clear_press(button);
                    }
                    return std::ptr::null_mut();
                }
            }
            if !listening() {
                return event.as_ptr();
            }
            if let Some(probe) = probe(event_ref) {
                // A page that went away just misses it.
                let _ = app.emit(PROBE_EVENT, probe);
            }
            // Over the finder's box, a tested button or the wheel does nothing else.
            if testing && tested_only(kind) {
                if let Some(button) = button {
                    if press {
                        dropped_press(button);
                    } else if release {
                        clear_press(button);
                    }
                }
                return std::ptr::null_mut();
            }
            event.as_ptr()
        });
        // SAFETY: the handler returns the event it was given (a valid pointer) or null, which
        // AppKit takes as "drop this event".
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
        // AppKit keeps the monitor until it is removed; Kleari never removes it.
        std::mem::forget(monitor);
    }
}

/// Installs the monitor at launch; it reports only while the Mouse page listens.
#[cfg(target_os = "macos")]
pub fn install(app: tauri::AppHandle) {
    macos::install(app);
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direction_is_where_the_hand_moved() {
        assert_eq!(direction(3.0, false), 1);
        assert_eq!(direction(-0.5, false), -1);
        // Natural scrolling flips the delta; the hand still moved up.
        assert_eq!(direction(-3.0, true), 1);
        assert_eq!(direction(0.0, true), 0);
    }

    #[test]
    fn testing_lapses_unless_renewed() {
        assert!(current(2_000, 1_999));
        assert!(!current(2_000, 2_000));
        // Never set, or cleared when the pointer left.
        assert!(!current(0, 1));
    }

    #[test]
    fn only_a_dropped_press_pairs_its_release_and_a_new_press_clears_it() {
        // Buttons 20–23 so this test doesn't meet others' use of the shared record.
        assert!(!press_dropped(20));
        dropped_press(20);
        assert!(press_dropped(20));
        // Another button's record is its own.
        assert!(!press_dropped(21));
        clear_press(20);
        assert!(!press_dropped(20));
        // Beyond button 31 nothing is recorded (macOS reports fewer buttons than that).
        dropped_press(40);
        assert!(!press_dropped(40));
        assert_eq!(bit(31), 1 << 31);
    }

    #[test]
    fn probes_are_tagged_for_the_page() {
        let text = serde_json::to_string(&Probe::Scroll {
            vertical: 1,
            horizontal: 0,
            stepped: true,
        })
        .unwrap_or_default();
        assert_eq!(
            text,
            r#"{"kind":"scroll","vertical":1,"horizontal":0,"stepped":true}"#
        );
        let down = serde_json::to_string(&Probe::Down { button: 3 }).unwrap_or_default();
        assert_eq!(down, r#"{"kind":"down","button":3}"#);
    }
}
