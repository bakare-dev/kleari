//! The mouse actions that work on windows and apps rather than sending a shortcut: minimise,
//! full screen, close and tile the focused window (through Accessibility, which the mouse
//! settings already need), and hide the frontmost app. A shortcut would mean something else
//! in some apps (⌘W closes a tab in a browser, not the window).
//!
//! They run on a worker thread, never the event tap's: another app may take a while to answer,
//! and a slow tap is turned off by macOS. Each call to an app is limited to `AX_TIMEOUT`.

use super::engine::WindowAction;

/// A rectangle in Accessibility's coordinates: from the top left of the main screen, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A screen as AppKit gives it: its whole frame and the part not under the menu bar or Dock,
/// both from the bottom left of the main screen, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub visible: Rect,
}

/// An AppKit rectangle (y up from the main screen's bottom) in Accessibility's coordinates.
fn flipped(rect: Rect, main_height: f64) -> Rect {
    Rect {
        y: main_height - rect.y - rect.height,
        ..rect
    }
}

/// Where a window tiled to the left or right half of its screen goes: the half of the usable
/// area of the screen its centre is on (or the first screen, if it is off all of them).
/// `screens[0]` is the main screen, whose height sets the flip. `None` without screens.
pub fn tiled(window: Rect, screens: &[Screen], left: bool) -> Option<Rect> {
    let main_height = screens.first()?.frame.height;
    let (centre_x, centre_y) = (
        window.x + window.width / 2.0,
        window.y + window.height / 2.0,
    );
    let screen = screens
        .iter()
        .find(|screen| flipped(screen.frame, main_height).contains(centre_x, centre_y))
        .unwrap_or(&screens[0]);
    let usable = flipped(screen.visible, main_height);
    let half = (usable.width / 2.0).floor();
    Some(Rect {
        x: if left { usable.x } else { usable.x + half },
        y: usable.y,
        width: if left { half } else { usable.width - half },
        height: usable.height,
    })
}

/// Carries out a window action, off the caller's thread.
#[cfg(target_os = "macos")]
pub fn window(action: WindowAction) {
    macos::send(macos::Job::Window(action));
}

/// Hides the frontmost app, off the caller's thread.
#[cfg(target_os = "macos")]
pub fn hide_app() {
    macos::send(macos::Job::HideApp);
}

/// Gives the worker the app, which it needs to read the screens on the main thread.
#[cfg(target_os = "macos")]
pub fn init(app: tauri::AppHandle) {
    macos::init(app);
}

#[cfg(not(target_os = "macos"))]
pub fn init(_app: tauri::AppHandle) {}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::mpsc::{self, SendError, Sender};
    use std::sync::{Mutex, OnceLock, PoisonError};
    use std::time::Duration;

    use objc2_app_kit::{NSScreen, NSWorkspace};
    use objc2_core_foundation::{
        CFBoolean, CFGetTypeID, CFRetained, CFString, CFType, CGPoint, CGSize,
    };
    use objc2_foundation::MainThreadMarker;
    use tauri::AppHandle;

    use super::{tiled, Rect, Screen, WindowAction};

    /// How long one call to another app may take before it is given up.
    const AX_TIMEOUT: f32 = 0.5;
    /// How long the worker waits for the main thread to list the screens.
    const SCREENS_WAIT: Duration = Duration::from_millis(500);
    /// `AXValueType`: a point and a size.
    const AX_POINT: u32 = 1;
    const AX_SIZE: u32 = 2;
    /// `kAXErrorSuccess`.
    const AX_OK: i32 = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> Option<NonNull<CFType>>;
        fn AXUIElementCreateSystemWide() -> Option<NonNull<CFType>>;
        fn AXUIElementGetTypeID() -> usize;
        fn AXUIElementCopyAttributeValue(
            element: &CFType,
            attribute: &CFString,
            value: *mut *const CFType,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: &CFType,
            attribute: &CFString,
            value: &CFType,
        ) -> i32;
        fn AXUIElementPerformAction(element: &CFType, action: &CFString) -> i32;
        fn AXUIElementSetMessagingTimeout(element: &CFType, seconds: f32) -> i32;
        fn AXValueCreate(kind: u32, value: *const c_void) -> Option<NonNull<CFType>>;
        fn AXValueGetValue(value: &CFType, kind: u32, out: *mut c_void) -> bool;
    }

    pub enum Job {
        Window(WindowAction),
        HideApp,
    }

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static WORKER: Mutex<Option<Sender<Job>>> = Mutex::new(None);

    pub fn init(app: AppHandle) {
        // Set once at launch; a second call has nothing new to give.
        let _ = APP.set(app);
    }

    /// Hands `job` to the worker, starting it the first time (or again, if it ended).
    pub fn send(job: Job) {
        let mut worker = WORKER.lock().unwrap_or_else(PoisonError::into_inner);
        let job = match worker.as_ref() {
            Some(sender) => match sender.send(job) {
                Ok(()) => return,
                // The worker ended: start another.
                Err(SendError(job)) => job,
            },
            None => job,
        };
        let (sender, jobs) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("kleari-mouse-window".to_owned())
            .spawn(move || {
                limit_all();
                for job in jobs {
                    perform(job);
                }
            });
        if spawned.is_err() {
            // No thread now: this action is lost, and the next one tries again.
            crate::log::note("window", "the window-action thread couldn't start");
            return;
        }
        // The worker was just made and is waiting on this channel.
        let _ = sender.send(job);
        *worker = Some(sender);
    }

    fn perform(job: Job) {
        let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            crate::log::note("window", "no frontmost app for a window action");
            return;
        };
        match job {
            Job::HideApp => {
                // `false` if it was already hidden; nothing more to do.
                let _ = app.hide();
            }
            Job::Window(action) => {
                if let Some(window) = focused_window(app.processIdentifier()) {
                    act(&window, action);
                }
            }
        }
    }

    fn owned(value: Option<NonNull<CFType>>) -> Option<CFRetained<CFType>> {
        // SAFETY: the functions passed here follow the Create/Copy rule: an owned object.
        value.map(|value| unsafe { CFRetained::from_raw(value) })
    }

    /// Limits each call to `element` to `AX_TIMEOUT`. macOS applies a timeout only to the
    /// element it is set on (the system-wide element's sets the default for all), so it is set
    /// on each element Kleari talks to.
    fn limit(element: &CFType) {
        // SAFETY: `element` is a live AXUIElement. A failure leaves macOS's default (6 s), on
        // this worker thread only.
        unsafe { AXUIElementSetMessagingTimeout(element, AX_TIMEOUT) };
    }

    /// Sets the default for every element, once per worker.
    fn limit_all() {
        // SAFETY: takes no arguments; the result follows the Create rule.
        if let Some(system) = owned(unsafe { AXUIElementCreateSystemWide() }) {
            limit(&system);
        }
    }

    /// Records a failed Accessibility call: which step, which attribute or action (names
    /// written here, never anything read from the app) and macOS's error code.
    fn failed(step: &'static str, name: &'static str, error: i32) {
        crate::log::note_detail("window", step, name, i64::from(error));
    }

    fn attribute(element: &CFType, name: &'static str) -> Option<CFRetained<CFType>> {
        let mut value: *const CFType = std::ptr::null();
        // SAFETY: `element` is a live AXUIElement; on success `value` is an owned object.
        let error = unsafe {
            AXUIElementCopyAttributeValue(element, &CFString::from_str(name), &mut value)
        };
        if error != AX_OK {
            failed("couldn't read", name, error);
            return None;
        }
        let value = owned(NonNull::new(value.cast_mut()))?;
        // An element read here (a window, a button) gets its own limit too; other values (a
        // point, a flag) aren't elements and are left alone.
        // SAFETY: takes no arguments.
        if CFGetTypeID(Some(&value)) == unsafe { AXUIElementGetTypeID() } {
            limit(&value);
        }
        Some(value)
    }

    fn set(element: &CFType, name: &'static str, value: &CFType) -> bool {
        // SAFETY: both are live CF objects.
        let error =
            unsafe { AXUIElementSetAttributeValue(element, &CFString::from_str(name), value) };
        if error != AX_OK {
            failed("couldn't set", name, error);
        }
        error == AX_OK
    }

    /// The frontmost app's focused window.
    fn focused_window(pid: i32) -> Option<CFRetained<CFType>> {
        // SAFETY: any pid is accepted; a gone process just answers nothing.
        let app = owned(unsafe { AXUIElementCreateApplication(pid) })?;
        limit(&app);
        attribute(&app, "AXFocusedWindow")
    }

    fn act(window: &CFType, action: WindowAction) {
        match action {
            WindowAction::Minimize => {
                set(window, "AXMinimized", CFBoolean::new(true));
            }
            WindowAction::ToggleFullScreen => {
                let on = attribute(window, "AXFullScreen")
                    .and_then(|value| value.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool))
                    .unwrap_or(false);
                set(window, "AXFullScreen", CFBoolean::new(!on));
            }
            WindowAction::Close => {
                if let Some(button) = attribute(window, "AXCloseButton") {
                    // SAFETY: `button` is a live AXUIElement.
                    let error = unsafe {
                        AXUIElementPerformAction(&button, &CFString::from_str("AXPress"))
                    };
                    if error != AX_OK {
                        failed("couldn't press", "AXCloseButton", error);
                    }
                }
            }
            WindowAction::TileLeft | WindowAction::TileRight => {
                tile(window, action == WindowAction::TileLeft);
            }
        }
    }

    fn tile(window: &CFType, left: bool) {
        let (Some(position), Some(size)) =
            (attribute(window, "AXPosition"), attribute(window, "AXSize"))
        else {
            return;
        };
        let mut at = CGPoint::new(0.0, 0.0);
        let mut extent = CGSize::new(0.0, 0.0);
        // SAFETY: the values are AXValues of these types; the out pointers are to matching
        // structs.
        let read = unsafe {
            AXValueGetValue(&position, AX_POINT, (&raw mut at).cast())
                && AXValueGetValue(&size, AX_SIZE, (&raw mut extent).cast())
        };
        if !read {
            crate::log::note(
                "window",
                "tile: the window's position or size was unreadable",
            );
            return;
        }
        let current = Rect {
            x: at.x,
            y: at.y,
            width: extent.width,
            height: extent.height,
        };
        let Some(target) = screens().and_then(|screens| tiled(current, &screens, left)) else {
            crate::log::note("window", "tile: no screen to tile on");
            return;
        };
        // A window in full screen can't be moved; it leaves full screen first.
        set(window, "AXFullScreen", CFBoolean::new(false));
        let at = CGPoint::new(target.x, target.y);
        let extent = CGSize::new(target.width, target.height);
        // SAFETY: the pointers are to a CGPoint and a CGSize, as the types say.
        let (position, size) = unsafe {
            (
                owned(AXValueCreate(AX_POINT, (&raw const at).cast())),
                owned(AXValueCreate(AX_SIZE, (&raw const extent).cast())),
            )
        };
        if let (Some(position), Some(size)) = (position, size) {
            // Moved first, then sized, then moved again: an app may refuse a size that would
            // run off its current screen.
            set(window, "AXPosition", &position);
            set(window, "AXSize", &size);
            set(window, "AXPosition", &position);
        }
    }

    /// The screens, read on the main thread (AppKit's rule); the main screen first.
    fn screens() -> Option<Vec<Screen>> {
        let app = APP.get()?;
        let (reply, answer) = mpsc::channel();
        let asked = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let rect = |r: objc2_foundation::NSRect| Rect {
                x: r.origin.x,
                y: r.origin.y,
                width: r.size.width,
                height: r.size.height,
            };
            let screens = NSScreen::screens(mtm)
                .iter()
                .map(|screen| Screen {
                    frame: rect(screen.frame()),
                    visible: rect(screen.visibleFrame()),
                })
                .collect::<Vec<_>>();
            // The worker may have stopped waiting.
            let _ = reply.send(screens);
        });
        asked.ok()?;
        answer.recv_timeout(SCREENS_WAIT).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// A 1440×900 main screen with a 25-point menu bar and an 80-point Dock at the bottom, and
    /// a 1920×1080 screen to its right whose bottom lines up with the main one's.
    fn two_screens() -> Vec<Screen> {
        vec![
            Screen {
                frame: rect(0.0, 0.0, 1440.0, 900.0),
                visible: rect(0.0, 80.0, 1440.0, 795.0),
            },
            Screen {
                frame: rect(1440.0, 0.0, 1920.0, 1080.0),
                visible: rect(1440.0, 0.0, 1920.0, 1055.0),
            },
        ]
    }

    #[test]
    fn a_window_tiles_to_half_the_usable_part_of_its_screen() {
        let screens = two_screens();
        let window = rect(100.0, 100.0, 600.0, 400.0);
        // Below the menu bar (y 25 from the top), above the Dock.
        assert_eq!(
            tiled(window, &screens, true),
            Some(rect(0.0, 25.0, 720.0, 795.0))
        );
        assert_eq!(
            tiled(window, &screens, false),
            Some(rect(720.0, 25.0, 720.0, 795.0))
        );
    }

    #[test]
    fn a_window_on_the_second_screen_tiles_there() {
        let screens = two_screens();
        // The second screen, in Accessibility's coordinates, runs from y = 900 − 1080 = −180.
        let window = rect(2000.0, 0.0, 800.0, 600.0);
        assert_eq!(
            tiled(window, &screens, true),
            Some(rect(1440.0, -155.0, 960.0, 1055.0))
        );
    }

    #[test]
    fn an_odd_width_keeps_every_point_and_no_screens_means_nothing() {
        let screens = vec![Screen {
            frame: rect(0.0, 0.0, 1001.0, 800.0),
            visible: rect(0.0, 0.0, 1001.0, 800.0),
        }];
        let window = rect(5000.0, 5000.0, 10.0, 10.0);
        // Off every screen: the main one. Halves 500 and 501.
        assert_eq!(
            tiled(window, &screens, false),
            Some(rect(500.0, 0.0, 501.0, 800.0))
        );
        assert_eq!(tiled(window, &[], true), None);
    }
}
