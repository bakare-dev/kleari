//! Uninstalling Kleari from Settings: everything Kleari put on this Mac goes to the Trash
//! (so it can still be recovered until the Trash is emptied), never deleted outright. It
//! refuses while Quarantine holds anything, because those items are the user's files. An app
//! can't reset the privacy permissions it was given; the page says how.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// What Kleari keeps on this Mac, besides the app itself, for the bundle `identifier`.
pub fn data_locations(home: &Path, identifier: &str) -> Vec<PathBuf> {
    let library = home.join("Library");
    vec![
        library.join("Application Support").join(identifier),
        library.join("Caches").join(identifier),
        library.join("WebKit").join(identifier),
        library.join("HTTPStorages").join(identifier),
        library
            .join("Saved Application State")
            .join(format!("{identifier}.savedState")),
        library
            .join("Preferences")
            .join(format!("{identifier}.plist")),
    ]
}

/// The running app's own bundle, if it is one (a `.app` folder), so it can go to the Trash
/// too. `None` in development, where there is no bundle to remove.
pub fn app_bundle(executable: &Path) -> Option<PathBuf> {
    // <bundle>.app/Contents/MacOS/<executable>
    let bundle = executable.parent()?.parent()?.parent()?;
    let is_app = bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        && executable.parent()?.file_name()? == "MacOS"
        && executable.parent()?.parent()?.file_name()? == "Contents";
    is_app.then(|| bundle.to_path_buf())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uninstalled {
    /// What was moved to the Trash, shown as `~/…`.
    pub moved: Vec<String>,
    /// What existed but couldn't be moved, shown as `~/…`.
    pub failed: Vec<String>,
    /// Open at login was on and macOS didn't turn it off; its reason, when it gave one.
    pub login_item_kept: Option<String>,
}

/// Moves each existing location to the Trash. Only locations that don't exist are skipped;
/// one that can't even be checked is reported as not moved.
pub fn move_to_trash(locations: &[PathBuf], home: Option<&Path>) -> Uninstalled {
    let shown = |path: &Path| crate::system_monitor::display_path(path, home);
    let mut done = Uninstalled {
        moved: Vec::new(),
        failed: Vec::new(),
        login_item_kept: None,
    };
    for location in locations {
        match std::fs::symlink_metadata(location) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                done.failed.push(shown(location));
                continue;
            }
            Ok(_) => {}
        }
        if trash(location) {
            done.moved.push(shown(location));
        } else {
            done.failed.push(shown(location));
        }
    }
    done
}

/// Moves `path` to the Trash through macOS, which keeps "Put Back" working.
#[cfg(target_os = "macos")]
pub fn trash(path: &Path) -> bool {
    use objc2_foundation::{NSFileManager, NSURL};
    let Some(url) = NSURL::from_file_path(path) else {
        return false;
    };
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .is_ok()
}

#[cfg(not(target_os = "macos"))]
pub fn trash(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_klearis_own_locations_are_listed() {
        let home = Path::new("/Users/someone");
        let locations = data_locations(home, "dev.bakare.kleari");
        assert!(locations
            .iter()
            .all(|path| path.starts_with("/Users/someone/Library")));
        assert!(locations
            .iter()
            .all(|path| path.to_string_lossy().contains("dev.bakare.kleari")));
        assert!(locations.contains(&PathBuf::from(
            "/Users/someone/Library/Application Support/dev.bakare.kleari"
        )));
        assert!(locations.contains(&PathBuf::from(
            "/Users/someone/Library/Preferences/dev.bakare.kleari.plist"
        )));
    }

    #[test]
    fn the_app_itself_is_found_only_when_running_from_its_bundle() {
        assert_eq!(
            app_bundle(Path::new("/Applications/Kleari.app/Contents/MacOS/kleari")),
            Some(PathBuf::from("/Applications/Kleari.app"))
        );
        // A development build has no bundle to remove.
        assert_eq!(
            app_bundle(Path::new("/Users/someone/code/target/debug/kleari")),
            None
        );
        assert_eq!(
            app_bundle(Path::new("/Applications/Other/Contents/MacOS/kleari")),
            None
        );
        assert_eq!(
            app_bundle(Path::new("/Applications/Kleari.app/Resources/x/kleari")),
            None
        );
    }

    #[test]
    fn missing_locations_are_skipped_not_reported() {
        let done = move_to_trash(&[PathBuf::from("/nonexistent/kleari-test-location")], None);
        assert!(done.moved.is_empty() && done.failed.is_empty());
    }

    #[test]
    fn a_location_that_cant_be_checked_is_reported_not_skipped() {
        // The OS refuses to look a path with a NUL byte up at all (not "not found").
        let done = move_to_trash(&[PathBuf::from("/nonexistent/kleari\0test")], None);
        assert!(done.moved.is_empty());
        assert_eq!(done.failed.len(), 1);
    }
}
