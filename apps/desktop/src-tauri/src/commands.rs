//! IPC transport boundary. Commands map domain results into stable, non-sensitive errors.

use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::developer::{self, DeveloperState};
use crate::history::History;
use crate::persistence::{Event, EventKind};
use crate::storage::analysis::{self, Analysis, AnalysisProgress, AnalysisState};
use crate::storage::volume::{self, StorageOverview};
use crate::storage::{
    self, picker, FileFilter, FolderChoice, ProgressDto, ScanError, ScanSummary, StateError,
    StorageState,
};
use crate::system_monitor::open_paths;
use crate::system_monitor::running_apps::{self, QuitOutcome};
use crate::system_monitor::{
    self, ActivitySampler, ProcessList, ProcessListSampler, StartupItems, SystemActivity,
    SystemDetails, SystemMetrics, SystemSampler,
};
use crate::system_settings::{self, SettingsPane};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandErrorCode {
    MetricsUnavailable,
    SettingsUnavailable,
    PickerUnavailable,
    GrantNotFound,
    RootChanged,
    RootUnreadable,
    ScanInProgress,
    NoScanResult,
    InvalidFilter,
    AnalysisInProgress,
    AnalysisFailed,
    QuitUnavailable,
    DeveloperScanInProgress,
    DeveloperScanFailed,
    /// Malformed IDs or scan options.
    InvalidRequest,
    /// The home folder or the running apps couldn't be read, so Cleanup can't check safely.
    CleanupUnavailable,
    CleanupScanInProgress,
    CleanupScanFailed,
    /// Another cleanup, restore or permanent delete is running.
    OperationInProgress,
    /// The history can't be read or written, so nothing was changed.
    HistoryUnavailable,
    RevealFailed,
    /// Quarantine still holds items (your files); uninstalling waits until they're dealt with.
    QuarantineNotEmpty,
    /// macOS didn't move Kleari or its data to the Trash.
    UninstallFailed,
    /// Uninstall runs only from an installed Kleari.app, not a development build.
    UninstallNeedsApp,
    /// Updates run only from an installed Kleari.app, not a development build.
    UpdateNeedsApp,
    /// The Mouse settings couldn't be read or applied.
    MouseUnavailable,
    /// Checking for or installing an update didn't work.
    UpdateFailed,
    /// macOS didn't show (or hide) the menu-bar item, so the preference wasn't changed.
    MenuBarUnavailable,
    AppsScanInProgress,
    AppsScanFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: CommandErrorCode,
}

impl From<CommandErrorCode> for CommandError {
    fn from(code: CommandErrorCode) -> Self {
        Self { code }
    }
}

impl From<StateError> for CommandError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::GrantNotFound => CommandErrorCode::GrantNotFound,
            StateError::ScanInProgress => CommandErrorCode::ScanInProgress,
            StateError::NoScanResult => CommandErrorCode::NoScanResult,
        }
        .into()
    }
}

/// Returns one read-only snapshot of CPU, memory and startup-disk readings.
#[tauri::command]
pub async fn get_system_metrics(
    sampler: State<'_, Mutex<SystemSampler>>,
) -> Result<SystemMetrics, CommandError> {
    // A poisoned lock means an earlier sample panicked; report it rather than reuse state.
    let mut sampler = sampler.lock().map_err(|_| CommandError {
        code: CommandErrorCode::MetricsUnavailable,
    })?;
    Ok(system_monitor::to_metrics(&sampler.sample()))
}

/// Returns live readings for the System page's charts: CPU, memory, swap, memory pressure,
/// thermal state and network and startup-disk throughput. Lighter than
/// `get_system_metrics`: no process enumeration, so it can be polled about once a second.
#[tauri::command]
pub async fn get_system_activity(
    sampler: State<'_, Mutex<ActivitySampler>>,
) -> Result<SystemActivity, CommandError> {
    let mut sampler = sampler.lock().map_err(|_| CommandError {
        code: CommandErrorCode::MetricsUnavailable,
    })?;
    Ok(sampler.sample())
}

/// Returns hardware and OS details (chip, cores, model, macOS, display, volume, battery
/// health). Read on demand; nothing is stored.
#[tauri::command]
pub async fn get_system_details(app: AppHandle) -> Result<SystemDetails, CommandError> {
    // The display is read on the main thread; wait for it off the async runtime.
    tauri::async_runtime::spawn_blocking(move || system_monitor::read_details(&app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::MetricsUnavailable))
}

/// Lists launch agents and daemons from the standard folders. Names and labels only.
#[tauri::command]
pub async fn list_startup_items() -> Result<StartupItems, CommandError> {
    tauri::async_runtime::spawn_blocking(system_monitor::read_startup_items)
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::MetricsUnavailable))
}

/// Lists running processes for the Processes page: apps with their helpers folded in, and
/// every other readable process. Read-only; the page polls it every 2 s while it is shown.
#[tauri::command]
pub async fn list_processes(app: AppHandle) -> Result<ProcessList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        // Regular apps are read on the main thread before the sampler is locked.
        let apps = running_apps::read(&app)
            .map_err(|running_apps::Unavailable| CommandErrorCode::MetricsUnavailable)?;
        let sampler = app.state::<Mutex<ProcessListSampler>>();
        // A poisoned lock means an earlier sample panicked; report it rather than reuse state.
        let mut sampler = sampler
            .lock()
            .map_err(|_| CommandErrorCode::MetricsUnavailable)?;
        // No list where the process sampler can't read this system: reported as unavailable.
        sampler
            .sample(&apps, storage::home_dir().as_deref())
            .ok_or(CommandErrorCode::MetricsUnavailable)
    })
    .await
    .map_err(|_| CommandError::from(CommandErrorCode::MetricsUnavailable))?
    .map_err(CommandError::from)
}

/// Asks one regular app to quit, the same as choosing Quit from its menu; the app may ask
/// to save or decline. `quit_token` is the row's identity token (the app process's start
/// time to the microsecond), checked against the PID's owner and start time immediately
/// before asking, so a PID that now belongs to another process is never asked. Kleari
/// itself, apps that ship with macOS, other accounts', background and system processes are
/// refused.
#[tauri::command]
pub async fn quit_app(
    app: AppHandle,
    pid: u32,
    quit_token: u64,
) -> Result<QuitOutcome, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        // Only an app the latest list offered can be asked; the bundle comes from the
        // backend's own record, never from the frontend.
        let target = {
            let sampler = app.state::<Mutex<ProcessListSampler>>();
            let sampler = sampler.lock().map_err(|_| running_apps::Unavailable)?;
            sampler.quit_target(pid, quit_token)
        };
        let Some(target) = target else {
            return Ok(QuitOutcome::NotRunning);
        };
        // The app's name, for the Activity page; never its path.
        let name = target
            .bundle
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
        let outcome = running_apps::quit(&app, target, quit_token)?;
        let answer = match outcome {
            QuitOutcome::Requested => Some("requested"),
            QuitOutcome::Declined => Some("refused"),
            QuitOutcome::NotRunning | QuitOutcome::NotAllowed => None,
        };
        if let Some(answer) = answer {
            let mut event = Event::new(EventKind::Quit, unix_time_ms(), answer, 1, 0);
            event.subject = name;
            app.state::<History>().note(&event);
        }
        Ok(outcome)
    })
    .await
    .map_err(|_| CommandError::from(CommandErrorCode::QuitUnavailable))?
    .map_err(|running_apps::Unavailable| CommandErrorCode::QuitUnavailable.into())
}

/// Opens one fixed System Settings pane. The pane is an enum; the frontend never sends a URL.
#[tauri::command]
pub async fn open_system_settings(app: AppHandle, pane: SettingsPane) -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(move || system_settings::open(&app, pane))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::SettingsUnavailable))?
        .map_err(|system_settings::OpenFailed| CommandErrorCode::SettingsUnavailable.into())
}

/// Opens the native folder panel. The chosen folder is kept in Rust under a grant ID; the
/// frontend receives only the ID and the folder's name.
#[tauri::command]
pub async fn choose_scan_folder(
    app: AppHandle,
    storage: State<'_, StorageState>,
) -> Result<FolderChoice, CommandError> {
    // The panel runs on the main thread; wait for it off the async runtime.
    let chosen = tauri::async_runtime::spawn_blocking(move || picker::pick_folder(&app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::PickerUnavailable))?
        .map_err(|picker::PickerError::Unavailable| {
            CommandError::from(CommandErrorCode::PickerUnavailable)
        })?;
    let Some(path) = chosen else {
        return Ok(FolderChoice::Cancelled);
    };
    let (grant_id, display_name) = storage
        .add_grant(&path)
        .ok_or_else(|| CommandError::from(CommandErrorCode::RootUnreadable))?;
    Ok(FolderChoice::Chosen {
        grant_id,
        display_name,
    })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Scans a granted folder read-only, streaming progress over `on_progress`.
#[tauri::command]
pub async fn start_scan(
    grant_id: String,
    on_progress: Channel<ProgressDto>,
    storage: State<'_, StorageState>,
) -> Result<ScanSummary, CommandError> {
    let (grant, cancel) = storage.begin_scan(&grant_id)?;
    let started_at_ms = unix_time_ms();
    let started = Instant::now();
    let root = grant.root.clone();
    let identity = grant.identity;
    let result = tauri::async_runtime::spawn_blocking(move || {
        storage::scan(&root, identity, &cancel, |progress| {
            // A closed channel means the window went away; the scan still completes.
            let _ = on_progress.send(ProgressDto::from(progress));
        })
    })
    .await;

    let outcome = match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            storage.end_scan(&grant_id, None);
            return Err(match error {
                ScanError::RootChanged => CommandErrorCode::RootChanged,
                ScanError::RootUnreadable => CommandErrorCode::RootUnreadable,
            }
            .into());
        }
        Err(_) => {
            // The scan thread panicked; report the root as unreadable rather than hang.
            storage.end_scan(&grant_id, None);
            return Err(CommandErrorCode::RootUnreadable.into());
        }
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let summary = storage::summarize(
        &grant_id,
        &grant.display_name,
        &outcome,
        started_at_ms,
        duration_ms,
    );
    storage.end_scan(&grant_id, Some(outcome));
    Ok(summary)
}

/// Returns the startup volume's capacity, the space macOS manages (purgeable, swap and sleep
/// image) and whether Full Disk Access is granted. Nothing walks the disk.
#[tauri::command]
pub async fn get_storage_overview() -> Result<StorageOverview, CommandError> {
    tauri::async_runtime::spawn_blocking(|| volume::read(storage::home_dir().as_deref()))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::MetricsUnavailable))
}

/// Measures developer caches, project build folders and Docker's disk use, read-only,
/// streaming progress over `on_progress`. The cache locations are fixed in Rust; projects are
/// searched in the home folder and in folders added through the macOS folder picker.
#[tauri::command]
pub async fn start_developer_scan(
    app: AppHandle,
    on_progress: Channel<developer::ScanProgress>,
    state: State<'_, DeveloperState>,
) -> Result<developer::DeveloperScan, CommandError> {
    // Folders excluded in Settings. If they can't be read the scan doesn't run: it would
    // read folders the user excluded.
    let excluded = app.state::<History>().exclusions()?;
    let cancel = state.begin().map_err(|developer::ScanBusy| {
        CommandError::from(CommandErrorCode::DeveloperScanInProgress)
    })?;
    let added = state.added();
    let started_at_ms = unix_time_ms();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let home = storage::home_dir();
        let open = open_paths::read().ok();
        developer::scan(
            home.as_deref(),
            &added,
            &excluded,
            open.as_ref(),
            &cancel,
            started_at_ms,
            |progress| {
                // A closed channel means the window went away; the scan still completes.
                let _ = on_progress.send(progress);
            },
        )
    })
    .await;
    state.end();
    // The worker panicked; report a failure rather than a partial result.
    let scan = result.map_err(|_| CommandError::from(CommandErrorCode::DeveloperScanFailed))?;
    // Kept so the page can send what it found to Cleanup's review.
    state.set_review(scan.review_items.clone());
    let bytes = scan
        .caches
        .iter()
        .map(|cache| cache.measured.bytes)
        .chain(scan.projects.iter().map(|project| project.bytes))
        .fold(0_u64, u64::saturating_add);
    app.state::<History>().note(&Event::new(
        EventKind::DeveloperScan,
        unix_time_ms(),
        match scan.status {
            developer::ScanStatus::Completed => "completed",
            developer::ScanStatus::Cancelled => "cancelled",
        },
        u64::try_from(scan.caches.len() + scan.projects.len()).unwrap_or(u64::MAX),
        bytes,
    ));
    Ok(scan)
}

/// Requests cancellation of the running Developer scan; it returns what was found so far.
#[tauri::command]
pub fn cancel_developer_scan(state: State<'_, DeveloperState>) {
    state.cancel();
}

/// Opens the macOS folder panel to add a folder the Developer page searches for projects.
/// The folder stays in Rust for the session; the frontend gets only its name.
#[tauri::command]
pub async fn add_developer_folder(
    app: AppHandle,
    state: State<'_, DeveloperState>,
) -> Result<developer::AddedFolder, CommandError> {
    let chosen = tauri::async_runtime::spawn_blocking(move || picker::pick_folder(&app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::PickerUnavailable))?
        .map_err(|picker::PickerError::Unavailable| {
            CommandError::from(CommandErrorCode::PickerUnavailable)
        })?;
    let Some(path) = chosen else {
        return Ok(developer::AddedFolder::Cancelled);
    };
    state
        .add(&path)
        .map(|name| developer::AddedFolder::Added { name })
        .ok_or_else(|| CommandError::from(CommandErrorCode::RootUnreadable))
}

/// Measures the fixed locations of the startup disk read-only, streaming progress over
/// `on_progress`. The locations are fixed in Rust; the frontend sends no path.
#[tauri::command]
pub async fn start_storage_analysis(
    app: AppHandle,
    on_progress: Channel<AnalysisProgress>,
    state: State<'_, AnalysisState>,
) -> Result<Analysis, CommandError> {
    let cancel = state.begin().map_err(|analysis::AnalysisBusy| {
        CommandError::from(CommandErrorCode::AnalysisInProgress)
    })?;
    let started_at_ms = unix_time_ms();
    let started = Instant::now();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let steps = analysis::steps(storage::home_dir().as_deref());
        analysis::analyze(&steps, analysis::Limits::default(), &cancel, |progress| {
            // A closed channel means the window went away; the analysis still completes.
            let _ = on_progress.send(progress);
        })
    })
    .await;
    state.end();
    // The worker panicked; report a failure rather than a partial result.
    let (status, steps, acc) =
        result.map_err(|_| CommandError::from(CommandErrorCode::AnalysisFailed))?;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut analysis = analysis::summarize(status, steps, acc, started_at_ms, duration_ms);
    // Large files are suggestions: Settings' exclusions and recent-file window leave some out.
    // The totals are measurements and still count everything. If the exclusions can't be read,
    // no large file is suggested.
    let history = app.state::<History>();
    let preferences = crate::preferences::load(&history.db).preferences;
    let recent = preferences.recent_cutoff_ms(started_at_ms);
    match history.exclusions() {
        Ok(excluded) => analysis.large_files.retain(|file| {
            !crate::preferences::touches_excluded(&file.source.path, &excluded)
                && recent.is_none_or(|cutoff| file.modified_ms.is_none_or(|ms| ms < cutoff))
        }),
        Err(_) => analysis.large_files.clear(),
    }
    // Kept so a large file can be added to Cleanup's review.
    state.set_large_files(analysis.large_files.clone());
    app.state::<History>().note(&Event::new(
        EventKind::Analysis,
        unix_time_ms(),
        match analysis.status {
            analysis::AnalysisStatus::Completed => "completed",
            analysis::AnalysisStatus::Cancelled => "cancelled",
        },
        u64::try_from(analysis.categories.len()).unwrap_or(u64::MAX),
        analysis.categories.iter().fold(0_u64, |total, category| {
            total.saturating_add(category.bytes)
        }),
    ));
    Ok(analysis)
}

/// Requests cancellation of the running analysis; it returns what was counted so far.
#[tauri::command]
pub fn cancel_storage_analysis(state: State<'_, AnalysisState>) {
    state.cancel();
}

/// Requests cancellation of the running scan; the scan returns partial results.
#[tauri::command]
pub fn cancel_scan(grant_id: String, storage: State<'_, StorageState>) {
    storage.cancel(&grant_id);
}

/// Re-ranks the last scan's files under the given filters without rescanning the disk. The
/// query can visit the whole index, so it runs on a worker thread, not the main thread.
#[tauri::command]
pub async fn query_scan(
    grant_id: String,
    filter: FileFilter,
    storage: State<'_, StorageState>,
) -> Result<storage::QueryResult, CommandError> {
    let last = storage.last_scan(&grant_id)?;
    tauri::async_runtime::spawn_blocking(move || storage::query(&last.outcome, &filter))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::NoScanResult))?
        .map_err(|storage::InvalidFilter| CommandErrorCode::InvalidFilter.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialize_to_stable_codes() {
        for (code, expected) in [
            (CommandErrorCode::MetricsUnavailable, "metricsUnavailable"),
            (CommandErrorCode::SettingsUnavailable, "settingsUnavailable"),
            (CommandErrorCode::GrantNotFound, "grantNotFound"),
            (CommandErrorCode::RootChanged, "rootChanged"),
            (CommandErrorCode::ScanInProgress, "scanInProgress"),
            (CommandErrorCode::InvalidFilter, "invalidFilter"),
            (CommandErrorCode::AnalysisInProgress, "analysisInProgress"),
            (CommandErrorCode::AnalysisFailed, "analysisFailed"),
            (CommandErrorCode::QuitUnavailable, "quitUnavailable"),
            (
                CommandErrorCode::DeveloperScanInProgress,
                "developerScanInProgress",
            ),
            (CommandErrorCode::DeveloperScanFailed, "developerScanFailed"),
            (CommandErrorCode::InvalidRequest, "invalidRequest"),
            (CommandErrorCode::CleanupUnavailable, "cleanupUnavailable"),
            (
                CommandErrorCode::CleanupScanInProgress,
                "cleanupScanInProgress",
            ),
            (CommandErrorCode::CleanupScanFailed, "cleanupScanFailed"),
            (CommandErrorCode::OperationInProgress, "operationInProgress"),
            (CommandErrorCode::HistoryUnavailable, "historyUnavailable"),
            (CommandErrorCode::RevealFailed, "revealFailed"),
            (CommandErrorCode::QuarantineNotEmpty, "quarantineNotEmpty"),
            (CommandErrorCode::UninstallFailed, "uninstallFailed"),
            (CommandErrorCode::UninstallNeedsApp, "uninstallNeedsApp"),
            (CommandErrorCode::UpdateNeedsApp, "updateNeedsApp"),
            (CommandErrorCode::MouseUnavailable, "mouseUnavailable"),
            (CommandErrorCode::MenuBarUnavailable, "menuBarUnavailable"),
            (CommandErrorCode::UpdateFailed, "updateFailed"),
        ] {
            assert_eq!(
                serde_json::to_value(CommandError::from(code)).expect("error serializes"),
                serde_json::json!({ "code": expected })
            );
        }
    }

    #[test]
    fn release_grants_the_live_screens_and_not_the_retired_commands() {
        let release: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).expect("json");
        let release = release["permissions"].to_string();
        // Processes and Developer ship in release builds (ADR-009, product owner decision).
        for permission in [
            "allow-list-processes",
            "allow-quit-app",
            "allow-start-developer-scan",
            "allow-cancel-developer-scan",
            "allow-add-developer-folder",
            // Cleanup and Quarantine ship by the product owner's decision (ADR-012).
            "allow-start-cleanup-scan",
            "allow-approve-cleanup-preview",
            "allow-restore-quarantine-items",
            "allow-delete-quarantine-items",
        ] {
            assert!(release.contains(permission), "{permission} ships");
        }
        // No screen uses these, so they stay ungranted.
        for permission in [
            "allow-get-system-metrics",
            "allow-choose-scan-folder",
            "allow-start-scan",
            "allow-cancel-scan",
            "allow-query-scan",
        ] {
            assert!(!release.contains(permission), "{permission} must not ship");
        }
    }
}
