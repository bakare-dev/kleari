//! The event tap (macOS): mouse wheel and extra-button events reach Kleari here before any
//! app, and `Engine` decides what happens to each. It never sees keystrokes or pointer
//! movement, and sees left and right clicks only while a combination uses one: only those
//! event types are asked for. Keys, clicks and glides Kleari sends are tagged, so the tap
//! lets its own events through.
//!
//! macOS creates the tap only once the user has allowed Kleari under Privacy & Security ›
//! Accessibility. It runs on its own thread with its own run loop, checked four times a second
//! for a request to stop.

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRetained, CFRunLoop, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGMouseButton, CGScrollEventUnit,
};

use super::config::{Clicks, Modifiers, MouseSettings, LEFT, RIGHT};
use super::engine::{glide_step, Effect, Engine, Input, Keys, Media, Output};

/// Marks events Kleari posts ("MPMS"), in the field macOS keeps for the poster's own use.
const OURS: i64 = 0x4D50_4D53;
/// How often a glide moves: about 120 frames a second.
const FRAME: Duration = Duration::from_millis(8);
/// How often the tap's thread checks whether it should stop.
const CHECK_SECONDS: f64 = 0.25;

/// How long `stop` waits for the tap's thread to finish. It normally takes at most one run-loop
/// check (a quarter of a second); only a callback that never returned could take longer.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// A running tap; dropping it doesn't stop it, `stop` does.
pub struct Handle {
    wanted: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    /// Disconnected once the tap's thread has finished (its sender is dropped on exit).
    finished: Receiver<()>,
    /// Which of left and right click the tap also sees: each only while a combination uses it.
    clicks: Clicks,
}

impl Handle {
    /// Which of left and right click the tap sees (fixed when it is made).
    pub fn clicks(&self) -> Clicks {
        self.clicks
    }

    /// The tap is installed and its thread running (false if macOS refused it).
    pub fn running(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Asked to keep running (not being stopped).
    pub fn wanted(&self) -> bool {
        self.wanted.load(Ordering::Relaxed)
    }

    /// From now on the tap passes every event untouched, and its thread ends (`stop` waits for
    /// that). Called with the engine's lock held, it takes effect between two events.
    pub fn let_go(&self) {
        self.wanted.store(false, Ordering::Relaxed);
    }

    /// Stops the tap and waits, at most `STOP_WAIT`, for its thread to finish: the tap removed
    /// and what it made freed. `false` if it hadn't finished by then; the caller must then not
    /// start another tap (both would handle the same events) and can ask again later.
    pub fn stop(&self) -> bool {
        self.let_go();
        matches!(
            self.finished.recv_timeout(STOP_WAIT),
            Err(RecvTimeoutError::Disconnected)
        )
    }
}

/// What the tap's callback works with, on the tap's own thread.
struct Context {
    settings: Arc<RwLock<MouseSettings>>,
    /// `MouseState::engine`, locked for each event.
    engine: Arc<Mutex<Engine>>,
    /// Cleared once the tap is let go (`Handle::let_go`): every event then passes untouched.
    wanted: Arc<AtomicBool>,
    port: RefCell<Option<CFRetained<CFMachPort>>>,
    glide: Sender<(f64, f64)>,
}

impl Context {
    /// The engine, for one event; its state is rebuilt from events, so a poisoned lock is
    /// usable. `None` once the tap has been let go, checked under the lock so a replacement's
    /// check that nothing is held stays true.
    fn engine(&self) -> Option<MutexGuard<'_, Engine>> {
        let engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
        self.wanted.load(Ordering::Relaxed).then_some(engine)
    }
}

/// Whether macOS lets Kleari change input (Accessibility).
pub fn allowed() -> bool {
    objc2_core_graphics::CGPreflightPostEventAccess()
}

/// Asks macOS for Accessibility access: it shows its own prompt the first time and lists
/// Kleari in System Settings, off, after that. Returns whether access is now allowed.
pub fn request() -> bool {
    objc2_core_graphics::CGRequestPostEventAccess()
}

/// How long `start` waits for macOS to accept or refuse the tap.
const READY: Duration = Duration::from_secs(1);

/// Installs the tap on a new thread and waits for macOS to accept or refuse it; `running` on
/// the handle says which. It also sees the clicks in `clicks`, and decides with `engine`, which
/// a tap replacing it carries on with.
pub fn start(
    settings: Arc<RwLock<MouseSettings>>,
    engine: Arc<Mutex<Engine>>,
    clicks: Clicks,
    revoked: impl Fn() -> bool + Send + 'static,
) -> Handle {
    let wanted = Arc::new(AtomicBool::new(true));
    let alive = Arc::new(AtomicBool::new(false));
    let (thread_wanted, thread_alive) = (Arc::clone(&wanted), Arc::clone(&alive));
    let (ready, installed) = mpsc::channel();
    let (done, finished) = mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name("kleari-mouse".to_owned())
        .spawn(move || {
            run(
                settings,
                engine,
                clicks,
                Arc::clone(&thread_wanted),
                &thread_alive,
                &ready,
                revoked,
            );
            thread_alive.store(false, Ordering::Relaxed);
            // Dropping it tells `stop` the thread has finished.
            drop(done);
        });
    if spawned.is_ok() {
        // No answer in time counts as refused; a tap that comes up later still shows as running
        // the next time it is asked.
        let _ = installed.recv_timeout(READY);
    }
    // If the thread couldn't be made, the sender went with the closure: already finished.
    Handle {
        wanted,
        alive,
        finished,
        clicks,
    }
}

const fn bit(kind: CGEventType) -> u64 {
    1 << kind.0
}

fn run(
    settings: Arc<RwLock<MouseSettings>>,
    engine: Arc<Mutex<Engine>>,
    clicks: Clicks,
    wanted: Arc<AtomicBool>,
    alive: &AtomicBool,
    ready: &Sender<()>,
    revoked: impl Fn() -> bool + Send + 'static,
) {
    // Whether the current withdrawal of Accessibility has been reported (`revoked` returned
    // true); until then it is reported again at each wake. Cleared once access is back.
    let mut reported = false;
    let (glide, glides) = mpsc::channel();
    let glider = std::thread::Builder::new()
        .name("kleari-glide".to_owned())
        .spawn(move || animate(&glides));
    let context = Box::into_raw(Box::new(Context {
        settings,
        engine,
        wanted: Arc::clone(&wanted),
        port: RefCell::new(None),
        glide,
    }));
    let mut mask = bit(CGEventType::ScrollWheel)
        | bit(CGEventType::OtherMouseDown)
        | bit(CGEventType::OtherMouseUp)
        | bit(CGEventType::OtherMouseDragged);
    // Left and right click are each seen only while a combination uses it (`Handle::clicks`).
    if clicks.left {
        mask |= bit(CGEventType::LeftMouseDown)
            | bit(CGEventType::LeftMouseUp)
            | bit(CGEventType::LeftMouseDragged);
    }
    if clicks.right {
        mask |= bit(CGEventType::RightMouseDown)
            | bit(CGEventType::RightMouseUp)
            | bit(CGEventType::RightMouseDragged);
    }
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
    if let Some(port) = port {
        if let (Some(source), Some(run_loop)) = (
            CFMachPort::new_run_loop_source(None, Some(&port), 0),
            CFRunLoop::current(),
        ) {
            // SAFETY: a static macOS defines; read once here.
            let mode = unsafe { kCFRunLoopDefaultMode };
            run_loop.add_source(Some(&source), mode);
            // SAFETY: the callback reads `port` only on this thread, inside the run loop below.
            unsafe { &*context }.port.replace(Some(port.clone()));
            CGEvent::tap_enable(&port, true);
            alive.store(true, Ordering::Relaxed);
            // `start` may have stopped waiting; then there is no one to tell.
            let _ = ready.send(());
            while wanted.load(Ordering::Relaxed) {
                CFRunLoop::run_in_mode(mode, CHECK_SECONDS, false);
                // Accessibility withdrawn in System Settings: say so, so the settings are put in
                // effect again (draining and stopping this tap); again at each wake until the
                // report gets through. Allowed again (and this tap kept), a later withdrawal is
                // reported afresh.
                if allowed() {
                    reported = false;
                } else if !reported {
                    reported = revoked();
                }
            }
            CGEvent::tap_enable(&port, false);
        }
        port.invalidate();
    }
    // Refused or stopped: `start` stops waiting at once.
    let _ = ready.send(());
    // SAFETY: the tap is invalidated (or was never made), so the callback can't run again;
    // this is the only owner. Dropping it closes the glide channel, which ends the glider.
    drop(unsafe { Box::from_raw(context) });
    if let Ok(glider) = glider {
        // The glider only posts events; nothing to report if it panicked.
        let _ = glider.join();
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
    // SAFETY: `user` is the `Context` `run` made, alive while the tap is.
    let context = unsafe { &*user.cast::<Context>() };
    if kind == CGEventType::TapDisabledByTimeout {
        // macOS turns a slow tap off; it is turned straight back on.
        if let Some(port) = context.port.borrow().as_ref() {
            CGEvent::tap_enable(port, true);
        }
        return pass;
    }
    // SAFETY: macOS passes a valid event for the duration of the call.
    let event_ref = unsafe { event.as_ref() };
    // A failure in deciding must never stop the mouse: the event goes through unchanged.
    match catch_unwind(AssertUnwindSafe(|| handle(context, kind, event_ref))) {
        Ok(false) => std::ptr::null_mut(),
        Ok(true) | Err(_) => pass,
    }
}

fn field(event: &CGEvent, field: CGEventField) -> i64 {
    CGEvent::integer_value_field(Some(event), field)
}

fn modifiers(flags: CGEventFlags) -> Modifiers {
    Modifiers {
        command: flags.contains(CGEventFlags::MaskCommand),
        shift: flags.contains(CGEventFlags::MaskShift),
        option: flags.contains(CGEventFlags::MaskAlternate),
        control: flags.contains(CGEventFlags::MaskControl),
    }
}

/// Whole wheel lines; a step too small to count as a line still counts as one, in its direction.
fn lines(event: &CGEvent, delta: CGEventField, fixed: CGEventField) -> i64 {
    match field(event, delta) {
        0 => {
            let fraction = CGEvent::double_value_field(Some(event), fixed);
            if fraction > 0.0 {
                1
            } else if fraction < 0.0 {
                -1
            } else {
                0
            }
        }
        lines => lines,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Down,
    Up,
    Dragged,
}

/// A button event as the button (macOS's numbering: 0 left, 1 right, 2 middle…) and what it
/// did; `None` for any other event.
fn button_event(kind: CGEventType, event: &CGEvent) -> Option<(u8, Phase)> {
    let (phase, click) = match kind {
        CGEventType::LeftMouseDown => (Phase::Down, Some(LEFT)),
        CGEventType::LeftMouseUp => (Phase::Up, Some(LEFT)),
        CGEventType::LeftMouseDragged => (Phase::Dragged, Some(LEFT)),
        CGEventType::RightMouseDown => (Phase::Down, Some(RIGHT)),
        CGEventType::RightMouseUp => (Phase::Up, Some(RIGHT)),
        CGEventType::RightMouseDragged => (Phase::Dragged, Some(RIGHT)),
        CGEventType::OtherMouseDown => (Phase::Down, None),
        CGEventType::OtherMouseUp => (Phase::Up, None),
        CGEventType::OtherMouseDragged => (Phase::Dragged, None),
        _ => return None,
    };
    let button =
        click.or_else(|| u8::try_from(field(event, CGEventField::MouseEventButtonNumber)).ok())?;
    Some((button, phase))
}

/// Decides one event and carries it out; `false` drops the event.
fn handle(context: &Context, kind: CGEventType, event: &CGEvent) -> bool {
    if field(event, CGEventField::EventSourceUserData) == OURS {
        return true;
    }
    // Held for the whole decision; a tap being replaced no longer touches anything.
    let Some(mut engine) = context.engine() else {
        return true;
    };
    let pressed = button_event(kind, event);
    // Buttons are being tested in the Mouse page's finder: the real buttons go through
    // untouched (Kleari's window then only lights them up, dropping them). A release ends its
    // own button's pairing; other buttons keep theirs.
    if super::probe::capturing() {
        if let Some((button, Phase::Up)) = pressed {
            engine.released(button);
        }
        return true;
    }
    // A press Kleari's window dropped while testing (recorded only where it was dropped, so
    // never one another app received): its drag and release are dropped too, wherever the
    // pointer is now. A new press of the button starts afresh.
    if let Some((button, phase)) = pressed {
        match phase {
            Phase::Down => super::probe::clear_press(button),
            Phase::Dragged if super::probe::press_dropped(button) => {
                return false;
            }
            Phase::Up if super::probe::press_dropped(button) => {
                super::probe::clear_press(button);
                // The tap may have handled the press just before testing began (marking it
                // held, say): end that pairing too, without the button's usual release action.
                engine.released(button);
                return false;
            }
            _ => {}
        }
    }
    let input = match (kind, pressed) {
        (_, Some((button, Phase::Down))) => Input::Down {
            button,
            modifiers: modifiers(CGEvent::flags(Some(event))),
        },
        (_, Some((button, Phase::Up))) => Input::Up(button),
        (_, Some((button, Phase::Dragged))) => Input::Dragged(button),
        (CGEventType::ScrollWheel, None) => Input::Scroll {
            continuous: field(event, CGEventField::ScrollWheelEventIsContinuous) != 0,
            vertical: lines(
                event,
                CGEventField::ScrollWheelEventDeltaAxis1,
                CGEventField::ScrollWheelEventFixedPtDeltaAxis1,
            ),
            horizontal: lines(
                event,
                CGEventField::ScrollWheelEventDeltaAxis2,
                CGEventField::ScrollWheelEventFixedPtDeltaAxis2,
            ),
            modifiers: modifiers(CGEvent::flags(Some(event))),
        },
        _ => return true,
    };
    let output = {
        // Settings are replaced whole, never left half-written, so a poisoned lock is usable.
        let settings = context
            .settings
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        engine.handle(input, &settings)
    };
    drop(engine);
    match output {
        Output::Pass => true,
        Output::Suppress => false,
        Output::Adjust {
            reverse_vertical,
            reverse_horizontal,
            percent,
        } => {
            adjust(event, Axis::Vertical, reverse_vertical, percent);
            adjust(event, Axis::Horizontal, reverse_horizontal, percent);
            true
        }
        Output::Glide {
            vertical,
            horizontal,
        } => {
            // The glider ends only with this thread; if it is gone, the step is lost.
            let _ = context.glide.send((vertical, horizontal));
            false
        }
        Output::Act(effects) => {
            let at = CGEvent::location(Some(event));
            // A click sent in place of this event keeps the modifier keys held now (⌘-right-click
            // stays one).
            let held = CGEvent::flags(Some(event));
            for effect in effects {
                perform(effect, at, held);
            }
            false
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Vertical,
    Horizontal,
}

/// Reverses and scales one direction of a wheel event, in each of its three forms.
fn adjust(event: &CGEvent, axis: Axis, reverse: bool, percent: u16) {
    let (delta, fixed, points) = match axis {
        Axis::Vertical => (
            CGEventField::ScrollWheelEventDeltaAxis1,
            CGEventField::ScrollWheelEventFixedPtDeltaAxis1,
            CGEventField::ScrollWheelEventPointDeltaAxis1,
        ),
        Axis::Horizontal => (
            CGEventField::ScrollWheelEventDeltaAxis2,
            CGEventField::ScrollWheelEventFixedPtDeltaAxis2,
            CGEventField::ScrollWheelEventPointDeltaAxis2,
        ),
    };
    let factor = f64::from(percent) / 100.0 * if reverse { -1.0 } else { 1.0 };
    let scale_whole = |value: i64| -> i64 {
        if value == 0 {
            return 0;
        }
        let scaled = (value as f64 * factor).round() as i64;
        // A step never disappears by being scaled down.
        if scaled == 0 {
            if (value > 0) == (factor > 0.0) {
                1
            } else {
                -1
            }
        } else {
            scaled
        }
    };
    let lines = field(event, delta);
    let fraction = CGEvent::double_value_field(Some(event), fixed);
    let moved = field(event, points);
    CGEvent::set_integer_value_field(Some(event), delta, scale_whole(lines));
    CGEvent::set_double_value_field(Some(event), fixed, fraction * factor);
    CGEvent::set_integer_value_field(Some(event), points, scale_whole(moved));
}

/// Posts an event Kleari made, tagged so the tap lets it through.
fn post(event: &CGEvent) {
    CGEvent::set_integer_value_field(Some(event), CGEventField::EventSourceUserData, OURS);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(event));
}

fn flags(keys: Keys) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    let modifiers = keys.modifiers;
    for (on, flag) in [
        (modifiers.command, CGEventFlags::MaskCommand),
        (modifiers.shift, CGEventFlags::MaskShift),
        (modifiers.option, CGEventFlags::MaskAlternate),
        (modifiers.control, CGEventFlags::MaskControl),
    ] {
        if on {
            flags |= flag;
        }
    }
    // Arrow keys carry these on a real keyboard; macOS's Space and Mission Control shortcuts
    // don't recognise ⌃+arrow without them.
    if (123..=126).contains(&keys.code) {
        flags |= CGEventFlags::MaskSecondaryFn | CGEventFlags::MaskNumericPad;
    }
    flags
}

/// Carries out `effect` at `at`; a click carries the modifier keys in `held`.
fn perform(effect: Effect, at: CGPoint, held: CGEventFlags) {
    match effect {
        Effect::Keys(keys) => {
            for down in [true, false] {
                if let Some(event) = CGEvent::new_keyboard_event(None, keys.code, down) {
                    CGEvent::set_flags(Some(&event), flags(keys));
                    post(&event);
                }
            }
        }
        Effect::Click(button) => {
            // A right click held back for a combination is sent as a right click.
            let kinds = if button == RIGHT {
                [CGEventType::RightMouseDown, CGEventType::RightMouseUp]
            } else {
                [CGEventType::OtherMouseDown, CGEventType::OtherMouseUp]
            };
            // The modifier state of the click it replaces, Caps Lock and Fn included.
            let keys = held
                & (CGEventFlags::MaskCommand
                    | CGEventFlags::MaskShift
                    | CGEventFlags::MaskAlternate
                    | CGEventFlags::MaskControl
                    | CGEventFlags::MaskAlphaShift
                    | CGEventFlags::MaskSecondaryFn);
            for kind in kinds {
                if let Some(event) =
                    CGEvent::new_mouse_event(None, kind, at, CGMouseButton(u32::from(button)))
                {
                    // macOS ignores the constructor's button for a right click; set it, and one
                    // click (not part of a double click), explicitly.
                    CGEvent::set_integer_value_field(
                        Some(&event),
                        CGEventField::MouseEventButtonNumber,
                        i64::from(button),
                    );
                    CGEvent::set_integer_value_field(
                        Some(&event),
                        CGEventField::MouseEventClickState,
                        1,
                    );
                    CGEvent::set_flags(Some(&event), keys);
                    post(&event);
                }
            }
        }
        Effect::Scroll {
            vertical,
            horizontal,
        } => scroll_points(vertical, horizontal),
        Effect::Media(media) => {
            for down in [true, false] {
                media_key(media, down);
            }
        }
        Effect::Command(down) => {
            // kVK_Command: held for the app switcher, released when the buttons are.
            if let Some(event) = CGEvent::new_keyboard_event(None, 55, down) {
                let flags = if down {
                    CGEventFlags::MaskCommand
                } else {
                    CGEventFlags::empty()
                };
                CGEvent::set_flags(Some(&event), flags);
                post(&event);
            }
        }
    }
}

/// The key type macOS gives each media key (`NX_KEYTYPE_*` in IOKit's `ev_keymap.h`).
const fn media_type(media: Media) -> isize {
    match media {
        Media::VolumeUp => 0,
        Media::VolumeDown => 1,
        Media::Mute => 7,
        Media::PlayPause => 16,
        Media::Next => 17,
        Media::Previous => 18,
    }
}

/// Posts a media key press or release, as a Mac keyboard's media keys send it: a
/// system-defined event of subtype 8 whose data carries the key type and the key state.
fn media_key(media: Media, down: bool) {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;
    // 0xa is "key down", 0xb "key up", in both the flags and the data.
    let state: isize = if down { 0xa } else { 0xb };
    let Some(event) =
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags(state.unsigned_abs() << 8),
            0.0,
            0,
            None,
            8,
            (media_type(media) << 16) | (state << 8),
            -1,
        )
    else {
        return;
    };
    if let Some(event) = event.CGEvent() {
        post(&event);
    }
}

fn scroll_points(vertical: f64, horizontal: f64) {
    // Whole points, well within i32 (a step is a few hundred points at most).
    let (vertical, horizontal) = (vertical.round() as i32, horizontal.round() as i32);
    if vertical == 0 && horizontal == 0 {
        return;
    }
    if let Some(event) =
        CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, vertical, horizontal, 0)
    {
        post(&event);
    }
}

/// Glides each wheel step: steps arriving while one glides add to what is left.
fn animate(glides: &Receiver<(f64, f64)>) {
    let (mut vertical, mut horizontal) = (0.0_f64, 0.0_f64);
    loop {
        let idle = glide_step(vertical) == 0.0 && glide_step(horizontal) == 0.0;
        let next = if idle {
            glides.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            glides.recv_timeout(FRAME)
        };
        match next {
            Ok((more_vertical, more_horizontal)) => {
                if idle {
                    // A new glide starts from nothing left over.
                    (vertical, horizontal) = (0.0, 0.0);
                }
                vertical += more_vertical;
                horizontal += more_horizontal;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let (step_vertical, step_horizontal) = (glide_step(vertical), glide_step(horizontal));
        vertical -= step_vertical;
        horizontal -= step_horizontal;
        scroll_points(step_vertical, step_horizontal);
    }
}
