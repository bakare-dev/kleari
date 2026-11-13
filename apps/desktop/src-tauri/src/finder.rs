//! Shows items in Finder (selected in a Finder window). Paths come from Rust's own records,
//! never from the frontend; revealing opens or changes nothing.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealFailed;

/// Selects `paths` in Finder. Must be called off the main thread; waits for AppKit.
#[cfg(target_os = "macos")]
pub fn reveal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: Vec<PathBuf>,
) -> Result<(), RevealFailed> {
    if paths.is_empty() {
        return Err(RevealFailed);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::reveal(&paths));
    })
    .map_err(|_| RevealFailed)?;
    if receiver.recv().map_err(|_| RevealFailed)? {
        Ok(())
    } else {
        Err(RevealFailed)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reveal<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _paths: Vec<PathBuf>,
) -> Result<(), RevealFailed> {
    Err(RevealFailed)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSURL};

    pub fn reveal(paths: &[PathBuf]) -> bool {
        let Some(urls) = paths
            .iter()
            .map(NSURL::from_file_path)
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        NSWorkspace::sharedWorkspace()
            .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&urls));
        true
    }
}
