//! Commands for the Storage page's Browse view: list a folder of the home folder, read-only,
//! and send what you chose to Cleanup's review, where it moves only into quarantine and only
//! when you approve a preview.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::cleanup::intake::Intake;
use crate::cleanup::CleanupState;
use crate::cleanup_commands::{blocking_failed, check_ids, home};
use crate::commands::{CommandError, CommandErrorCode};
use crate::history::History;
use crate::persistence::{Event, EventKind};
use crate::storage::explorer::{ExplorerState, Listing, ListingProgress};
use crate::system_monitor::open_paths;
use crate::system_monitor::running_apps;

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

/// Lists a folder (`None`: the home folder), measuring each entry. A new listing stops one
/// still running. `None` back means the ID is unknown (forgotten): list from Home again.
#[tauri::command]
pub async fn list_storage_folder(
    app: AppHandle,
    id: Option<String>,
    on_progress: Channel<ListingProgress>,
) -> Result<Option<Listing>, CommandError> {
    if let Some(id) = &id {
        check_ids(std::slice::from_ref(id))?;
    }
    let cancel = app.state::<ExplorerState>().begin();
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        // Folders excluded in Settings are never entered; without the list, nothing is.
        let excluded = app.state::<History>().exclusions()?;
        let mut last = std::time::Instant::now();
        Ok(app.state::<ExplorerState>().list(
            &home,
            id.as_deref(),
            &excluded,
            &cancel,
            &mut |progress| {
                if last.elapsed() >= std::time::Duration::from_millis(200) {
                    last = std::time::Instant::now();
                    // A closed channel means the page went away; the listing stops with the next.
                    let _ = on_progress.send(progress);
                }
            },
        ))
    })
    .await
    .map_err(|_| CommandError::from(CommandErrorCode::AnalysisFailed))?
}

#[tauri::command]
pub fn cancel_storage_listing(state: State<'_, ExplorerState>) {
    state.cancel();
}

/// Sends chosen files and folders to Cleanup's review. Returns how many were sent.
#[tauri::command]
pub async fn send_storage_items_to_review(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        // Unread, everything counts as open (fail closed); read again before anything moves.
        let open = open_paths::read().ok();
        let history = app.state::<History>();
        let excluded = history.exclusions()?;
        let chosen = app.state::<ExplorerState>().chosen(&ids);
        let intake = Intake {
            home: &home,
            running: &running,
            open: open.as_ref(),
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let candidates = intake.chosen_items(chosen);
        if !candidates.is_empty() {
            let bytes = candidates
                .iter()
                .fold(0_u64, |total, item| total.saturating_add(item.bytes));
            let count = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
            let mut event =
                Event::new(EventKind::Review, unix_time_ms(), "completed", count, bytes);
            event.subject = Some("storage".to_owned());
            history.note(&event);
        }
        Ok(app.state::<CleanupState>().add_sent(candidates))
    })
    .await
    .map_err(blocking_failed)?
}
