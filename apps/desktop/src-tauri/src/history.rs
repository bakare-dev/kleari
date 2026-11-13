//! The app's local record and its quarantine folder, opened once at launch in the app's data
//! folder (`~/Library/Application Support/<bundle id>`).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::persistence::{Database, Event, Unavailable};
use crate::quarantine::{Scope, Store};

pub struct History {
    pub db: Database,
    pub store: Store,
}

pub fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

impl History {
    /// The folders excluded in Settings. If they can't be read, nothing that depends on them
    /// may go ahead as if there were none: callers refuse, or leave out what they would have
    /// shown.
    pub fn exclusions(&self) -> Result<Vec<PathBuf>, crate::commands::CommandError> {
        self.db
            .excluded_folders()
            .map_err(|_| crate::commands::CommandErrorCode::HistoryUnavailable.into())
    }

    /// Records an event for the Activity page. The operation it describes already happened,
    /// so a failed write never fails it; the page then lacks that entry.
    pub fn note(&self, event: &Event) {
        let _ = self.db.record_event(event);
    }

    /// Opens the history in `data_folder` and resolves any move a previous run left
    /// unfinished (held if the quarantine folder has it, otherwise not moved).
    pub fn open(data_folder: Option<PathBuf>) -> Self {
        let Some(folder) = data_folder else {
            return Self {
                db: Database::failed(Unavailable::CannotOpen),
                store: Store::new(PathBuf::new()),
            };
        };
        let db = Database::open(&folder);
        let store = Store::new(folder);
        // An unreadable history stays as it is; nothing is resolved until it opens. An item
        // whose state can't be told from the folders stays unresolved too, and so does one
        // whose recorded original isn't a place its category comes from: its outcome is never
        // concluded from that path.
        let (home, temp) = (crate::storage::home_dir(), crate::storage::temp_dir());
        // An original place in a folder excluded in Settings isn't looked at either; if the
        // exclusions can't be read, no original place is (only what the quarantine folder
        // proves settles a record).
        let excluded = db.excluded_folders().ok();
        // Nothing has been chosen in the folder panel yet at launch.
        let scope = home
            .as_deref()
            .zip(excluded.as_deref())
            .map(|(home, excluded)| Scope {
                home,
                temp: temp.as_deref(),
                added: &[],
                excluded,
            });
        let _ = crate::quarantine::reconcile(&db, &store, scope.as_ref(), unix_time_ms());
        // Activity older than the user keeps is deleted; quarantine records are kept. An
        // unreadable history prunes nothing.
        let preferences = crate::preferences::load(&db).preferences;
        let _ = db.prune_events(preferences.retention_start_ms(unix_time_ms()));
        Self { db, store }
    }
}
