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
    /// Privacy & Security › Automation, which Browser care and Empty Trash need.
    Automation,
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
            Self::Automation => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
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

/// Kleari's release page for `version`, built here from a fixed address: the page names only a
/// version, which must look like one (`0.1.0` or `0.1.0-beta.3`), and never a URL.
pub fn release_page(version: &str) -> Option<String> {
    let (core, pre) = version
        .split_once('-')
        .map_or((version, None), |(core, pre)| (core, Some(pre)));
    let numbers: Vec<&str> = core.split('.').collect();
    let digits = |part: &str| {
        !part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit())
    };
    let core_ok = numbers.len() == 3 && numbers.iter().all(|part| digits(part));
    let pre_ok = pre.is_none_or(|pre| {
        pre.split_once('.').is_some_and(|(label, number)| {
            matches!(label, "alpha" | "beta" | "rc") && digits(number)
        })
    });
    (core_ok && pre_ok)
        .then(|| format!("https://github.com/bakare-dev/kleari/releases/tag/v{version}"))
}

/// Kleari's legal and policy documents, which Settings › About links to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegalDocument {
    License,
    Disclaimer,
    Privacy,
    Security,
    ThirdPartyNotices,
}

impl LegalDocument {
    const fn file(self) -> &'static str {
        match self {
            Self::License => "LICENSE",
            Self::Disclaimer => "DISCLAIMER.md",
            Self::Privacy => "PRIVACY.md",
            Self::Security => "SECURITY.md",
            Self::ThirdPartyNotices => "THIRD_PARTY_NOTICES.md",
        }
    }
}

/// `document` on GitHub as it is in this version's release (its tag), so the notices match
/// what this copy ships; a version that isn't a release's (a development build) gets `main`.
pub fn legal_page(document: LegalDocument, version: &str) -> String {
    let tree = if release_page(version).is_some() {
        format!("v{version}")
    } else {
        "main".to_owned()
    };
    format!(
        "https://github.com/bakare-dev/kleari/blob/{tree}/{}",
        document.file()
    )
}

/// Opens Kleari's release page for `version` in the default browser.
pub fn open_release_page<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    version: &str,
) -> Result<(), OpenFailed> {
    open_page(app, release_page(version).ok_or(OpenFailed)?)
}

/// Opens one of Kleari's legal documents, for the running version, in the default browser.
pub fn open_legal_document<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    document: LegalDocument,
) -> Result<(), OpenFailed> {
    open_page(
        app,
        legal_page(document, &app.package_info().version.to_string()),
    )
}

/// Opens one of the addresses built above in the default browser (never one from the page).
#[cfg(target_os = "macos")]
fn open_page<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: String) -> Result<(), OpenFailed> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::open_url(&url));
    })
    .map_err(|_| OpenFailed)?;
    if receiver.recv().map_err(|_| OpenFailed)? {
        Ok(())
    } else {
        Err(OpenFailed)
    }
}

#[cfg(not(target_os = "macos"))]
fn open_page<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _url: String,
) -> Result<(), OpenFailed> {
    Err(OpenFailed)
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
    fn only_a_version_makes_a_release_page() {
        assert_eq!(
            release_page("0.1.0-beta.3").as_deref(),
            Some("https://github.com/bakare-dev/kleari/releases/tag/v0.1.0-beta.3")
        );
        assert!(release_page("1.2.0").is_some());
        for bad in [
            "",
            "1.2",
            "1.2.0.4",
            "v1.2.0",
            "1.2.0-beta",
            "1.2.0-nightly.1",
            "1.2.0/../../evil",
            "1.2.0-beta.3?x=1",
            "https://example.com",
        ] {
            assert_eq!(release_page(bad), None, "{bad}");
        }
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

    #[test]
    fn a_legal_document_opens_as_this_release_has_it() {
        assert_eq!(
            legal_page(LegalDocument::ThirdPartyNotices, "0.1.0-beta.4"),
            "https://github.com/bakare-dev/kleari/blob/v0.1.0-beta.4/THIRD_PARTY_NOTICES.md"
        );
        assert_eq!(
            legal_page(LegalDocument::License, "1.2.3"),
            "https://github.com/bakare-dev/kleari/blob/v1.2.3/LICENSE"
        );
        // Not a release's version: the current documents.
        assert_eq!(
            legal_page(LegalDocument::Disclaimer, "0.1.0-dev"),
            "https://github.com/bakare-dev/kleari/blob/main/DISCLAIMER.md"
        );
    }
}
