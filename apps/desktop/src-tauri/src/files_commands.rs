//! Commands for the Files page (ADR-019): run one tool, read-only, and send what you chose to
//! Cleanup's review, where it moves only into quarantine and only when you approve a preview.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};

use crate::cleanup::execute::PreviewView;
use crate::cleanup::intake::Intake;
use crate::cleanup_commands::{blocking_failed, check_ids, home};
use crate::commands::{CommandError, CommandErrorCode};
use crate::files::{self, FilesScan, FilesState, Options, ScanBusy, Tool};
use crate::history::History;
use crate::storage::explorer::ExplorerState;
use crate::system_monitor::open_paths;
use crate::system_monitor::running_apps;

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

#[tauri::command]
pub async fn start_files_scan(
    app: AppHandle,
    tool: Tool,
    options: Options,
) -> Result<FilesScan, CommandError> {
    if !options.valid() {
        return Err(CommandErrorCode::InvalidRequest.into());
    }
    let cancel = app
        .state::<FilesState>()
        .begin()
        .map_err(|ScanBusy| CommandError::from(CommandErrorCode::FilesScanInProgress))?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = match home() {
            Ok(home) => home,
            Err(error) => {
                app.state::<FilesState>().abandon();
                return Err(error);
            }
        };
        // Folders excluded in Settings are never entered; without the list, nothing runs.
        let excluded = match app.state::<History>().exclusions() {
            Ok(excluded) => excluded,
            Err(error) => {
                app.state::<FilesState>().abandon();
                return Err(error);
            }
        };
        Ok(files::scan(
            &app.state::<FilesState>(),
            tool,
            options,
            &home,
            &excluded,
            unix_time_ms(),
            &cancel,
        ))
    })
    .await
    .map_err(|_| CommandError::from(CommandErrorCode::FilesScanFailed))?
}

#[tauri::command]
pub fn cancel_files_scan(state: State<'_, FilesState>) {
    state.cancel();
}

/// Opens a folder the last search found (Files › Downloads) in the Browse view, read-only:
/// its ID there, to list with `list_storage_folder`, or `None` when it isn't a known folder
/// or changed since.
#[tauri::command]
pub async fn open_found_folder(app: AppHandle, id: String) -> Result<Option<String>, CommandError> {
    check_ids(std::slice::from_ref(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let excluded = app.state::<History>().exclusions()?;
        let Some((path, stat)) = app.state::<FilesState>().folder(&id) else {
            return Ok(None);
        };
        Ok(app
            .state::<ExplorerState>()
            .folder_at(&home, &path, &stat, &excluded))
    })
    .await
    .map_err(blocking_failed)?
}

/// Prepares what you chose for removal on the Files page (in a group of copies one is always
/// held back): its preview.
#[tauri::command]
pub async fn prepare_files_removal(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<PreviewView, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        // Unread, everything counts as open (fail closed); read again before anything moves.
        let open = open_paths::read().ok();
        let history = app.state::<History>();
        let excluded = history.exclusions()?;
        let chosen = app.state::<FilesState>().chosen(&ids);
        let intake = Intake {
            home: &home,
            running: &running,
            open: open.as_ref(),
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        crate::cleanup_commands::prepare_removal(&app, intake.chosen_items(chosen))
    })
    .await
    .map_err(blocking_failed)?
}
