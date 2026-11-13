//! Native folder picker, opened from Rust so the frontend never supplies a path.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerError {
    /// The panel could not be shown (not on macOS, or the main thread was unavailable).
    Unavailable,
}

/// Shows the macOS folder panel and returns the chosen folder, or `None` if cancelled.
/// Must be called off the main thread; it blocks until the user closes the panel.
#[cfg(target_os = "macos")]
pub fn pick_folder<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<PathBuf>, PickerError> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only disappears if the waiting command was dropped; nothing to report to.
        let _ = sender.send(macos::run_panel());
    })
    .map_err(|_| PickerError::Unavailable)?;
    receiver.recv().map_err(|_| PickerError::Unavailable)?
}

#[cfg(not(target_os = "macos"))]
pub fn pick_folder<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
) -> Result<Option<PathBuf>, PickerError> {
    Err(PickerError::Unavailable)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::NSString;

    use super::PickerError;

    pub fn run_panel() -> Result<Option<PathBuf>, PickerError> {
        let mtm = MainThreadMarker::new().ok_or(PickerError::Unavailable)?;
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setCanCreateDirectories(false);
        panel.setPrompt(Some(&NSString::from_str("Scan")));
        panel.setMessage(Some(&NSString::from_str(
            "Choose a folder for Kleari to scan. Scanning only reads; nothing is changed.",
        )));
        if panel.runModal() != NSModalResponseOK {
            return Ok(None);
        }
        Ok(panel.URL().and_then(|url| url.to_file_path()))
    }
}
