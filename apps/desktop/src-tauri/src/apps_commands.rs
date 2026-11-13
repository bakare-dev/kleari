//! Commands for the Apps page (ADR-017): a read-only scan, sending leftovers and installers to
//! Cleanup's review, and removing an app, which moves its bundle to the Trash through macOS
//! and sends what it left in the Library to the review. Nothing here deletes anything; the
//! leftovers move only after the user approves a preview, into quarantine.

use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::apps::{self, AppsScan, AppsState, ScanBusy, ScanStatus};
use crate::cleanup::intake::Intake;
use crate::cleanup::CleanupState;
use crate::cleanup_commands::{blocking_failed, check_ids, home};
use crate::commands::{CommandError, CommandErrorCode};
use crate::history::History;
use crate::persistence::{Event, EventKind};
use crate::system_monitor::open_paths::OpenPaths;
use crate::system_monitor::running_apps;

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

/// Lists installed apps, leftovers of apps that are gone and installers, read-only.
#[tauri::command]
pub async fn start_apps_scan(app: AppHandle) -> Result<AppsScan, CommandError> {
    let cancel = app
        .state::<AppsState>()
        .begin()
        .map_err(|ScanBusy| CommandError::from(CommandErrorCode::AppsScanInProgress))?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = crate::storage::home_dir();
        // Without the running apps, none is shown as open; removing reads them again and
        // refuses to go on without them.
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        let (result, bundles, review) = apps::scan(
            &apps::app_folders(home.as_deref()),
            home.as_deref(),
            &running,
            unix_time_ms(),
            &cancel,
        );
        let completed = result.status == ScanStatus::Completed;
        app.state::<AppsState>()
            .finish(completed.then_some((bundles, review)));
        result
    })
    .await
    .map_err(|_| CommandError::from(CommandErrorCode::AppsScanFailed))
}

#[tauri::command]
pub fn cancel_apps_scan(state: State<'_, AppsState>) {
    state.cancel();
}

/// Sends leftovers and installers from the last scan to Cleanup's review. Returns the IDs sent.
#[tauri::command]
pub async fn send_app_items_to_review(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        let history = app.state::<History>();
        let excluded = history.exclusions()?;
        let items = app.state::<AppsState>().review_items(&ids);
        let sent: Vec<String> = items.iter().map(|item| item.id.clone()).collect();
        let intake = Intake {
            home: &home,
            running: &running,
            // Leftovers and installers belong to no project.
            open: Some(&OpenPaths::NOTHING),
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let candidates = intake.app_items(items);
        note_sent(&history, &candidates);
        app.state::<CleanupState>().add_sent(candidates);
        Ok(sent)
    })
    .await
    .map_err(blocking_failed)?
}

fn note_sent(history: &History, candidates: &[crate::cleanup::Candidate]) {
    if candidates.is_empty() {
        return;
    }
    let bytes = candidates
        .iter()
        .fold(0_u64, |total, item| total.saturating_add(item.bytes));
    let count = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
    let mut event = Event::new(EventKind::Review, unix_time_ms(), "completed", count, bytes);
    event.subject = Some("apps".to_owned());
    history.note(&event);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum RemoveOutcome {
    /// In the Trash; its leftovers (if any) are in Cleanup's review.
    #[serde(rename_all = "camelCase")]
    Removed { leftovers: u64, leftover_bytes: u64 },
    /// It's open: quit it first.
    Running,
    /// Part of macOS, or not where apps are installed: never removed.
    Refused,
    /// It changed or went away since the scan: scan again.
    Changed,
    /// macOS didn't move it (usually App Management permission, or an admin-owned app).
    Failed,
}

/// Moves an app from the last scan to the Trash, then sends its leftovers to the review.
#[tauri::command]
pub async fn remove_app(app: AppHandle, id: String) -> Result<RemoveOutcome, CommandError> {
    check_ids(std::slice::from_ref(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let state = app.state::<AppsState>();
        let Some(bundle) = state.bundle(&id) else {
            return Ok(RemoveOutcome::Changed);
        };
        if bundle.apple || !apps::bundle_in_bounds(&bundle.path, &apps::app_folders(Some(&home))) {
            return Ok(RemoveOutcome::Refused);
        }
        // Read fresh; without it nothing is removed.
        let running =
            running_apps::running_bundles(&app).map_err(|running_apps::Unavailable| {
                CommandError::from(CommandErrorCode::CleanupUnavailable)
            })?;
        let open = bundle.bundle_id.as_deref().is_some_and(|bundle_id| {
            running
                .iter()
                .any(|app| app.id.eq_ignore_ascii_case(bundle_id))
        });
        if open {
            return Ok(RemoveOutcome::Running);
        }
        if !same_bundle(&bundle) {
            return Ok(RemoveOutcome::Changed);
        }
        if !crate::uninstall::trash(&bundle.path) {
            return Ok(RemoveOutcome::Failed);
        }
        state.forget(&id);
        let history = app.state::<History>();
        let mut event = Event::new(EventKind::AppRemoved, unix_time_ms(), "completed", 1, 0);
        event.subject = Some(bundle.name.clone());
        history.note(&event);

        let cancel = AtomicBool::new(false);
        let items = apps::leftovers_for_removed(&bundle, &home, unix_time_ms(), &cancel);
        let excluded = history.exclusions()?;
        let intake = Intake {
            home: &home,
            running: &running,
            open: Some(&OpenPaths::NOTHING),
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let candidates = intake.app_items(items);
        let leftover_bytes = candidates
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        let leftovers = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
        note_sent(&history, &candidates);
        app.state::<CleanupState>().add_sent(candidates);
        Ok(RemoveOutcome::Removed {
            leftovers,
            leftover_bytes,
        })
    })
    .await
    .map_err(blocking_failed)?
}

/// Whether the bundle is still the folder the scan found (same device and inode), read
/// without following symlinks.
fn same_bundle(bundle: &apps::Bundle) -> bool {
    let (Some(parent), Some(name)) = (bundle.path.parent(), bundle.path.file_name()) else {
        return false;
    };
    let Ok(name) = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(name)) else {
        return false;
    };
    crate::developer::open_absolute(parent)
        .and_then(|(dir, _)| dir.stat_child(&name).ok())
        .is_some_and(|now| now.is_dir() && now.dev == bundle.stat.dev && now.ino == bundle.stat.ino)
}
