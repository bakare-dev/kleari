//! IPC commands for Cleanup, Review, Preview and Quarantine, with the same conventions as
//! `commands`: stable error codes, no paths in or out, file work off the main thread.
//!
//! The frontend sends only scan options, opaque IDs and fixed enum values. Every file-changing
//! command (approve, restore, permanent delete) takes the single operation lock, and approve
//! and preview read the running apps fresh: if that read fails they fail closed.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::cleanup::execute::{self, ApproveOutcome, Checks, MoveProgress, PreviewView};
use crate::cleanup::intake::Intake;
use crate::cleanup::scan::{self, Context, Progress, StepResult};
use crate::cleanup::scope::{self, Access, ProtectedFolder, ScopeFolder};
use crate::cleanup::{CleanupState, Review, ScanInfo, ScanMode, ScanOptions};
use crate::commands::{CommandError, CommandErrorCode};
use crate::developer::docker::DockerState;
use crate::developer::DeveloperState;
use crate::history::{unix_time_ms, History};
use crate::persistence::{Event, EventKind};
use crate::preferences;
use crate::quarantine::{
    self, DeleteProgress, DeleteSummary, QuarantineList, QuarantineState, RestoreResult,
};
use crate::storage::analysis::AnalysisState;
use crate::storage::{self, picker, temp_dir};
use crate::system_monitor::running_apps::{self, RunningBundle};

/// IDs come from Rust (hex digits); anything else, or too many, is rejected.
const MAX_IDS: usize = 10_000;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn check_ids(ids: &[String]) -> Result<(), CommandError> {
    if ids.len() <= MAX_IDS && ids.iter().all(|id| valid_id(id)) {
        Ok(())
    } else {
        Err(CommandErrorCode::InvalidRequest.into())
    }
}

fn home() -> Result<PathBuf, CommandError> {
    storage::home_dir().ok_or_else(|| CommandErrorCode::CleanupUnavailable.into())
}

/// Folders the user chose in the folder panel this session (Cleanup's, Developer's, and
/// Quarantine's restore-only ones): the only folders outside the home folder a restore may go
/// back into.
fn chosen_folders(app: &AppHandle) -> Vec<PathBuf> {
    [
        app.state::<CleanupState>().added_folders(),
        app.state::<DeveloperState>().added(),
        app.state::<QuarantineState>().restore_folders(),
    ]
    .concat()
}

/// What quarantine holds, in bytes; `None` when the history can't be read.
fn held_bytes(history: &History) -> Option<u64> {
    history.db.held_items().ok().map(|items| {
        items
            .iter()
            .filter(|item| item.state == crate::persistence::ItemState::Held)
            .fold(0_u64, |total, item| total.saturating_add(item.bytes))
    })
}

/// Sends the quarantine reminder once, as the total crosses the size chosen in Settings.
fn remind_if_large(history: &History, before: Option<u64>, after: Option<u64>) {
    let preferences = preferences::load(&history.db).preferences;
    let (Some(limit), Some(before), Some(after)) = (preferences.reminder_bytes(), before, after)
    else {
        return;
    };
    if preferences.notify_quarantine && before <= limit && after > limit {
        crate::notifications::send(
            "quarantine-reminder",
            "Quarantine is getting large",
            &format!(
                "It now holds {}. Nothing is deleted until you choose; review it in Quarantine.",
                gigabytes(after)
            ),
            preferences.notify_sound,
        );
    }
}

/// Bytes as decimal gigabytes with one decimal, as Finder shows them.
fn gigabytes(bytes: u64) -> String {
    let tenths = bytes / 100_000_000;
    format!("{}.{} GB", tenths / 10, tenths % 10)
}

/// Settles records a previous run left unfinished, with everything in scope now. The caller
/// holds the operation lock, so nothing is moving meanwhile.
fn settle_interrupted(app: &AppHandle) {
    let Some(home) = storage::home_dir() else {
        return;
    };
    let (temp, added) = (temp_dir(), chosen_folders(app));
    let history = app.state::<History>();
    // Without the exclusions no original place may be looked at; settled next time.
    let Ok(excluded) = history.exclusions() else {
        return;
    };
    let scope = quarantine::Scope {
        home: &home,
        temp: temp.as_deref(),
        added: &added,
        excluded: &excluded,
    };
    // An unreadable history settles nothing; the list says so when it is read.
    let _ = quarantine::reconcile(&history.db, &history.store, Some(&scope), unix_time_ms());
}

/// Opens the macOS folder panel so items can be restored into the chosen folder this session.
/// It is a restore-only grant: the folder isn't added to any scan. It is granted only while
/// no restore or delete is running (otherwise `operationInProgress`, and nothing is granted),
/// and unfinished restores are settled straight away, now that their place may be in scope.
#[tauri::command]
pub async fn choose_restore_folder(app: AppHandle) -> Result<AddedFolder, CommandError> {
    let picker_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || picker::pick_folder(&picker_app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::PickerUnavailable))?
        .map_err(|picker::PickerError::Unavailable| {
            CommandError::from(CommandErrorCode::PickerUnavailable)
        })?;
    let Some(path) = chosen else {
        return Ok(AddedFolder::Cancelled);
    };
    let folder = std::fs::canonicalize(&path)
        .map_err(|_| CommandError::from(CommandErrorCode::RootUnreadable))?;
    if !folder.is_dir() {
        return Err(CommandErrorCode::RootUnreadable.into());
    }
    let shown = crate::system_monitor::display_path(&folder, storage::home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let operations = app.state::<QuarantineState>();
        let Ok(_guard) = operations.begin_settle() else {
            return Err(CommandError::from(CommandErrorCode::OperationInProgress));
        };
        operations.grant_restore_folder(folder);
        settle_interrupted(&app);
        Ok(AddedFolder::Added { path: shown })
    })
    .await
    .map_err(blocking_failed)?
}

fn running(app: &AppHandle) -> Result<Vec<RunningBundle>, CommandError> {
    running_apps::running_bundles(app)
        .map_err(|running_apps::Unavailable| CommandErrorCode::CleanupUnavailable.into())
}

fn blocking_failed<T>(_: T) -> CommandError {
    CommandErrorCode::CleanupUnavailable.into()
}

/// The folders each scan mode reads and whether Kleari may read them.
#[tauri::command]
pub async fn get_cleanup_scope(app: AppHandle) -> Result<Vec<ScopeFolder>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let history = app.state::<History>();
        let added = app.state::<CleanupState>().added_folders();
        Ok(scope::folders(
            &home,
            &history.db,
            &added,
            scope::has_full_disk_access(&home),
        ))
    })
    .await
    .map_err(blocking_failed)?
}

/// Reads a protected folder once so macOS asks the user for access, and remembers the
/// answer. Waits while the macOS prompt is open.
#[tauri::command]
pub async fn request_folder_access(
    app: AppHandle,
    folder: ProtectedFolder,
) -> Result<Access, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let history = app.state::<History>();
        let access = scope::request_access(&home, &history.db, folder, unix_time_ms());
        let answer = match access {
            Access::Granted | Access::ByFullDisk | Access::Available => Some("granted"),
            Access::Denied => Some("denied"),
            Access::Missing | Access::NotRequested => None,
        };
        if let Some(answer) = answer {
            let mut event = Event::new(EventKind::Access, unix_time_ms(), answer, 1, 0);
            event.subject = Some(folder.key().to_owned());
            history.note(&event);
        }
        Ok(access)
    })
    .await
    .map_err(blocking_failed)?
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AddedFolder {
    Added { path: String },
    Cancelled,
}

/// Opens the macOS folder panel to add a folder to scan (projects and large files). The
/// folder stays in Rust for the session; the frontend gets its `~/…` path.
#[tauri::command]
pub async fn add_cleanup_folder(app: AppHandle) -> Result<AddedFolder, CommandError> {
    let picker_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || picker::pick_folder(&picker_app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::PickerUnavailable))?
        .map_err(|picker::PickerError::Unavailable| {
            CommandError::from(CommandErrorCode::PickerUnavailable)
        })?;
    let Some(path) = chosen else {
        return Ok(AddedFolder::Cancelled);
    };
    let folder = std::fs::canonicalize(&path)
        .map_err(|_| CommandError::from(CommandErrorCode::RootUnreadable))?;
    if !folder.is_dir() {
        return Err(CommandErrorCode::RootUnreadable.into());
    }
    let shown = crate::system_monitor::display_path(&folder, storage::home_dir().as_deref());
    app.state::<CleanupState>().add_folder(folder);
    Ok(AddedFolder::Added { path: shown })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScanResult {
    pub mode: ScanMode,
    pub cancelled: bool,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub steps: Vec<StepResult>,
    /// macOS refused some folders or items; Full Disk Access would let Kleari read them.
    pub needs_full_disk_access: bool,
    /// Granted or added folders that couldn't be searched, shown as `~/…`.
    pub skipped_folders: Vec<String>,
    /// Docker's disk use, for the Developer scan (read-only).
    pub docker: Option<DockerState>,
}

/// Who started a scan: you, in Cleanup, or the schedule set in Settings (ADR-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOrigin {
    Cleanup,
    Schedule,
}

/// The notification a scan sends, clicked to open the review.
pub const SCAN_NOTIFICATION: &str = "scan-finished";

/// "1 item" or "12 items".
fn items(count: usize) -> String {
    match count {
        1 => "1 item".to_owned(),
        count => format!("{count} items"),
    }
}

/// Runs a read-only Cleanup scan; its findings replace the last scan's in the review. The
/// caller holds the scan lock (`CleanupState::begin_scan`, which gives `cancel`) and ends it.
pub fn run_scan(
    app: &AppHandle,
    mode: ScanMode,
    options: ScanOptions,
    cancel: &AtomicBool,
    origin: ScanOrigin,
    on_progress: impl FnMut(Progress),
) -> Result<CleanupScanResult, CommandError> {
    let home = home()?;
    // A scan without the running apps marks nothing as in use; preview and approval read
    // them again and refuse to go on without them.
    let running = running_apps::running_bundles(app).unwrap_or_default();
    let history = app.state::<History>();
    let cleanup = app.state::<CleanupState>();
    let added = cleanup.added_folders();
    let developer_added = app.state::<DeveloperState>().added();
    let large_roots = scope::large_roots(
        &home,
        &history.db,
        &added,
        scope::has_full_disk_access(&home),
    );
    let project_roots: Vec<PathBuf> =
        crate::developer::search_roots(Some(&home), &[developer_added, added].concat());
    let temp = temp_dir();
    let started_at_ms = unix_time_ms();
    let started = Instant::now();
    // Preferences only leave findings out. If the exclusions can't be read, the scan doesn't
    // run: it would read folders the user excluded.
    let preferences = preferences::load(&history.db).preferences;
    let excluded = history.exclusions()?;
    let keep = |item: &crate::cleanup::Candidate| {
        crate::cleanup::kept_by_preferences(item, &preferences, &excluded, started_at_ms)
    };
    let ctx = Context {
        home: &home,
        temp: temp.as_deref(),
        running: &running,
        now_ms: started_at_ms,
        options,
        store_dev: history.store.volume(),
        large_roots: &large_roots,
        project_roots: &project_roots,
        cancel,
        keep: &keep,
        excluded: &excluded,
    };
    let outcome = scan::run(mode, &ctx, on_progress);
    let mut event = Event::new(
        EventKind::Scan,
        unix_time_ms(),
        match (outcome.cancelled, origin) {
            (true, _) => "cancelled",
            (false, ScanOrigin::Cleanup) => "completed",
            (false, ScanOrigin::Schedule) => "scheduled",
        },
        u64::try_from(outcome.items.len()).unwrap_or(u64::MAX),
        outcome
            .items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes)),
    );
    event.subject = Some(mode.as_str().to_owned());
    history.note(&event);
    // You chose the schedule to be told what it found; a scan you start notifies only if
    // turned on in Settings. Either notification opens the review when clicked.
    if !outcome.cancelled && (origin == ScanOrigin::Schedule || preferences.notify_scan) {
        let found = if outcome.items.is_empty() {
            "Nothing to clean up was found.".to_owned()
        } else {
            format!(
                "{} found, {}. Nothing was changed; click to review them.",
                items(outcome.items.len()),
                gigabytes(event.bytes)
            )
        };
        let title = match origin {
            ScanOrigin::Cleanup => "Scan finished",
            ScanOrigin::Schedule => "Scheduled scan finished",
        };
        crate::notifications::send(SCAN_NOTIFICATION, title, &found, preferences.notify_sound);
    }
    cleanup.set_scan(
        ScanInfo {
            mode,
            started_at_ms,
            cancelled: outcome.cancelled,
        },
        outcome.items,
    );
    Ok(CleanupScanResult {
        mode,
        cancelled: outcome.cancelled,
        started_at_ms,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        steps: outcome.steps,
        needs_full_disk_access: outcome.denied > 0,
        skipped_folders: outcome.skipped_roots,
        docker: outcome.docker,
    })
}

/// Runs a read-only Cleanup scan. The findings replace the last scan's in the review.
#[tauri::command]
pub async fn start_cleanup_scan(
    app: AppHandle,
    mode: ScanMode,
    options: ScanOptions,
    on_progress: Channel<Progress>,
) -> Result<CleanupScanResult, CommandError> {
    if !options.valid() {
        return Err(CommandErrorCode::InvalidRequest.into());
    }
    let state = app.state::<CleanupState>();
    let cancel = state
        .begin_scan()
        .map_err(|_| CommandError::from(CommandErrorCode::CleanupScanInProgress))?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_scan(
            &worker,
            mode,
            options,
            &cancel,
            ScanOrigin::Cleanup,
            |progress| {
                // A closed channel means the window went away; the scan still completes.
                let _ = on_progress.send(progress);
            },
        )
    })
    .await;
    state.end_scan();
    result.map_err(|_| CommandError::from(CommandErrorCode::CleanupScanFailed))?
}

#[tauri::command]
pub fn cancel_cleanup_scan(state: State<'_, CleanupState>) {
    state.cancel_scan();
}

/// The review: the last scan's findings and the items sent from Storage and Developer.
#[tauri::command]
pub fn get_cleanup_review(state: State<'_, CleanupState>) -> Review {
    state.review()
}

/// Sends caches and build folders from the latest Developer scan to the review. Returns the
/// Developer review IDs that were added (unknown or stale IDs are left out).
#[tauri::command]
pub async fn send_developer_items_to_review(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        let history = app.state::<History>();
        // Folders excluded since the Developer scan: their items aren't sent, or read. Unknown
        // exclusions send nothing.
        let excluded = history.exclusions()?;
        let items: Vec<_> = app
            .state::<DeveloperState>()
            .review_items(&ids)
            .into_iter()
            .filter(|item| !preferences::touches_excluded(&item.path, &excluded))
            .collect();
        let sent: Vec<String> = items.iter().map(|item| item.id.clone()).collect();
        let intake = Intake {
            home: &home,
            running: &running,
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let candidates = intake.developer_items(items);
        let bytes = candidates
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        let count = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
        app.state::<CleanupState>().add_sent(candidates);
        if count > 0 {
            let mut event =
                Event::new(EventKind::Review, unix_time_ms(), "completed", count, bytes);
            event.subject = Some("developer".to_owned());
            history.note(&event);
        }
        Ok(sent)
    })
    .await
    .map_err(blocking_failed)?
}

/// Adds a large file from the last Storage analysis to the review (inspect only).
#[tauri::command]
pub async fn add_storage_file_to_review(
    app: AppHandle,
    review_id: String,
) -> Result<bool, CommandError> {
    check_ids(std::slice::from_ref(&review_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let Some(file) = app.state::<AnalysisState>().large_file(&review_id) else {
            return Ok(false);
        };
        let history = app.state::<History>();
        let excluded = history.exclusions()?;
        let intake = Intake {
            home: &home,
            running: &[],
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let Some(candidate) = intake.storage_file(file) else {
            return Ok(false);
        };
        let mut event = Event::new(
            EventKind::Review,
            unix_time_ms(),
            "completed",
            1,
            candidate.bytes,
        );
        event.subject = Some("storage".to_owned());
        app.state::<CleanupState>().add_sent(vec![candidate]);
        history.note(&event);
        Ok(true)
    })
    .await
    .map_err(blocking_failed)?
}

/// Shows a review item in Finder, if it is still the item that was found.
#[tauri::command]
pub async fn reveal_review_item(app: AppHandle, id: String) -> Result<(), CommandError> {
    check_ids(std::slice::from_ref(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let item = app
            .state::<CleanupState>()
            .item(&id)
            .ok_or_else(|| CommandError::from(CommandErrorCode::RevealFailed))?;
        // Excluded since the scan: not read, not shown. An unreadable history excludes nothing,
        // but then Cleanup can't move anything either.
        let excluded = app.state::<History>().exclusions()?;
        if !crate::cleanup::revealable(&item, &excluded) {
            return Err(CommandErrorCode::RevealFailed.into());
        }
        crate::finder::reveal(&app, vec![item.path])
            .map_err(|_| CommandErrorCode::RevealFailed.into())
    })
    .await
    .map_err(blocking_failed)?
}

fn with_checks<T>(
    app: &AppHandle,
    run: impl FnOnce(&Checks<'_>, &History, &CleanupState) -> T,
) -> Result<T, CommandError> {
    let home = home()?;
    let running = running(app)?;
    let history = app.state::<History>();
    let cleanup = app.state::<CleanupState>();
    let added = cleanup.added_folders();
    let developer_added = app.state::<DeveloperState>().added();
    let added: Vec<PathBuf> = [developer_added, added].concat();
    let temp = temp_dir();
    let cancel = AtomicBool::new(false);
    // Settings may have changed since the scan; they are applied again here.
    let preferences = preferences::load(&history.db).preferences;
    let excluded = history.exclusions()?;
    let now_ms = unix_time_ms();
    let keep = |item: &crate::cleanup::Candidate| {
        crate::cleanup::kept_by_preferences(item, &preferences, &excluded, now_ms)
    };
    let checks = Checks {
        home: &home,
        temp: temp.as_deref(),
        running: &running,
        store_dev: history.store.volume(),
        added: &added,
        cancel: &cancel,
        keep: &keep,
        excluded: &excluded,
    };
    Ok(run(&checks, &history, &cleanup))
}

/// Makes a preview of the selected review items, each checked and measured again now.
#[tauri::command]
pub async fn create_cleanup_preview(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<PreviewView, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_checks(&app, |checks, _, cleanup| {
            execute::create_preview(cleanup, &ids, checks, unix_time_ms())
        })
    })
    .await
    .map_err(blocking_failed)?
}

/// Approves the preview: checks every item again, then moves each into quarantine, streaming
/// each item's progress over `on_progress`.
#[tauri::command]
pub async fn approve_cleanup_preview(
    app: AppHandle,
    preview_id: String,
    on_progress: Channel<MoveProgress>,
) -> Result<ApproveOutcome, CommandError> {
    check_ids(std::slice::from_ref(&preview_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let operations = app.state::<QuarantineState>();
        let _guard = operations
            .begin()
            .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
        with_checks(&app, |checks, history, cleanup| {
            let origin = cleanup
                .review()
                .scan
                .map_or("sent", |scan| scan.mode.as_str())
                .to_owned();
            let held_before = held_bytes(history);
            let outcome = execute::approve(
                cleanup,
                &preview_id,
                checks,
                history,
                &origin,
                unix_time_ms,
                |progress| {
                    // A closed channel means the window went away; the moves still complete.
                    let _ = on_progress.send(progress);
                },
            );
            if matches!(outcome, ApproveOutcome::Finished { .. }) {
                remind_if_large(history, held_before, held_bytes(history));
            }
            outcome
        })
    })
    .await
    .map_err(blocking_failed)?
}

/// Everything in quarantine, grouped by cleanup, with whether each item can go back now.
#[tauri::command]
pub async fn list_quarantine(app: AppHandle) -> Result<QuarantineList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        // Unfinished records are settled again whenever nothing is running, so one whose
        // place came into scope while an operation held the lock isn't left pending. If an
        // operation is running, it is listed as it is and settled on a later read.
        let operations = app.state::<QuarantineState>();
        if let Ok(guard) = operations.begin_settle() {
            settle_interrupted(&app);
            drop(guard);
        }
        let history = app.state::<History>();
        let (home, temp, added) = (storage::home_dir(), temp_dir(), chosen_folders(&app));
        // Without the exclusions no original place is looked at: nothing is verified.
        let excluded = history.exclusions().ok();
        let scope = home
            .as_deref()
            .zip(excluded.as_deref())
            .map(|(home, excluded)| quarantine::Scope {
                home,
                temp: temp.as_deref(),
                added: &added,
                excluded,
            });
        quarantine::list(&history.db, &history.store, scope.as_ref())
    })
    .await
    .map_err(blocking_failed)
}

/// Puts items back where they came from; never overwrites anything there.
#[tauri::command]
pub async fn restore_quarantine_items(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<RestoreResult>, CommandError> {
    check_ids(&ids)?;
    let home = home()?;
    tauri::async_runtime::spawn_blocking(move || {
        let operations = app.state::<QuarantineState>();
        let _guard = operations
            .begin()
            .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
        let history = app.state::<History>();
        let (temp, added) = (temp_dir(), chosen_folders(&app));
        let scope = quarantine::Scope {
            home: &home,
            temp: temp.as_deref(),
            added: &added,
            // A restore you ask for may put the item back into a folder excluded since; it
            // never overwrites what is there.
            excluded: &[],
        };
        quarantine::restore_items(&history.db, &history.store, &scope, &ids, unix_time_ms())
            .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))
    })
    .await
    .map_err(blocking_failed)?
}

/// Permanently deletes items from quarantine, one at a time, streaming progress. The user has
/// confirmed this separately in the page; `stop_quarantine_delete` stops after the current
/// item.
#[tauri::command]
pub async fn delete_quarantine_items(
    app: AppHandle,
    ids: Vec<String>,
    on_progress: Channel<DeleteProgress>,
) -> Result<DeleteSummary, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let operations = app.state::<QuarantineState>();
        let _guard = operations
            .begin()
            .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
        let stop = operations.stop_flag();
        let history = app.state::<History>();
        quarantine::delete_items(
            &history.db,
            &history.store,
            &ids,
            &stop,
            unix_time_ms,
            |progress| {
                // A closed channel means the window went away; the delete still stops on request.
                let _ = on_progress.send(progress);
            },
        )
        .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))
    })
    .await
    .map_err(blocking_failed)?
}

#[tauri::command]
pub fn stop_quarantine_delete(state: State<'_, QuarantineState>) {
    state.request_stop();
}

/// Shows a quarantined item in Finder, and with `with_original` also whatever is now at its
/// original place (to compare a blocked restore).
#[tauri::command]
pub async fn reveal_quarantine_item(
    app: AppHandle,
    id: String,
    with_original: bool,
) -> Result<(), CommandError> {
    check_ids(std::slice::from_ref(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let history = app.state::<History>();
        let item = history
            .db
            .item(&id)
            .ok()
            .flatten()
            .ok_or_else(|| CommandError::from(CommandErrorCode::RevealFailed))?;
        // Built only from IDs Kleari makes; a malformed stored ID reveals nothing.
        let held = history
            .store
            .reveal_path(&item.batch_id, &item.id)
            .ok_or_else(|| CommandError::from(CommandErrorCode::RevealFailed))?;
        let mut paths = vec![held];
        // The original comes from the stored record: it is shown only if it is a place an
        // item of its category comes from.
        let (temp, added) = (temp_dir(), chosen_folders(&app));
        let allowed = storage::home_dir().is_some_and(|home| {
            quarantine::Scope {
                home: &home,
                temp: temp.as_deref(),
                added: &added,
                // Exclusions are checked just below.
                excluded: &[],
            }
            .allows(&item)
        });
        // Nor is what is now at a place the user has since excluded looked at.
        // If the exclusions can't be read, the original isn't shown.
        let clear = app
            .state::<History>()
            .exclusions()
            .is_ok_and(|excluded| !preferences::touches_excluded(&item.original, &excluded));
        if with_original && allowed && clear && crate::cleanup::stat_path(&item.original).is_some()
        {
            paths.push(item.original.clone());
        }
        crate::finder::reveal(&app, paths).map_err(|_| CommandErrorCode::RevealFailed.into())
    })
    .await
    .map_err(blocking_failed)?
}
