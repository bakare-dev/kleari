//! Opens a fixed pane of macOS System Settings for the user. The frontend names a pane from
//! this enum and never supplies a URL; Kleari changes no setting itself.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsPane {
    SoftwareUpdate,
    LoginItems,
    FullDiskAccess,
    /// Privacy & Security › Files and Folders, where folder access can be turned back on.
    FilesAndFolders,
    /// Notifications, where Kleari's notifications can be allowed after a refusal.
    Notifications,
    /// Privacy & Security › Accessibility, which the Mouse module needs.
    Accessibility,
    /// Privacy & Security › App Management, which removing another app needs.
    AppManagement,
}

impl SettingsPane {
    // Only the macOS opener uses it; on other targets (Linux CI) it is exercised by tests only.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub const fn url(self) -> &'static str {
        match self {
            Self::SoftwareUpdate => {
                "x-apple.systempreferences:com.apple.Software-Update-Settings.extension"
            }
            Self::LoginItems => "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
            Self::FullDiskAccess => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
            }
            Self::FilesAndFolders => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders"
            }
            Self::Notifications => {
                "x-apple.systempreferences:com.apple.Notifications-Settings.extension"
            }
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Self::AppManagement => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFailed;

/// Asks macOS to open the pane. Must be called off the main thread; waits for the result.
#[cfg(target_os = "macos")]
pub fn open<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pane: SettingsPane,
) -> Result<(), OpenFailed> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::open_url(pane.url()));
    })
    .map_err(|_| OpenFailed)?;
    if receiver.recv().map_err(|_| OpenFailed)? {
        Ok(())
    } else {
        Err(OpenFailed)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _pane: SettingsPane,
) -> Result<(), OpenFailed> {
    Err(OpenFailed)
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    pub fn open_url(url: &str) -> bool {
        let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return false;
        };
        NSWorkspace::sharedWorkspace().openURL(&url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_deserialize_from_fixed_names_only() {
        assert_eq!(
            serde_json::from_str::<SettingsPane>(r#""softwareUpdate""#).ok(),
            Some(SettingsPane::SoftwareUpdate)
        );
        assert_eq!(
            serde_json::from_str::<SettingsPane>(r#""loginItems""#).ok(),
            Some(SettingsPane::LoginItems)
        );
        assert_eq!(
            serde_json::from_str::<SettingsPane>(r#""fullDiskAccess""#).ok(),
            Some(SettingsPane::FullDiskAccess)
        );
        assert_eq!(
            serde_json::from_str::<SettingsPane>(r#""filesAndFolders""#).ok(),
            Some(SettingsPane::FilesAndFolders)
        );
        // A URL or any other value is rejected before reaching AppKit.
        assert!(serde_json::from_str::<SettingsPane>(
            r#""x-apple.systempreferences:com.apple.anything""#
        )
        .is_err());
        assert!(serde_json::from_str::<SettingsPane>(r#""privacy""#).is_err());
    }

    #[test]
    fn every_pane_opens_a_system_settings_url() {
        for pane in [
            SettingsPane::SoftwareUpdate,
            SettingsPane::LoginItems,
            SettingsPane::FullDiskAccess,
            SettingsPane::FilesAndFolders,
            SettingsPane::Notifications,
            SettingsPane::Accessibility,
        ] {
            assert!(pane
                .url()
                .starts_with("x-apple.systempreferences:com.apple."));
        }
    }
}
