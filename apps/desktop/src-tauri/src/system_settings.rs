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

/// `text` for a URL's query: everything but unreserved characters is percent-encoded.
fn query_text(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// A new GitHub issue on Kleari's repository, pre-filled with the version, macOS version and
/// kind of chip, and empty sections to describe the problem. Nothing else about the Mac, and
/// never the log: the user attaches that themselves if they choose.
pub fn issue_page(version: &str, macos: &str, chip: &str) -> String {
    let body = format!(
        "**What happened**\n\n\n**What you expected**\n\n\n**Steps to reproduce**\n1. \n\n---\n\
         Kleari {version} \u{b7} macOS {macos} \u{b7} {chip}\n\n\
         Kleari showed its log file in Finder (~/Library/Logs/Kleari/kleari.log). You can attach \
         it here if you like: it records what Kleari did and errors, never file names, paths or \
         what you type. Look it over first."
    );
    format!(
        "https://github.com/bakare-dev/kleari/issues/new?body={}",
        query_text(&body)
    )
}

/// This Mac's macOS version and kind of chip, for the issue page.
fn mac_summary() -> (String, &'static str) {
    let macos = crate::system_monitor::sysctl::string("kern.osproductversion")
        .filter(|version| {
            version.len() <= 16 && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let chip = if crate::system_monitor::sysctl::integer("hw.optional.arm64") == Some(1) {
        "Apple silicon"
    } else {
        "Intel"
    };
    (macos, chip)
}

/// Opens a new, pre-filled GitHub issue in the default browser (`issue_page`).
pub fn open_issue_page<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), OpenFailed> {
    let (macos, chip) = mac_summary();
    open_page(
        app,
        issue_page(&app.package_info().version.to_string(), &macos, chip),
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
    fn the_issue_page_is_klearis_own_and_names_only_version_macos_and_chip() {
        let page = issue_page("0.1.0-beta.4", "15.2", "Apple silicon");
        let prefix = "https://github.com/bakare-dev/kleari/issues/new?body=";
        assert!(page.starts_with(prefix));
        let query = &page[prefix.len()..];
        // Fully encoded: nothing in the body can add another parameter or leave the page.
        assert!(query
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)));
        assert!(query
            .contains("Kleari%200.1.0-beta.4%20%C2%B7%20macOS%2015.2%20%C2%B7%20Apple%20silicon"));
        assert!(query.contains("What%20happened"));
        // Well under the length browsers and GitHub accept for an address.
        assert!(page.len() < 2000);
    }

    #[test]
    fn query_text_keeps_only_unreserved_characters() {
        assert_eq!(query_text("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }

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
