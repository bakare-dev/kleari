//! The Mac's display: the built-in panel if there is one, otherwise the main display.
//! CoreGraphics gives the panel's physical size; AppKit gives macOS's localized name, which
//! requires the main thread.

use super::details::RawDisplay;

/// Reads the display on the main thread and waits for the result. Must be called off the
/// main thread (from a command's worker), like the folder picker.
#[cfg(target_os = "macos")]
pub fn read<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<RawDisplay> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::read());
    })
    .ok()?;
    receiver.recv().ok().flatten()
}

#[cfg(not(target_os = "macos"))]
pub fn read<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Option<RawDisplay> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_core_graphics::{
        CGDirectDisplayID, CGDisplayIsBuiltin, CGDisplayScreenSize, CGGetOnlineDisplayList,
        CGMainDisplayID,
    };
    use objc2_foundation::{NSNumber, NSString};

    use super::RawDisplay;

    const MAX_DISPLAYS: usize = 16;

    pub fn read() -> Option<RawDisplay> {
        let mtm = MainThreadMarker::new()?;
        let display = built_in_display().unwrap_or_else(|| CGMainDisplayID());
        let name = screen_name(mtm, display)?;
        let size = CGDisplayScreenSize(display);
        Some(RawDisplay {
            name,
            built_in: CGDisplayIsBuiltin(display),
            width_mm: size.width,
            height_mm: size.height,
        })
    }

    fn built_in_display() -> Option<CGDirectDisplayID> {
        let mut displays = [0 as CGDirectDisplayID; MAX_DISPLAYS];
        let mut count = 0_u32;
        // SAFETY: `displays` has room for MAX_DISPLAYS entries, which is the capacity passed;
        // CoreGraphics writes at most that many and stores the number written in `count`.
        let status = unsafe {
            CGGetOnlineDisplayList(MAX_DISPLAYS as u32, displays.as_mut_ptr(), &mut count)
        };
        if status.0 != 0 {
            return None;
        }
        let count = (count as usize).min(MAX_DISPLAYS);
        displays[..count]
            .iter()
            .copied()
            .find(|&display| CGDisplayIsBuiltin(display))
    }

    /// The localized name of the `NSScreen` whose display ID matches.
    fn screen_name(mtm: MainThreadMarker, display: CGDirectDisplayID) -> Option<String> {
        let key = NSString::from_str("NSScreenNumber");
        NSScreen::screens(mtm).iter().find_map(|screen| {
            let description = screen.deviceDescription();
            let number = description.objectForKey(&key)?;
            let number = number.downcast_ref::<NSNumber>()?;
            (number.unsignedIntValue() == display).then(|| screen.localizedName().to_string())
        })
    }
}
