//! The menu-bar item: an icon that opens a small popover (live readings, quarantine, quick
//! links) below it. While it is on (Settings › General › Show in menu bar, the default),
//! closing the main window keeps Kleari running in the menu bar; Quit from the menu bar, the
//! app menu or ⌘Q ends it. While it is off, closing the window quits as usual.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{AppHandle, LogicalPosition, Manager, Rect, WebviewUrl, WebviewWindowBuilder};

use crate::commands::{CommandError, CommandErrorCode};

const TRAY_ID: &str = "kleari";
pub const POPOVER: &str = "popover";
const MAIN: &str = "main";
/// The popover's size in points; its page fits itself to this.
const POPOVER_WIDTH: f64 = 340.0;
const POPOVER_HEIGHT: f64 = 726.0;
/// Matches the panel's corner radius in popover.css.
const POPOVER_RADIUS: f64 = 14.0;
/// Space kept between the popover and the screen edges, in points.
const SCREEN_MARGIN: f64 = 8.0;
/// Clicking the icon while the popover is open first takes focus from it, which closes it;
/// the click that follows within this time must not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// When the popover last closed because it lost focus.
#[derive(Default)]
pub struct PopoverState {
    closed_on_blur: Mutex<Option<Instant>>,
}

/// Closes the popover because it lost focus (a click elsewhere, or on the icon).
pub fn close_on_blur(window: &tauri::Window) {
    if let Ok(mut closed) = window.state::<PopoverState>().closed_on_blur.lock() {
        *closed = Some(Instant::now());
    }
    // Best effort: if macOS refuses, it stays open until the next click on the icon.
    let _ = window.hide();
}

/// Whether the popover closed on blur just now, consuming that record.
fn just_closed(app: &AppHandle) -> bool {
    app.state::<PopoverState>()
        .closed_on_blur
        .lock()
        .ok()
        .and_then(|mut closed| closed.take())
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD)
}

/// Whether the menu-bar item is on screen, so closing the window can keep Kleari running.
/// Read from the item itself, so it can't claim an icon that failed to appear.
pub fn active(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

/// Shows or removes the menu-bar item to match the preference. Must run on the main thread.
/// Only showing it can fail, and then there is no item.
pub fn apply(app: &AppHandle, on: bool) -> tauri::Result<()> {
    let exists = active(app);
    if on && !exists {
        // 44×44 RGBA pixels of icons/tray.svg (rendered at 22 pt @2x), so no image decoder is
        // needed. macOS tints it, as a template image.
        let icon = Image::new(include_bytes!("../icons/tray.rgba"), 44, 44);
        TrayIconBuilder::with_id(TRAY_ID)
            .icon(icon)
            .icon_as_template(true)
            .tooltip("Kleari")
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } = event
                {
                    toggle_popover(tray.app_handle(), rect);
                }
            })
            .build(app)?;
        // Made now, hidden, so the first click opens it at once with its data read. If this
        // fails, the first click tries again.
        let _ = popover(app);
    } else if !on && exists {
        app.remove_tray_by_id(TRAY_ID);
        // Closed, not hidden: a hidden window would keep Kleari running with no way back
        // once the main window closes.
        if let Some(popover) = app.get_webview_window(POPOVER) {
            let _ = popover.destroy();
        }
        // With no menu-bar item, a hidden window would leave Kleari unreachable.
        show_main(app);
    }
    Ok(())
}

/// `apply` from a command thread: runs it on the main thread and waits for the outcome.
pub fn set(app: &AppHandle, on: bool) -> Result<(), CommandError> {
    if active(app) == on {
        return Ok(());
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        // The receiver waits below, so the send only fails if it has already given up.
        let _ = sender.send(apply(&handle, on).is_ok());
    })
    .map_err(|_| CommandError::from(CommandErrorCode::MenuBarUnavailable))?;
    match receiver.recv() {
        Ok(true) => Ok(()),
        _ => Err(CommandErrorCode::MenuBarUnavailable.into()),
    }
}

/// The popover window, created hidden the first time it is needed.
fn popover(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(POPOVER) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("index.html".into()))
        .title("Kleari")
        .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        // The system material behind the page, as in the main window (ADR-008): Liquid Glass
        // on macOS 26 and later, the popover material before.
        .effects(
            EffectsBuilder::new()
                .effects([Effect::LiquidGlassRegular, Effect::Popover])
                .state(EffectState::Active)
                .radius(POPOVER_RADIUS)
                .build(),
        )
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
}

/// Opens the popover below the menu-bar icon, or closes it if it is open.
fn toggle_popover(app: &AppHandle, icon: Rect) {
    let Ok(window) = popover(app) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    if just_closed(app) {
        return;
    }
    let (icon, screen) = icon_on_screen(app, icon, window.scale_factor().unwrap_or(1.0));
    let centre = icon.x + icon.width / 2.0;
    let x = popover_x(centre, POPOVER_WIDTH, SCREEN_MARGIN, screen);
    let y = icon.y + icon.height + 4.0;
    // Opening the popover is best effort: if macOS refuses to place or show it, the icon
    // stays and the next click tries again.
    let _ = window.set_position(LogicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
}

/// A rectangle in points (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Area {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Area {
    fn contains(&self, x: f64, y: f64) -> bool {
        (self.x..self.x + self.width).contains(&x) && (self.y..self.y + self.height).contains(&y)
    }
}

/// The icon in points, and the `(left, right)` edges of the screen it is on. The icon's
/// pixels are in its own screen's scale, so each screen is tried in its own scale; with no
/// match, `fallback` (the popover's scale) is used and the edges are unknown.
fn icon_on_screen(app: &AppHandle, icon: Rect, fallback: f64) -> (Area, Option<(f64, f64)>) {
    let in_points = |scale: f64| {
        let position = icon.position.to_logical::<f64>(scale);
        let size = icon.size.to_logical::<f64>(scale);
        Area {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    };
    let screens = app.available_monitors().unwrap_or_default();
    for monitor in &screens {
        let scale = monitor.scale_factor();
        let position = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);
        let screen = Area {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        };
        let area = in_points(scale);
        if screen.contains(area.x + area.width / 2.0, area.y) {
            return (area, Some((screen.x, screen.x + screen.width)));
        }
    }
    (in_points(fallback), None)
}

/// The popover's left edge: centred under the icon, kept `margin` inside the screen's
/// `(left, right)` edges when they are known.
fn popover_x(centre: f64, width: f64, margin: f64, screen: Option<(f64, f64)>) -> f64 {
    let x = centre - width / 2.0;
    match screen {
        Some((left, right)) => x.min(right - margin - width).max(left + margin),
        None => x.max(0.0),
    }
}

/// Brings the main window back (after it was closed into the menu bar).
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Where the popover's links go; only these destinations can be named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Destination {
    Dashboard,
    Cleanup,
    Review,
    Quarantine,
    Storage,
    System,
    Developer,
    Apps,
    Maintenance,
    Processes,
    Mouse,
    Activity,
    Settings,
}

impl Destination {
    pub const fn route(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Cleanup => "cleanup",
            Self::Review => "review",
            Self::Quarantine => "quarantine",
            Self::Storage => "storage",
            Self::System => "system",
            Self::Developer => "developer",
            Self::Apps => "apps",
            Self::Maintenance => "maintenance",
            Self::Processes => "processes",
            Self::Mouse => "mouse",
            Self::Activity => "activity",
            Self::Settings => "settings",
        }
    }
}

/// Opens the main window at `destination` and closes the popover.
#[tauri::command]
pub fn open_main(app: AppHandle, destination: Destination) -> Result<(), CommandError> {
    open_at(&app, destination)
}

/// Brings the main window forward at `destination` (from the popover, or a clicked
/// notification), closing the popover.
pub fn open_at(app: &AppHandle, destination: Destination) -> Result<(), CommandError> {
    if let Some(popover) = app.get_webview_window(POPOVER) {
        let _ = popover.hide();
    }
    let window = app
        .get_webview_window(MAIN)
        .ok_or_else(|| CommandError::from(CommandErrorCode::SettingsUnavailable))?;
    // The route is one of the fixed names above, never text from the page.
    window
        .eval(format!("window.location.hash = '#{}'", destination.route()))
        .map_err(|_| CommandError::from(CommandErrorCode::SettingsUnavailable))?;
    show_main(app);
    Ok(())
}

/// Closes the popover (Escape, or a click elsewhere).
#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    if let Some(popover) = app.get_webview_window(POPOVER) {
        let _ = popover.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_destinations_are_accepted() {
        assert_eq!(
            serde_json::from_str::<Destination>(r#""quarantine""#).ok(),
            Some(Destination::Quarantine)
        );
        assert_eq!(Destination::Review.route(), "review");
        assert!(serde_json::from_str::<Destination>(r#""'; alert(1); '""#).is_err());
        assert!(serde_json::from_str::<Destination>(r#""preview""#).is_err());
    }

    #[test]
    fn the_popover_stays_on_screen() {
        let screen = Some((0.0, 1000.0));
        // Centred under an icon in the middle of the menu bar.
        assert_eq!(popover_x(500.0, 340.0, 8.0, screen), 330.0);
        // Pulled in from the right edge, and from the left.
        assert_eq!(popover_x(990.0, 340.0, 8.0, screen), 652.0);
        assert_eq!(popover_x(20.0, 340.0, 8.0, screen), 8.0);
        // A second screen to the left of the main one.
        assert_eq!(popover_x(-30.0, 340.0, 8.0, Some((-1200.0, 0.0))), -348.0);
        assert_eq!(popover_x(100.0, 340.0, 8.0, None), 0.0);
    }

    #[test]
    fn a_screen_contains_points_on_its_left_and_top_edges_only() {
        let screen = Area {
            x: -1200.0,
            y: 0.0,
            width: 1200.0,
            height: 800.0,
        };
        assert!(screen.contains(-1200.0, 0.0));
        assert!(screen.contains(-1.0, 799.0));
        // The next screen starts where this one ends.
        assert!(!screen.contains(0.0, 0.0));
        assert!(!screen.contains(-600.0, 800.0));
    }
}
