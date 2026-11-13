//! The Mouse module (ADR-016): a mouse wheel that scrolls the classic way while the trackpad
//! keeps natural scrolling, wheel speed and smooth scrolling, extra buttons mapped to actions
//! or shortcuts, and a button or modifier held to make the wheel do something else.
//!
//! It works through an event tap (`tap.rs`), which macOS allows only once the user turns
//! Kleari on under Privacy & Security › Accessibility, and only while Kleari runs. The tap
//! sees only wheel and extra-button events; nothing it sees is stored or sent anywhere.

pub mod config;
// Only the macOS event tap drives it; on other targets (Linux CI) only its tests do.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub mod engine;
pub mod probe;
#[cfg(target_os = "macos")]
mod tap;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Mutex;
use std::sync::{Arc, PoisonError, RwLock};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::{CommandError, CommandErrorCode};
use crate::history::History;
use config::{MouseSettings, SaveError};

/// Whether macOS lets Kleari change mouse input.
// macOS reports the first two, other targets (Linux CI) only the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Allowed,
    /// Not (yet) turned on in Privacy & Security › Accessibility.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NotAllowed,
    /// Not on this system (only macOS has the event tap).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported,
}

/// What the Mouse page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseStatus {
    pub settings: MouseSettings,
    /// `saved`, `default`, `invalid` or `unavailable`, as for the preferences.
    pub source: &'static str,
    pub access: Access,
    /// The settings are being applied to the mouse now.
    pub active: bool,
}

#[cfg(target_os = "macos")]
fn access() -> Access {
    if tap::allowed() {
        Access::Allowed
    } else {
        Access::NotAllowed
    }
}

#[cfg(not(target_os = "macos"))]
fn access() -> Access {
    Access::Unsupported
}

/// The settings the tap reads, and the tap while it runs.
#[derive(Default)]
pub struct MouseState {
    /// The settings the running tap decides with. A change that needs a new tap takes effect
    /// with it, never half in the old one (`run`).
    settings: Arc<RwLock<MouseSettings>>,
    /// The settings last read or saved, to put in effect; a waiting change is retried with them.
    #[cfg(target_os = "macos")]
    latest: RwLock<MouseSettings>,
    // Only macOS has the event tap.
    #[cfg(target_os = "macos")]
    tap: Mutex<Option<tap::Handle>>,
    /// What the tap holds between events. Kept here so a replacement or stop can tell, under
    /// its lock, that nothing is held before the old tap lets go (`run`).
    #[cfg(target_os = "macos")]
    engine: Arc<Mutex<engine::Engine>>,
    /// Since when a replacement or stop has been waiting for held buttons to be let go.
    #[cfg(target_os = "macos")]
    waiting: Mutex<Option<Instant>>,
    /// A retry is scheduled (`retry`); one chain at a time.
    #[cfg(target_os = "macos")]
    deferred: AtomicBool,
}

/// How soon a change waiting for held buttons is tried again.
#[cfg(target_os = "macos")]
const RETRY: Duration = Duration::from_millis(200);
/// How long a change waits for held buttons at most; then it goes ahead, so a press whose
/// release never came can't keep the old tap forever.
#[cfg(target_os = "macos")]
const MAX_WAIT: Duration = Duration::from_secs(10);

impl MouseState {
    /// Puts `settings` in effect: the tap runs while they are on and macOS allows it, and is
    /// stopped otherwise. Returns whether the tap is running, and the access answer that was
    /// acted on (so the page never shows one from another moment).
    fn apply(&self, app: &AppHandle, settings: &MouseSettings) -> (bool, Access) {
        #[cfg(target_os = "macos")]
        {
            *self.latest.write().unwrap_or_else(PoisonError::into_inner) = settings.clone();
            self.run(app)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            self.publish(settings);
            (false, access())
        }
    }

    /// The running tap decides with `settings` from its next event.
    fn publish(&self, settings: &MouseSettings) {
        *self
            .settings
            .write()
            .unwrap_or_else(PoisonError::into_inner) = settings.clone();
    }

    /// Puts the latest settings in effect, if macOS allows it. The tap sees each of left and
    /// right click only while a combination uses it, so a change of which it needs replaces the
    /// tap.
    ///
    /// A replacement, or a stop, waits until no press is held or held back (at most
    /// `MAX_WAIT`), so none is split from its release. Meanwhile a replacement keeps the old
    /// settings whole, and a stop puts in off settings at once: new presses then pass untouched,
    /// and held ones still pair with their release (`Engine::handle`).
    #[cfg(target_os = "macos")]
    fn run(&self, app: &AppHandle) -> (bool, Access) {
        // Only `Some`/`None` is kept here; a panic elsewhere can't leave it half-changed.
        // Held throughout, so no other change can start a tap while this one is stopping.
        let mut tap = self.tap.lock().unwrap_or_else(PoisonError::into_inner);
        // Both read under the tap's lock, so a change waiting its turn (or a retry) never
        // applies settings, or an access answer, that a later one replaced.
        let access = access();
        let active = self.transition(app, &mut tap, access);
        (active, access)
    }

    /// `run`'s change, with the tap's lock held (`tap`) and `access` read under it.
    #[cfg(target_os = "macos")]
    fn transition(&self, app: &AppHandle, tap: &mut Option<tap::Handle>, access: Access) -> bool {
        let mut settings = self
            .latest
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let wanted = settings.enabled && access == Access::Allowed;
        // Not wanted (off, or macOS no longer allows it): the tap, while it lasts, changes
        // nothing, and only pairs held presses with their release.
        settings.enabled = wanted;
        let settings = &settings;
        let clicks = settings.clicks();
        if let Some(handle) = tap.take() {
            let live = handle.wanted() && handle.running();
            if wanted && live && handle.clicks() == clicks {
                self.publish(settings);
                self.settle();
                *tap = Some(handle);
                return true;
            }
            if live {
                if !wanted {
                    self.publish(settings);
                }
                // Checked and handed over under the engine's lock, which the tap takes for each
                // event: once let go, the old tap passes every event untouched, so nothing
                // becomes held between this check and the tap's removal.
                let engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
                if !engine.idle() && !self.waited_too_long() {
                    drop(engine);
                    *tap = Some(handle);
                    self.retry(app);
                    // A replacement keeps the old settings working meanwhile; a stop doesn't.
                    return wanted;
                }
                handle.let_go();
            }
            self.settle();
            // Waits (boundedly) until the old tap is removed. If it hasn't been, no new tap
            // starts, so two never handle the same events; the old one is asked again next time,
            // and meanwhile the page says the settings aren't running.
            if !handle.stop() {
                *tap = Some(handle);
                return false;
            }
        }
        // Nothing is held (the change waited for that), or no tap was watching releases.
        *self.engine.lock().unwrap_or_else(PoisonError::into_inner) = engine::Engine::default();
        self.publish(settings);
        if !wanted {
            return false;
        }
        // If Accessibility is withdrawn while it runs, the tap says so, and the settings are put
        // in effect again (from another thread: this one would wait for the tap to finish).
        // `false` if that thread couldn't start; the tap then asks again at its next wake.
        let revoked_app = app.clone();
        let revoked = move || {
            let app = revoked_app.clone();
            std::thread::Builder::new()
                .name("kleari-mouse-revoked".to_owned())
                .spawn(move || {
                    app.state::<MouseState>().run(&app);
                })
                .is_ok()
        };
        let handle = tap::start(
            Arc::clone(&self.settings),
            Arc::clone(&self.engine),
            clicks,
            revoked,
        );
        let running = handle.running();
        *tap = Some(handle);
        running
    }

    /// A waiting change has waited `MAX_WAIT` (counted from its first wait).
    #[cfg(target_os = "macos")]
    fn waited_too_long(&self) -> bool {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        waiting.get_or_insert_with(Instant::now).elapsed() >= MAX_WAIT
    }

    /// Nothing is waiting any more.
    #[cfg(target_os = "macos")]
    fn settle(&self) {
        *self.waiting.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Tries the waiting change again shortly, with the latest settings and access; each try
    /// that still finds a button held schedules the next. One chain at a time.
    #[cfg(target_os = "macos")]
    fn retry(&self, app: &AppHandle) {
        if self.deferred.swap(true, Ordering::Relaxed) {
            return;
        }
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("kleari-mouse-retry".to_owned())
            .spawn(move || {
                std::thread::sleep(RETRY);
                let state = app.state::<MouseState>();
                state.deferred.store(false, Ordering::Relaxed);
                state.run(&app);
            });
        if spawned.is_err() {
            // No thread now: the next read or save of the settings tries again.
            self.deferred.store(false, Ordering::Relaxed);
        }
    }
}

/// Reads the saved settings and puts them in effect (at launch, and on each read, so the tap
/// starts as soon as Accessibility is turned on).
fn refresh(app: &AppHandle) -> MouseStatus {
    let history = app.state::<History>();
    let loaded = config::load(&history.db);
    let (active, access) = app.state::<MouseState>().apply(app, &loaded.settings);
    MouseStatus {
        settings: loaded.settings,
        source: loaded.source,
        access,
        active,
    }
}

/// At launch: the saved settings take effect if they are on and macOS allows it.
pub fn start(app: &AppHandle) {
    let _ = refresh(app);
    // The button finder's in-app monitor; silent until the Mouse page listens.
    probe::install(app.clone());
}

/// The Mouse page starts (`true`) and stops (`false`) listening for which button is pressed.
#[tauri::command]
pub fn listen_mouse_probe(on: bool) {
    probe::listen(on);
}

/// The pointer is over the finder's box (`true`, sent again every second while it stays) or
/// has left it. While it is, tested buttons and the wheel only light up the drawing.
#[tauri::command]
pub fn capture_mouse_probe(on: bool) {
    probe::capture(on);
}

fn blocking_failed(_: tauri::Error) -> CommandError {
    CommandErrorCode::MouseUnavailable.into()
}

#[tauri::command]
pub async fn get_mouse(app: AppHandle) -> Result<MouseStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || refresh(&app))
        .await
        .map_err(blocking_failed)
}

/// Saves the settings (if every value is one the page offers) and applies them at once.
#[tauri::command]
pub async fn set_mouse(
    app: AppHandle,
    settings: MouseSettings,
) -> Result<MouseStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let history = app.state::<History>();
        config::save(&history.db, &settings).map_err(|error| match error {
            SaveError::Invalid => CommandError::from(CommandErrorCode::InvalidRequest),
            SaveError::Unavailable(_) => CommandErrorCode::HistoryUnavailable.into(),
        })?;
        Ok(refresh(&app))
    })
    .await
    .map_err(blocking_failed)?
}

/// Asks macOS for Accessibility access. The first time macOS shows its own prompt; after
/// that it only lists Kleari in Privacy & Security › Accessibility, where the user turns it
/// on. Returns where things stand, applying the settings if access is now allowed.
#[tauri::command]
pub async fn request_mouse_access(app: AppHandle) -> Result<MouseStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        tap::request();
        refresh(&app)
    })
    .await
    .map_err(blocking_failed)
}
