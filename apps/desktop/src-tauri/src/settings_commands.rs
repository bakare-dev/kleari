//! Commands for the Settings and Activity pages: preferences, Open at login, notification
//! permission, folder access and exclusions, and the activity log. Paths stay in Rust: the
//! frontend sees folders as `~/…` and names one back only to remove it, matched against
//! Rust's own list.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::activity::{self, ActivityList};
use crate::cleanup::scope::{self, ProtectedFolder};
use crate::cleanup::CleanupState;
use crate::commands::{CommandError, CommandErrorCode};
use crate::developer::DeveloperState;
use crate::history::{unix_time_ms, History};
use crate::login_item::{self, LoginChange, LoginItem};
use crate::notifications::{self, NotificationAccess};
use crate::persistence::AccessState;
use crate::preferences::{self, Loaded, Preferences, SaveError};
use crate::quarantine::QuarantineState;
use crate::storage::{self, picker};
use crate::system_monitor::display_path;

/// One preferences save at a time, so the menu-bar item and the saved choice can't cross over
/// between two overlapping saves.
static SAVING_PREFERENCES: Mutex<()> = Mutex::new(());

fn blocking_failed(_: tauri::Error) -> CommandError {
    CommandErrorCode::HistoryUnavailable.into()
}

/// Where a folder Kleari may read came from, so it can be removed the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderSource {
    /// Downloads, Desktop, Documents or Movies, after Grant access.
    Protected,
    /// Added in Cleanup's folder panel this session.
    Cleanup,
    /// Added in Developer's folder panel this session.
    Developer,
    /// Chosen in Quarantine to restore into this session.
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessFolder {
    pub source: FolderSource,
    pub name: String,
    /// Shown as `~/…`; sent back as is to remove it.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsState {
    pub preferences: Loaded,
    pub login_item: LoginItem,
    pub notifications: NotificationAccess,
    pub full_disk_access: bool,
    pub folders: Vec<AccessFolder>,
    /// Folders scans skip, shown as `~/…`.
    pub exclusions: Vec<String>,
    pub quarantine_folder: String,
    pub version: String,
    /// When the saved schedule next scans and last did.
    pub schedule: crate::schedule::Status,
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn folders(app: &AppHandle, home: Option<&Path>) -> Vec<AccessFolder> {
    let history = app.state::<History>();
    let mut folders: Vec<AccessFolder> = home
        .map(|home| {
            ProtectedFolder::ALL
                .into_iter()
                .filter(|folder| {
                    history.db.folder_access(folder.key()) == Ok(Some(AccessState::Granted))
                })
                .map(|folder| {
                    let path = folder.path(home);
                    AccessFolder {
                        source: FolderSource::Protected,
                        name: name_of(&path),
                        path: display_path(&path, Some(home)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let sessions = [
        (
            FolderSource::Cleanup,
            app.state::<CleanupState>().added_folders(),
        ),
        (
            FolderSource::Developer,
            app.state::<DeveloperState>().added(),
        ),
        (
            FolderSource::Restore,
            app.state::<QuarantineState>().restore_folders(),
        ),
    ];
    for (source, paths) in sessions {
        folders.extend(paths.iter().map(|path| AccessFolder {
            source,
            name: name_of(path),
            path: display_path(path, home),
        }));
    }
    folders
}

fn exclusions(history: &History, home: Option<&Path>) -> Result<Vec<String>, CommandError> {
    history
        .db
        .excluded_folders()
        .map(|paths| paths.iter().map(|path| display_path(path, home)).collect())
        .map_err(|_| CommandErrorCode::HistoryUnavailable.into())
}

fn state(app: &AppHandle) -> Result<SettingsState, CommandError> {
    let home = storage::home_dir();
    let history = app.state::<History>();
    let preferences = preferences::load(&history.db);
    Ok(SettingsState {
        schedule: crate::schedule::status(&history.db, &preferences.preferences.scan_schedule),
        preferences,
        login_item: login_item::status(),
        notifications: notifications::status(),
        full_disk_access: home.as_deref().is_some_and(scope::has_full_disk_access),
        folders: folders(app, home.as_deref()),
        exclusions: exclusions(&history, home.as_deref())?,
        quarantine_folder: display_path(&history.store.path(), home.as_deref()),
        version: app.package_info().version.to_string(),
    })
}

/// Everything the Settings page shows, read now.
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<SettingsState, CommandError> {
    tauri::async_runtime::spawn_blocking(move || state(&app))
        .await
        .map_err(blocking_failed)?
}

/// Saves the preferences if every value is one the page offers. A shorter activity window
/// deletes older activity at once (quarantine records are kept).
#[tauri::command]
pub async fn set_preferences(
    app: AppHandle,
    preferences: Preferences,
) -> Result<Loaded, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        // After a panic in an earlier save, this save still sets the item and the saved choice
        // together, so it can go ahead.
        let _saving = SAVING_PREFERENCES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let history = app.state::<History>();
        // The menu-bar item changes first; if it can't, nothing is saved and the page says so.
        let menu_bar_was = crate::menubar::active(&app);
        crate::menubar::set(&app, preferences.menu_bar)?;
        if let Err(error) = preferences::save(&history.db, &preferences) {
            // Put the item back as it was, to match what is still saved. Best effort: if this
            // fails too there is no item, and closing the window quits as usual.
            let _ = crate::menubar::set(&app, menu_bar_was);
            return Err(match error {
                SaveError::Invalid => CommandError::from(CommandErrorCode::InvalidRequest),
                SaveError::Unavailable(_) => CommandErrorCode::HistoryUnavailable.into(),
            });
        }
        // The preferences are saved; a failed prune is retried at the next launch.
        let _ = history
            .db
            .prune_events(preferences.retention_start_ms(unix_time_ms()));
        Ok(preferences::load(&history.db))
    })
    .await
    .map_err(blocking_failed)?
}

/// Turns Open at login on or off and returns what macOS now reports.
#[tauri::command]
pub async fn set_open_at_login(enabled: bool) -> Result<LoginChange, CommandError> {
    tauri::async_runtime::spawn_blocking(move || login_item::set(enabled))
        .await
        .map_err(|_| CommandErrorCode::SettingsUnavailable.into())
}

/// Asks macOS for permission to show notifications (macOS prompts only the first time).
#[tauri::command]
pub async fn request_notifications() -> Result<NotificationAccess, CommandError> {
    tauri::async_runtime::spawn_blocking(notifications::request)
        .await
        .map_err(|_| CommandErrorCode::SettingsUnavailable.into())
}

/// Opens the macOS folder panel and adds the chosen folder to those scans skip. Returns the
/// list, shown as `~/…`.
#[tauri::command]
pub async fn add_excluded_folder(app: AppHandle) -> Result<Option<Vec<String>>, CommandError> {
    let picker_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || picker::pick_folder(&picker_app))
        .await
        .map_err(|_| CommandError::from(CommandErrorCode::PickerUnavailable))?
        .map_err(|picker::PickerError::Unavailable| {
            CommandError::from(CommandErrorCode::PickerUnavailable)
        })?;
    let Some(path) = chosen else {
        return Ok(None);
    };
    let folder = std::fs::canonicalize(&path)
        .map_err(|_| CommandError::from(CommandErrorCode::RootUnreadable))?;
    tauri::async_runtime::spawn_blocking(move || {
        let history = app.state::<History>();
        history
            .db
            .add_excluded_folder(&folder, unix_time_ms())
            .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))?;
        exclusions(&history, storage::home_dir().as_deref()).map(Some)
    })
    .await
    .map_err(blocking_failed)?
}

/// Stops skipping the excluded folder shown as `path`. Returns the list.
#[tauri::command]
pub async fn remove_excluded_folder(
    app: AppHandle,
    path: String,
) -> Result<Vec<String>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let history = app.state::<History>();
        let home = storage::home_dir();
        let stored = history
            .db
            .excluded_folders()
            .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))?;
        // Only a folder Rust has on its list can be named; anything else changes nothing.
        let found: Option<PathBuf> = stored
            .into_iter()
            .find(|folder| display_path(folder, home.as_deref()) == path);
        let folder = found.ok_or_else(|| CommandError::from(CommandErrorCode::InvalidRequest))?;
        history
            .db
            .remove_excluded_folder(&folder)
            .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))?;
        exclusions(&history, home.as_deref())
    })
    .await
    .map_err(blocking_failed)?
}

/// Stops Kleari reading a folder: a protected folder's remembered answer is forgotten (macOS's
/// own permission is changed in System Settings), and a folder added this session is dropped.
/// Returns the folders Kleari may read now.
#[tauri::command]
pub async fn remove_folder_access(
    app: AppHandle,
    source: FolderSource,
    path: String,
) -> Result<Vec<AccessFolder>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = storage::home_dir();
        let matches = |folder: &Path| display_path(folder, home.as_deref()) == path;
        let removed = match source {
            FolderSource::Protected => home.as_deref().and_then(|home| {
                ProtectedFolder::ALL
                    .into_iter()
                    .find(|folder| matches(&folder.path(home)))
                    .map(|folder| {
                        app.state::<History>()
                            .db
                            .forget_folder_access(folder.key())
                            .is_ok()
                    })
            }),
            FolderSource::Cleanup => {
                let state = app.state::<CleanupState>();
                state
                    .added_folders()
                    .into_iter()
                    .find(|folder| matches(folder))
                    .map(|folder| state.remove_folder(&folder))
            }
            FolderSource::Developer => {
                let state = app.state::<DeveloperState>();
                state
                    .added()
                    .into_iter()
                    .find(|folder| matches(folder))
                    .map(|folder| state.remove(&folder))
            }
            FolderSource::Restore => {
                let state = app.state::<QuarantineState>();
                state
                    .restore_folders()
                    .into_iter()
                    .find(|folder| matches(folder))
                    .map(|folder| state.revoke_restore_folder(&folder))
            }
        };
        match removed {
            Some(true) => Ok(folders(&app, home.as_deref())),
            Some(false) => Err(CommandErrorCode::HistoryUnavailable.into()),
            None => Err(CommandErrorCode::InvalidRequest.into()),
        }
    })
    .await
    .map_err(blocking_failed)?
}

/// Shows the quarantine folder in Finder.
#[tauri::command]
pub async fn reveal_quarantine_folder(app: AppHandle) -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = app.state::<History>().store.path();
        crate::finder::reveal(&app, vec![folder])
            .map_err(|_| CommandError::from(CommandErrorCode::RevealFailed))
    })
    .await
    .map_err(blocking_failed)?
}

/// Whether quarantine still holds anything: a record not yet restored or deleted, or anything
/// in its folder. Unknown counts as holding something.
fn quarantine_holds_items(history: &History) -> bool {
    let recorded = history
        .db
        .held_items()
        .map_or(true, |items| !items.is_empty());
    let on_disk = match std::fs::read_dir(history.store.path()) {
        Ok(entries) => entries
            .into_iter()
            .any(|entry| entry.map_or(true, |entry| entry.file_name() != ".DS_Store")),
        // Never created: nothing was ever quarantined there.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    recorded || on_disk
}

/// Uninstalls Kleari: tries to turn off Open at login (reported if macOS keeps it), then moves the app and everything it keeps on
/// this Mac to the Trash. Refused while quarantine holds anything (those are the user's
/// files) or another operation runs, and outside an installed Kleari.app: a development build
/// shares the installed app's data, and has no app of its own to remove. The app keeps running
/// until `quit_kleari`.
#[tauri::command]
pub async fn uninstall_kleari(
    app: AppHandle,
) -> Result<crate::uninstall::Uninstalled, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let operations = app.state::<QuarantineState>();
        let _guard = operations
            .begin()
            .map_err(|_| CommandError::from(CommandErrorCode::OperationInProgress))?;
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|executable| crate::uninstall::app_bundle(&executable))
            .ok_or_else(|| CommandError::from(CommandErrorCode::UninstallNeedsApp))?;
        let history = app.state::<History>();
        if quarantine_holds_items(&history) {
            return Err(CommandErrorCode::QuarantineNotEmpty.into());
        }
        let home = storage::home_dir()
            .ok_or_else(|| CommandError::from(CommandErrorCode::UninstallFailed))?;
        // A refusal doesn't stop the uninstall; it is reported, so you can remove the item in
        // System Settings › General › Login Items.
        let login_item_kept = if matches!(
            login_item::status(),
            LoginItem::Enabled | LoginItem::RequiresApproval
        ) {
            let change = login_item::set(false);
            match change.state {
                LoginItem::Enabled | LoginItem::RequiresApproval => Some(
                    change
                        .refusal
                        .unwrap_or_else(|| "macOS kept it on.".to_owned()),
                ),
                _ => None,
            }
        } else {
            None
        };
        let identifier = app.config().identifier.clone();
        let mut locations = crate::uninstall::data_locations(&home, &identifier);
        // The app itself goes last, after its data.
        locations.push(bundle);
        let mut done = crate::uninstall::move_to_trash(&locations, Some(&home));
        done.login_item_kept = login_item_kept;
        Ok(done)
    })
    .await
    .map_err(blocking_failed)?
}

/// Quits Kleari (after an uninstall, or from the menu bar).
#[tauri::command]
pub fn quit_kleari(app: AppHandle) {
    app.exit(0);
}

/// The activity kept, newest first.
#[tauri::command]
pub async fn list_activity(app: AppHandle) -> Result<ActivityList, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let history = app.state::<History>();
        let preferences = preferences::load(&history.db).preferences;
        activity::list(
            &history.db,
            storage::home_dir().as_deref(),
            preferences.retention_start_ms(unix_time_ms()),
            preferences.retention_days,
        )
    })
    .await
    .map_err(blocking_failed)
}

/// Deletes the activity log. Quarantine and its records are untouched.
#[tauri::command]
pub async fn clear_activity(app: AppHandle) -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<History>()
            .db
            .prune_events(u64::MAX)
            .map(|_| ())
            .map_err(|_| CommandError::from(CommandErrorCode::HistoryUnavailable))
    })
    .await
    .map_err(blocking_failed)?
}
