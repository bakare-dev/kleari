//! Updates from Kleari's GitHub releases (ADR-014). Checking asks GitHub for the latest
//! release's `latest.json` (version, notes, download link and signature) and sends nothing
//! about this Mac beyond what any download sends. An update installs only after you choose
//! Install, only if it is signed with Kleari's updater key, and never while a quarantine
//! move, restore or delete is running. Kleari then restarts into the new version.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::{CommandError, CommandErrorCode};
use crate::history::History;
use crate::quarantine::QuarantineState;

/// Release notes longer than this are cut; the release page has them in full.
const MAX_NOTES: usize = 1000;
/// Where Kleari's release assets are served from, on github.com.
const RELEASE_DOWNLOADS: &str = "/bakare-dev/kleari/releases/download/";
/// Stable releases only: GitHub's latest release is never a pre-release, and betas are
/// published as pre-releases.
const STABLE_FEED: &str =
    "https://github.com/bakare-dev/kleari/releases/latest/download/latest.json";
/// The newest of the beta and stable releases, kept on the rolling `beta-channel` release
/// (docs/release-checklist.md).
const BETA_FEED: &str =
    "https://github.com/bakare-dev/kleari/releases/download/beta-channel/latest.json";

/// The manifest to check: betas too when Settings › About › Beta updates is on.
fn feed(beta: bool) -> &'static str {
    if beta {
        BETA_FEED
    } else {
        STABLE_FEED
    }
}

/// The update the last check found, kept so Install applies exactly what was shown.
#[derive(Default)]
pub struct UpdateState {
    found: Mutex<Option<Update>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateCheck {
    UpToDate {
        current: String,
    },
    Available {
        current: String,
        version: String,
        /// The release notes, shortened to `MAX_NOTES` characters.
        notes: Option<String>,
    },
    /// The channel checked has no release yet (no stable release before 1.0, or no beta), so
    /// there is nothing to compare with: not a failure.
    NoRelease {
        current: String,
        /// Whether betas were checked.
        beta: bool,
    },
}

fn failed<E>(_: E) -> CommandError {
    CommandErrorCode::UpdateFailed.into()
}

fn shortened(notes: &str) -> Option<String> {
    let notes = notes.trim();
    if notes.is_empty() {
        return None;
    }
    Some(match notes.char_indices().nth(MAX_NOTES) {
        Some((end, _)) => format!("{}…", notes[..end].trim_end()),
        None => notes.to_owned(),
    })
}

/// Asks GitHub whether a newer Kleari is out: a stable release, or with betas included (as
/// chosen, or by default on a beta build), the newest beta or stable. Never an older version,
/// so turning betas off waits for the next stable release rather than going back.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateCheck, CommandError> {
    require_app()?;
    let current = app.package_info().version.to_string();
    let beta = crate::preferences::load(&app.state::<History>().db)
        .preferences
        .gets_betas();
    let endpoint = Url::parse(feed(beta)).map_err(failed)?;
    let checked = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(failed)?
        .build()
        .map_err(failed)?
        .check()
        .await;
    let found = match checked {
        Ok(found) => found,
        // GitHub answered, but the channel has no manifest: nothing released on it yet.
        // Network and signature failures stay failures.
        Err(tauri_plugin_updater::Error::ReleaseNotFound) => {
            *app.state::<UpdateState>().found.lock().map_err(failed)? = None;
            return Ok(UpdateCheck::NoRelease { current, beta });
        }
        Err(error) => return Err(failed(error)),
    };
    // An update whose download isn't one of Kleari's release assets is never offered.
    if found
        .as_ref()
        .is_some_and(|update| !from_our_releases(&update.download_url))
    {
        return Err(CommandErrorCode::UpdateFailed.into());
    }
    let check = match &found {
        Some(update) => UpdateCheck::Available {
            current,
            version: update.version.clone(),
            notes: update.body.as_deref().and_then(shortened),
        },
        None => UpdateCheck::UpToDate { current },
    };
    *app.state::<UpdateState>().found.lock().map_err(failed)? = found;
    Ok(check)
}

/// Downloads the update the last check found, checks its signature, installs it and restarts.
/// Refused while a quarantine operation runs; nothing changes if any step fails.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), CommandError> {
    require_app()?;
    let update = app
        .state::<UpdateState>()
        .found
        .lock()
        .map_err(failed)?
        .clone()
        .ok_or_else(|| CommandError::from(CommandErrorCode::UpdateFailed))?;
    if !from_our_releases(&update.download_url) {
        return Err(CommandErrorCode::UpdateFailed.into());
    }
    let bytes = update.download(|_, _| {}, || {}).await.map_err(failed)?;
    let operations = app.state::<QuarantineState>();
    // Held until the process restarts, so no move, restore or delete can start in between.
    let _guard = operations
        .begin()
        .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
    // Verifies the signature against tauri.conf.json's key (and, with requireSignedVersion,
    // that it was signed for the announced version) before replacing the app.
    update.install(&bytes).map_err(failed)?;
    app.restart()
}

/// Updates replace the running app's bundle, so they run only from an installed Kleari.app.
/// A development build has none: the updater would replace the folder its executable is in
/// (Cargo's `target/debug`).
fn require_app() -> Result<(), CommandError> {
    std::env::current_exe()
        .ok()
        .and_then(|executable| crate::uninstall::app_bundle(&executable))
        .map(|_| ())
        .ok_or_else(|| CommandErrorCode::UpdateNeedsApp.into())
}

/// Whether a download link points at an asset of Kleari's own GitHub releases, over HTTPS.
/// GitHub then redirects to its download host; the signature check covers the bytes.
fn from_our_releases(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.path().starts_with(RELEASE_DOWNLOADS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_trimmed_and_cut_at_a_character() {
        assert_eq!(shortened("  \n"), None);
        assert_eq!(shortened(" Fixes.\n"), Some("Fixes.".to_owned()));
        let long = "é".repeat(MAX_NOTES + 5);
        let cut = shortened(&long).unwrap_or_default();
        assert_eq!(cut.chars().count(), MAX_NOTES + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn only_our_release_assets_are_downloaded() {
        let ok = |url: &str| url.parse::<Url>().is_ok_and(|url| from_our_releases(&url));
        assert!(ok("https://github.com/bakare-dev/kleari/releases/download/v0.1.0-beta.3/Kleari.app.tar.gz"));
        for url in [
            "http://github.com/bakare-dev/kleari/releases/download/v1/Kleari.app.tar.gz",
            "https://github.com:8443/bakare-dev/kleari/releases/download/v1/Kleari.app.tar.gz",
            "https://github.com/someone/kleari/releases/download/v1/Kleari.app.tar.gz",
            "https://github.com.evil.example/bakare-dev/kleari/releases/download/v1/x.tar.gz",
            "https://evil.example/bakare-dev/kleari/releases/download/v1/x.tar.gz",
            "https://github.com/bakare-dev/kleari/archive/v1.tar.gz",
            "https://github.com/bakare-dev/kleari-fork/releases/download/v1/x.tar.gz",
        ] {
            assert!(!ok(url), "{url}");
        }
    }

    #[test]
    fn both_feeds_are_kleari_release_manifests_and_betas_are_opt_in() {
        for beta in [false, true] {
            let url: Url = feed(beta).parse().unwrap_or_else(|_| panic!("feed {beta}"));
            assert_eq!(
                (url.scheme(), url.host_str(), url.port()),
                ("https", Some("github.com"), None)
            );
            assert!(url.path().starts_with("/bakare-dev/kleari/releases/"));
            assert!(url.path().ends_with("/latest.json"));
        }
        assert_eq!(feed(false), STABLE_FEED);
        assert_ne!(feed(true), feed(false));
    }

    #[test]
    fn betas_follow_the_installed_version_until_chosen() {
        use crate::preferences::Preferences;
        let unchosen = Preferences::default();
        assert_eq!(unchosen.beta_updates, None);
        // This build's own version decides until the user chooses.
        assert_eq!(
            unchosen.gets_betas(),
            env!("CARGO_PKG_VERSION").contains('-')
        );
        for chosen in [true, false] {
            let preferences = Preferences {
                beta_updates: Some(chosen),
                ..Preferences::default()
            };
            assert_eq!(preferences.gets_betas(), chosen);
        }
        let none_yet = serde_json::to_value(UpdateCheck::NoRelease {
            current: "0.1.0-beta.1".to_owned(),
            beta: false,
        })
        .unwrap_or_default();
        assert_eq!(
            none_yet,
            serde_json::json!({"status": "noRelease", "current": "0.1.0-beta.1", "beta": false})
        );
    }

    #[test]
    fn a_check_serialises_with_its_status() {
        let up_to_date = serde_json::to_value(UpdateCheck::UpToDate {
            current: "0.1.0-beta.2".to_owned(),
        })
        .unwrap_or_default();
        assert_eq!(
            up_to_date,
            serde_json::json!({"status": "upToDate", "current": "0.1.0-beta.2"})
        );
        let available = serde_json::to_value(UpdateCheck::Available {
            current: "0.1.0-beta.2".to_owned(),
            version: "0.1.0-beta.3".to_owned(),
            notes: None,
        })
        .unwrap_or_default();
        assert_eq!(
            available,
            serde_json::json!({"status": "available", "current": "0.1.0-beta.2", "version": "0.1.0-beta.3", "notes": null})
        );
    }
}
