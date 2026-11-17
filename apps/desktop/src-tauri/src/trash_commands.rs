//! The Trash commands (`trash`): list it, delete chosen items for good, put them back. Emptying
//! the whole Trash is Maintenance's Empty Trash task.

use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Manager};

use crate::cleanup_commands::{blocking_failed, check_ids, home};
use crate::commands::{CommandError, CommandErrorCode};
use crate::history::{unix_time_ms, History};
use crate::persistence::{Event, EventKind};
use crate::quarantine::QuarantineState;
use crate::trash::{self, TrashList, TrashOutcome, TrashState};

/// What is in the Trash (yours and each external disk's); needs Full Disk Access.
#[tauri::command]
pub async fn list_trash(app: AppHandle) -> Result<TrashList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        // SAFETY: getuid has no preconditions and can't fail.
        let uid = unsafe { libc::getuid() };
        let volumes = crate::maintenance::external_volumes();
        Ok(trash::list(
            &app.state::<TrashState>(),
            &home,
            uid,
            &volumes,
            unix_time_ms(),
            &AtomicBool::new(false),
        ))
    })
    .await
    .map_err(blocking_failed)?
}

/// Records what was done in Activity: counts and size, never names or paths.
fn note(history: &History, kind: EventKind, ids: usize, outcome: &TrashOutcome) {
    if ids == 0 {
        return;
    }
    let failed = outcome.failed.len();
    let mut event = Event::new(
        kind,
        unix_time_ms(),
        match (outcome.done.len(), failed) {
            (_, 0) => "completed",
            (0, _) => "failed",
            _ => "partial",
        },
        u64::try_from(ids).unwrap_or(u64::MAX),
        outcome.bytes,
    );
    event.failed = u64::try_from(failed).unwrap_or(u64::MAX);
    event.subject = Some("trash".to_owned());
    history.note(&event);
}

fn run(
    app: AppHandle,
    ids: Vec<String>,
    kind: EventKind,
    action: fn(&TrashState, &[String]) -> TrashOutcome,
) -> Result<TrashOutcome, CommandError> {
    // One change at a time, as for quarantine and Cleanup.
    let operations = app.state::<QuarantineState>();
    let _guard = operations
        .begin()
        .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
    let outcome = action(&app.state::<TrashState>(), &ids);
    note(&app.state::<History>(), kind, ids.len(), &outcome);
    Ok(outcome)
}

/// Deletes the chosen items from the Trash for good.
#[tauri::command]
pub async fn delete_trash_items(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<TrashOutcome, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || run(app, ids, EventKind::Delete, trash::delete))
        .await
        .map_err(blocking_failed)?
}

/// Puts the chosen items back where Finder says they came from (never over anything there).
#[tauri::command]
pub async fn put_back_trash_items(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<TrashOutcome, CommandError> {
    check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || run(app, ids, EventKind::Restore, trash::put_back))
        .await
        .map_err(blocking_failed)?
}
