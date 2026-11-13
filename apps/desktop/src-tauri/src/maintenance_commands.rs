//! Commands for the Maintenance page (ADR-020): read what each task concerns, run one task at
//! a time (macOS asks for an administrator's password for most), and send a launch agent you
//! chose to Cleanup's review.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::cleanup::intake::Intake;
use crate::cleanup::CleanupState;
use crate::cleanup_commands::{blocking_failed, home};
use crate::commands::{CommandError, CommandErrorCode};
use crate::history::History;
use crate::maintenance::{self, Outcome, Task, TaskInfo};
use crate::persistence::{Event, EventKind};
use crate::system_monitor::open_paths::OpenPaths;
use crate::system_monitor::running_apps;

/// One task at a time.
#[derive(Default)]
pub struct MaintenanceState {
    running: Mutex<bool>,
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

#[tauri::command]
pub async fn get_maintenance_info() -> Result<Vec<TaskInfo>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        // SAFETY: getuid has no preconditions and can't fail.
        let uid = unsafe { libc::getuid() };
        Ok(maintenance::info(&home, uid, &AtomicBool::new(false)))
    })
    .await
    .map_err(blocking_failed)?
}

/// Runs a task; the page has already asked you to confirm.
#[tauri::command]
pub async fn run_maintenance_task(app: AppHandle, task: Task) -> Result<Outcome, CommandError> {
    {
        let state = app.state::<MaintenanceState>();
        let mut running = state
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *running {
            return Err(CommandErrorCode::OperationInProgress.into());
        }
        *running = true;
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let outcome = maintenance::run(task);
        let label = match outcome {
            Outcome::Done => "completed",
            Outcome::Cancelled => "cancelled",
            Outcome::Failed => "failed",
        };
        let mut event = Event::new(EventKind::Maintenance, unix_time_ms(), label, 1, 0);
        event.subject = Some(task.as_str().to_owned());
        worker.state::<History>().note(&event);
        outcome
    })
    .await;
    // Released whether the task finished or its thread failed.
    let state = app.state::<MaintenanceState>();
    *state
        .running
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    result.map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))
}

/// Sends the launch agent with launchd label `label`, in your LaunchAgents folder, to
/// Cleanup's review. `false` if none has that label (or it is Apple's).
#[tauri::command]
pub async fn send_launch_agent_to_review(
    app: AppHandle,
    label: String,
) -> Result<bool, CommandError> {
    if label.is_empty() || label.len() > 200 || label.contains(['/', '\0']) {
        return Err(CommandErrorCode::InvalidRequest.into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let home = home()?;
        let folder = home.join("Library/LaunchAgents");
        let Some((mut dir, _)) = crate::developer::open_absolute(&folder) else {
            return Ok(false);
        };
        let cancel = AtomicBool::new(false);
        let mut found = None;
        while let Some(Ok(name)) = dir.next_name() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let Ok(text) = name.to_str() else {
                continue;
            };
            if !text.ends_with(".plist") || text.to_lowercase().starts_with("com.apple.") {
                continue;
            }
            let Ok(stat) = dir.stat_child(&name) else {
                continue;
            };
            if !stat.is_file() {
                continue;
            }
            let Ok(file) = dir.open_file_child(&name) else {
                continue;
            };
            let mut bytes = Vec::new();
            if file.take(256 * 1024).read_to_end(&mut bytes).is_err() {
                continue;
            }
            let matches = plist::Value::from_reader(std::io::Cursor::new(&bytes))
                .ok()
                .and_then(|value| {
                    value
                        .as_dictionary()?
                        .get("Label")?
                        .as_string()
                        .map(|found| found == label)
                })
                .unwrap_or(false);
            if matches {
                found = Some((folder.join(text), stat));
                break;
            }
        }
        let Some((path, stat)) = found else {
            return Ok(false);
        };
        let running = running_apps::running_bundles(&app).unwrap_or_default();
        let history = app.state::<History>();
        let excluded = history.exclusions()?;
        let intake = Intake {
            home: &home,
            running: &running,
            open: Some(&OpenPaths::NOTHING),
            store_dev: history.store.volume(),
            now_ms: unix_time_ms(),
            excluded: &excluded,
        };
        let Some(candidate) = intake.launch_agent(path, &stat) else {
            return Ok(false);
        };
        let mut event = Event::new(
            EventKind::Review,
            unix_time_ms(),
            "completed",
            1,
            candidate.bytes,
        );
        event.subject = Some("maintenance".to_owned());
        history.note(&event);
        app.state::<CleanupState>().add_sent(vec![candidate]);
        Ok(true)
    })
    .await
    .map_err(blocking_failed)?
}
