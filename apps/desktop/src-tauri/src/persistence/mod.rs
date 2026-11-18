//! Local history in SQLite (ADR-006): what each cleanup moved into quarantine, where each item
//! came from (so it can be restored), what happened to it since, the outcome of each
//! operation, and whether the user granted access to a protected folder. Nothing leaves this
//! Mac.
//!
//! Integrity comes before availability. The schema is created and upgraded by explicit,
//! numbered migrations inside a transaction. A database that fails its integrity check, or
//! that a newer Kleari migrated past what this one knows, is never repaired, recreated or
//! downgraded: it is reported as unavailable and left as it is, and every operation that would
//! need a record fails closed (nothing moves without a record of where it came from).

mod schema;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::Serialize;

/// File name of the database inside the app's data folder.
pub const FILE_NAME: &str = "history.sqlite3";

/// Why the history can't be used. The file is left untouched in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unavailable {
    /// The app's data folder or the database file couldn't be opened or created.
    CannotOpen,
    /// SQLite's integrity check failed.
    Corrupt,
    /// A newer Kleari changed the schema; this version won't read or write it.
    NewerVersion,
    /// A migration failed; it was rolled back.
    MigrationFailed,
    /// A read or write failed after opening (for example, the disk is full).
    WriteFailed,
}

/// What became of a quarantined item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    /// Recorded just before the move; the move's outcome isn't known yet (it is reconciled
    /// against the quarantine folder when the history next opens).
    Moving,
    /// In the quarantine folder.
    Held,
    /// Recorded just before a restore; resolved like `Moving` if the outcome wasn't recorded.
    Restoring,
    /// Put back where it came from.
    Restored,
    /// Permanently deleted from the quarantine folder.
    Deleted,
    /// The move didn't happen; the item stayed where it was.
    NotMoved,
}

impl ItemState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Moving => "moving",
            Self::Held => "held",
            Self::Restoring => "restoring",
            Self::Restored => "restored",
            Self::Deleted => "deleted",
            Self::NotMoved => "notMoved",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "moving" => Self::Moving,
            "held" => Self::Held,
            "restoring" => Self::Restoring,
            "restored" => Self::Restored,
            "deleted" => Self::Deleted,
            "notMoved" => Self::NotMoved,
            _ => return None,
        })
    }
}

/// One item as recorded. `original` is the absolute path it was moved from (needed to put it
/// back); it is never sent to the frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRecord {
    pub id: String,
    pub batch_id: String,
    pub name: String,
    pub category: String,
    pub original: PathBuf,
    /// The folder added through the picker that the item was approved under, for a build
    /// folder outside the home folder; `None` for everything else.
    pub root: Option<PathBuf>,
    pub bytes: u64,
    pub dev: u64,
    pub ino: u64,
    pub is_dir: bool,
    pub state: ItemState,
    pub updated_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRecord {
    pub id: String,
    pub created_ms: u64,
    /// The scan the items came from (`standard`, `deep`, `dev`), or `sent` for items sent
    /// from Storage or Developer.
    pub origin: String,
}

/// The kind of operation an event records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Quarantine,
    Restore,
    Delete,
    /// A Cleanup scan (`subject`: its mode).
    Scan,
    /// A Storage analysis.
    Analysis,
    /// A Developer scan.
    DeveloperScan,
    /// Items sent to Cleanup's review (`subject`: `developer` or `storage`).
    Review,
    /// An answer to a protected-folder request (`subject`: the folder's ID).
    Access,
    /// An app asked to quit from Processes (`subject`: its display name).
    Quit,
    /// An app moved to the Trash from Apps (`subject`: its display name).
    AppRemoved,
    /// A Maintenance task (`subject`: the task's ID; outcome done, cancelled or failed).
    Maintenance,
}

impl EventKind {
    const ALL: [Self; 11] = [
        Self::Quarantine,
        Self::Restore,
        Self::Delete,
        Self::Scan,
        Self::Analysis,
        Self::DeveloperScan,
        Self::Review,
        Self::Access,
        Self::Quit,
        Self::AppRemoved,
        Self::Maintenance,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Quarantine => "quarantine",
            Self::Restore => "restore",
            Self::Delete => "delete",
            Self::Scan => "scan",
            Self::Analysis => "analysis",
            Self::DeveloperScan => "developerScan",
            Self::Review => "review",
            Self::Access => "access",
            Self::Quit => "quit",
            Self::AppRemoved => "appRemoved",
            Self::Maintenance => "maintenance",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// One operation's outcome: counts, sizes, an outcome code, what it was about (`subject`, never
/// a path) and the quarantined items it concerned, each with its own outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub at_ms: u64,
    pub kind: EventKind,
    pub batch_id: Option<String>,
    pub items: u64,
    pub bytes: u64,
    pub failed: u64,
    /// `completed`, `partial`, `stopped`, `cancelled`, `failed`; for access `granted` or
    /// `denied`; for a quit `requested` or `refused`.
    pub outcome: &'static str,
    pub subject: Option<String>,
    /// Quarantine item IDs with what happened to each (`moved`, `restored`, `conflict`, …).
    pub lines: Vec<(String, &'static str)>,
}

impl Event {
    /// An event about no quarantined items.
    pub fn new(kind: EventKind, at_ms: u64, outcome: &'static str, items: u64, bytes: u64) -> Self {
        Self {
            at_ms,
            kind,
            batch_id: None,
            items,
            bytes,
            failed: 0,
            outcome,
            subject: None,
            lines: Vec::new(),
        }
    }
}

/// An event as read back, with its items' names and original folders joined from the
/// quarantine records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub id: i64,
    pub at_ms: u64,
    pub kind: EventKind,
    pub items: u64,
    pub bytes: u64,
    pub failed: u64,
    pub outcome: String,
    pub subject: Option<String>,
    pub lines: Vec<StoredLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLine {
    pub name: String,
    pub category: String,
    pub original: PathBuf,
    pub bytes: u64,
    pub outcome: String,
}

/// Access the user gave (or refused) for a protected folder, as last observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessState {
    Granted,
    Denied,
}

// SQLite integers are signed 64-bit. Identities and sizes are stored bit for bit, so values
// above i64::MAX survive the round trip.
const fn to_sql(value: u64) -> i64 {
    i64::from_ne_bytes(value.to_ne_bytes())
}

const fn from_sql(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes()
}

fn path_from(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// The open history, or why it can't be used.
pub struct Database {
    conn: Mutex<Result<Connection, Unavailable>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every write runs in a transaction, so a panic mid-write leaves the database consistent
    // and the connection reusable.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_failed(_: rusqlite::Error) -> Unavailable {
    Unavailable::WriteFailed
}

impl Database {
    /// Opens (creating if needed) the history in `folder`, checks its integrity and applies
    /// any pending migrations.
    pub fn open(folder: &Path) -> Self {
        Self {
            conn: Mutex::new(open_checked(folder)),
        }
    }

    /// A history that couldn't be opened at all (no data folder).
    pub fn failed(reason: Unavailable) -> Self {
        Self {
            conn: Mutex::new(Err(reason)),
        }
    }

    /// An in-memory history, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory()
            .map_err(|_| Unavailable::CannotOpen)
            .and_then(|mut conn| {
                schema::migrate(&mut conn)?;
                Ok(conn)
            });
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// `None` when the history is usable.
    pub fn unavailable(&self) -> Option<Unavailable> {
        lock(&self.conn).as_ref().err().copied()
    }

    fn with<T>(
        &self,
        run: impl FnOnce(&mut Connection) -> Result<T, Unavailable>,
    ) -> Result<T, Unavailable> {
        let mut guard = lock(&self.conn);
        match guard.as_mut() {
            Ok(conn) => run(conn),
            Err(reason) => Err(*reason),
        }
    }

    /// Records a new batch and its items as `moving`, in one transaction, before anything is
    /// moved.
    pub fn begin_batch(
        &self,
        batch: &BatchRecord,
        items: &[ItemRecord],
    ) -> Result<(), Unavailable> {
        self.with(|conn| {
            let tx = conn.transaction().map_err(write_failed)?;
            tx.execute(
                "INSERT INTO quarantine_batch (id, created_ms, origin) VALUES (?1, ?2, ?3)",
                params![batch.id, to_sql(batch.created_ms), batch.origin],
            )
            .map_err(write_failed)?;
            for item in items {
                insert_item(&tx, item)?;
            }
            tx.commit().map_err(write_failed)
        })
    }

    /// Records an item's new state.
    pub fn set_state(
        &self,
        item_id: &str,
        state: ItemState,
        at_ms: u64,
    ) -> Result<(), Unavailable> {
        self.with(|conn| {
            let changed = conn
                .execute(
                    "UPDATE quarantine_item SET state = ?1, updated_ms = ?2 WHERE id = ?3",
                    params![state.as_str(), to_sql(at_ms), item_id],
                )
                .map_err(write_failed)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(Unavailable::WriteFailed)
            }
        })
    }

    /// Records an event and its items in one transaction.
    pub fn record_event(&self, event: &Event) -> Result<(), Unavailable> {
        self.with(|conn| {
            let tx = conn.transaction().map_err(write_failed)?;
            tx.execute(
                "INSERT INTO history_event
                 (at_ms, kind, batch_id, items, bytes, failed, outcome, subject)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    to_sql(event.at_ms),
                    event.kind.as_str(),
                    event.batch_id,
                    to_sql(event.items),
                    to_sql(event.bytes),
                    to_sql(event.failed),
                    event.outcome,
                    event.subject
                ],
            )
            .map_err(write_failed)?;
            let event_id = tx.last_insert_rowid();
            for (item_id, outcome) in &event.lines {
                tx.execute(
                    "INSERT INTO history_event_item (event_id, item_id, outcome)
                     VALUES (?1, ?2, ?3)",
                    params![event_id, item_id, outcome],
                )
                .map_err(write_failed)?;
            }
            tx.commit().map_err(write_failed)
        })
    }

    /// Events at or after `since_ms`, newest first, at most `limit`, each with its items
    /// (names and original places from the quarantine records).
    pub fn events_since(&self, since_ms: u64, limit: u32) -> Result<Vec<StoredEvent>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT id, at_ms, kind, items, bytes, failed, outcome, subject
                     FROM history_event WHERE at_ms >= ?1 ORDER BY at_ms DESC, id DESC LIMIT ?2",
                )
                .map_err(write_failed)?;
            let rows = statement
                .query_map(params![to_sql(since_ms), limit], |row| {
                    let kind: String = row.get(2)?;
                    Ok((
                        row.get::<_, i64>(0)?,
                        from_sql(row.get(1)?),
                        kind,
                        from_sql(row.get(3)?),
                        from_sql(row.get(4)?),
                        from_sql(row.get(5)?),
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                })
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            let mut lines = conn
                .prepare(
                    "SELECT q.name, q.category, q.original, q.bytes, e.outcome
                     FROM history_event_item e JOIN quarantine_item q ON q.id = e.item_id
                     WHERE e.event_id = ?1 ORDER BY e.rowid",
                )
                .map_err(write_failed)?;
            rows.into_iter()
                .map(
                    |(id, at_ms, kind, items, bytes, failed, outcome, subject)| {
                        let kind = EventKind::parse(&kind).ok_or(Unavailable::Corrupt)?;
                        let lines = lines
                            .query_map([id], |row| {
                                Ok(StoredLine {
                                    name: row.get(0)?,
                                    category: row.get(1)?,
                                    original: path_from(row.get(2)?),
                                    bytes: from_sql(row.get(3)?),
                                    outcome: row.get(4)?,
                                })
                            })
                            .map_err(write_failed)?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(write_failed)?;
                        Ok(StoredEvent {
                            id,
                            at_ms,
                            kind,
                            items,
                            bytes,
                            failed,
                            outcome,
                            subject,
                            lines,
                        })
                    },
                )
                .collect()
        })
    }

    /// Deletes events older than `before_ms`, with their item lines. Quarantine records are
    /// kept: they are needed to restore what is still held. Returns how many were deleted.
    pub fn prune_events(&self, before_ms: u64) -> Result<usize, Unavailable> {
        self.with(|conn| {
            let tx = conn.transaction().map_err(write_failed)?;
            tx.execute(
                "DELETE FROM history_event_item WHERE event_id IN
                 (SELECT id FROM history_event WHERE at_ms < ?1)",
                [to_sql(before_ms)],
            )
            .map_err(write_failed)?;
            let deleted = tx
                .execute(
                    "DELETE FROM history_event WHERE at_ms < ?1",
                    [to_sql(before_ms)],
                )
                .map_err(write_failed)?;
            tx.commit().map_err(write_failed)?;
            Ok(deleted)
        })
    }

    /// Items still held in quarantine (and any whose move is unresolved), oldest batch first.
    pub fn held_items(&self) -> Result<Vec<ItemRecord>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT i.id, i.batch_id, i.name, i.category, i.original, i.bytes, i.dev,
                            i.ino, i.is_dir, i.state, i.updated_ms, i.root
                     FROM quarantine_item i JOIN quarantine_batch b ON b.id = i.batch_id
                     WHERE i.state IN ('held', 'moving', 'restoring')
                     ORDER BY b.created_ms, i.rowid",
                )
                .map_err(write_failed)?;
            let rows = statement
                .query_map([], read_item)
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            rows.into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or(Unavailable::Corrupt)
        })
    }

    pub fn item(&self, item_id: &str) -> Result<Option<ItemRecord>, Unavailable> {
        self.with(|conn| {
            let row = conn
                .query_row(
                    "SELECT id, batch_id, name, category, original, bytes, dev, ino, is_dir,
                            state, updated_ms, root
                     FROM quarantine_item WHERE id = ?1",
                    [item_id],
                    read_item,
                )
                .optional()
                .map_err(write_failed)?;
            match row {
                Some(Some(item)) => Ok(Some(item)),
                Some(None) => Err(Unavailable::Corrupt),
                None => Ok(None),
            }
        })
    }

    pub fn batches(&self) -> Result<Vec<BatchRecord>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare("SELECT id, created_ms, origin FROM quarantine_batch ORDER BY created_ms")
                .map_err(write_failed)?;
            let rows = statement
                .query_map([], |row| {
                    Ok(BatchRecord {
                        id: row.get(0)?,
                        created_ms: from_sql(row.get(1)?),
                        origin: row.get(2)?,
                    })
                })
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            Ok(rows)
        })
    }

    pub fn folder_access(&self, folder: &str) -> Result<Option<AccessState>, Unavailable> {
        self.with(|conn| {
            let state: Option<String> = conn
                .query_row(
                    "SELECT state FROM folder_access WHERE folder = ?1",
                    [folder],
                    |row| row.get(0),
                )
                .optional()
                .map_err(write_failed)?;
            Ok(state.and_then(|state| match state.as_str() {
                "granted" => Some(AccessState::Granted),
                "denied" => Some(AccessState::Denied),
                _ => None,
            }))
        })
    }

    pub fn set_folder_access(
        &self,
        folder: &str,
        state: AccessState,
        at_ms: u64,
    ) -> Result<(), Unavailable> {
        let text = match state {
            AccessState::Granted => "granted",
            AccessState::Denied => "denied",
        };
        self.with(|conn| {
            conn.execute(
                "INSERT INTO folder_access (folder, state, updated_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT (folder) DO UPDATE SET state = excluded.state, updated_ms = excluded.updated_ms",
                params![folder, text, to_sql(at_ms)],
            )
            .map(|_| ())
            .map_err(write_failed)
        })
    }

    /// Forgets the remembered answer for a protected folder, so Kleari doesn't read it again
    /// until Grant access is chosen. (macOS's own permission is changed in System Settings.)
    pub fn forget_folder_access(&self, folder: &str) -> Result<(), Unavailable> {
        self.with(|conn| {
            conn.execute("DELETE FROM folder_access WHERE folder = ?1", [folder])
                .map(|_| ())
                .map_err(write_failed)
        })
    }

    /// When the latest kept event of `kind` happened (for the weekly cleanup reminder).
    pub fn last_event_ms(&self, kind: EventKind) -> Result<Option<u64>, Unavailable> {
        self.with(|conn| {
            conn.query_row(
                "SELECT MAX(at_ms) FROM history_event WHERE kind = ?1",
                params![kind.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map(|at| at.map(from_sql))
            .map_err(write_failed)
        })
    }

    /// Every saved preference, as stored text by key.
    pub fn settings(&self) -> Result<Vec<(String, String)>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare("SELECT key, value FROM setting ORDER BY key")
                .map_err(write_failed)?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            Ok(rows)
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), Unavailable> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(write_failed)
        })
    }

    /// Folders scans skip, in the order they were added.
    pub fn excluded_folders(&self) -> Result<Vec<PathBuf>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare("SELECT path FROM excluded_folder ORDER BY added_ms, rowid")
                .map_err(write_failed)?;
            let rows = statement
                .query_map([], |row| row.get::<_, Vec<u8>>(0))
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            Ok(rows.into_iter().map(path_from).collect())
        })
    }

    /// Adds a folder scans skip; `false` if it was already there.
    pub fn add_excluded_folder(&self, path: &Path, at_ms: u64) -> Result<bool, Unavailable> {
        self.with(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO excluded_folder (path, added_ms) VALUES (?1, ?2)",
                params![path_bytes(path), to_sql(at_ms)],
            )
            .map(|changed| changed == 1)
            .map_err(write_failed)
        })
    }

    pub fn remove_excluded_folder(&self, path: &Path) -> Result<(), Unavailable> {
        self.with(|conn| {
            conn.execute(
                "DELETE FROM excluded_folder WHERE path = ?1",
                [path_bytes(path)],
            )
            .map(|_| ())
            .map_err(write_failed)
        })
    }

    /// Resolves items left `moving` by an interrupted cleanup: `held` if the quarantine folder
    /// has them (`in_store` says so), otherwise `notMoved`. Returns how many were resolved.
    pub fn reconcile(
        &self,
        resolve: impl Fn(&ItemRecord) -> Option<ItemState>,
        at_ms: u64,
    ) -> Result<usize, Unavailable> {
        let pending: Vec<ItemRecord> = self
            .held_items()?
            .into_iter()
            .filter(|item| matches!(item.state, ItemState::Moving | ItemState::Restoring))
            .collect();
        let mut resolved = 0;
        for item in &pending {
            // `None`: the folders couldn't be inspected; the record stays for next time.
            if let Some(state) = resolve(item) {
                self.set_state(&item.id, state, at_ms)?;
                resolved += 1;
            }
        }
        Ok(resolved)
    }

    /// Records the identity of what is actually held, when it differs from the one checked
    /// before the move (an entry swapped in that couldn't be moved back).
    pub fn set_identity(
        &self,
        item_id: &str,
        dev: u64,
        ino: u64,
        is_dir: bool,
    ) -> Result<(), Unavailable> {
        self.with(|conn| {
            let changed = conn
                .execute(
                    "UPDATE quarantine_item SET dev = ?1, ino = ?2, is_dir = ?3 WHERE id = ?4",
                    params![to_sql(dev), to_sql(ino), is_dir, item_id],
                )
                .map_err(write_failed)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(Unavailable::WriteFailed)
            }
        })
    }

    /// Adds items to an existing batch.
    #[cfg(test)]
    pub fn begin_batch_items_for_tests(&self, items: &[ItemRecord]) {
        self.with(|conn| {
            let tx = conn.transaction().map_err(write_failed)?;
            for item in items {
                insert_item(&tx, item)?;
            }
            tx.commit().map_err(write_failed)
        })
        .expect("items inserted");
    }

    /// Rewrites an item's original path, as an edited history file would.
    #[cfg(test)]
    pub fn set_original_for_tests(&self, item_id: &str, original: &std::path::Path) {
        self.with(|conn| {
            conn.execute(
                "UPDATE quarantine_item SET original = ?1 WHERE id = ?2",
                params![path_bytes(original), item_id],
            )
            .map(|_| ())
            .map_err(write_failed)
        })
        .expect("original rewritten");
    }

    #[cfg(test)]
    fn events(&self) -> Result<Vec<(String, String, u64)>, Unavailable> {
        self.with(|conn| {
            let mut statement = conn
                .prepare("SELECT kind, outcome, items FROM history_event ORDER BY id")
                .map_err(write_failed)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, from_sql(row.get(2)?)))
                })
                .map_err(write_failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(write_failed)?;
            Ok(rows)
        })
    }
}

fn insert_item(tx: &Transaction<'_>, item: &ItemRecord) -> Result<(), Unavailable> {
    tx.execute(
        "INSERT INTO quarantine_item
         (id, batch_id, name, category, original, bytes, dev, ino, is_dir, state, updated_ms,
          root)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            item.id,
            item.batch_id,
            item.name,
            item.category,
            path_bytes(&item.original),
            to_sql(item.bytes),
            to_sql(item.dev),
            to_sql(item.ino),
            item.is_dir,
            item.state.as_str(),
            to_sql(item.updated_ms),
            item.root.as_deref().map(path_bytes)
        ],
    )
    .map(|_| ())
    .map_err(write_failed)
}

/// Reads one item row. `Ok(None)` for a row whose state isn't one this version knows.
fn read_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<ItemRecord>> {
    let state: String = row.get(9)?;
    let Some(state) = ItemState::parse(&state) else {
        return Ok(None);
    };
    Ok(Some(ItemRecord {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        name: row.get(2)?,
        category: row.get(3)?,
        original: path_from(row.get(4)?),
        bytes: from_sql(row.get(5)?),
        dev: from_sql(row.get(6)?),
        ino: from_sql(row.get(7)?),
        is_dir: row.get(8)?,
        state,
        updated_ms: from_sql(row.get(10)?),
        root: row.get::<_, Option<Vec<u8>>>(11)?.map(path_from),
    }))
}

fn open_checked(folder: &Path) -> Result<Connection, Unavailable> {
    create_private_folder(folder)?;
    let path = folder.join(FILE_NAME);
    let mut conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| Unavailable::CannotOpen)?;
    // Only the app's own account should read where items came from.
    restrict_to_owner(&path)?;
    check_integrity(&conn)?;
    // A newer Kleari's file is refused before anything persistent (the journal mode) is
    // changed.
    schema::refuse_newer(&conn)?;
    configure(&conn)?;
    schema::migrate(&mut conn)?;
    Ok(conn)
}

/// The data folder, created owner-only if missing. A symlink in its place is refused.
fn create_private_folder(folder: &Path) -> Result<(), Unavailable> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::symlink_metadata(folder) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(Unavailable::CannotOpen),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(folder)
            .map_err(|_| Unavailable::CannotOpen),
        Err(_) => Err(Unavailable::CannotOpen),
    }
}

fn restrict_to_owner(path: &Path) -> Result<(), Unavailable> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Unavailable::CannotOpen)?;
    if !metadata.is_file() {
        return Err(Unavailable::CannotOpen);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| Unavailable::CannotOpen)?;
    }
    Ok(())
}

fn check_integrity(conn: &Connection) -> Result<(), Unavailable> {
    let result: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|_| Unavailable::Corrupt)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(Unavailable::Corrupt)
    }
}

fn configure(conn: &Connection) -> Result<(), Unavailable> {
    // FULL sync: a recorded move survives a power loss. Foreign keys keep items tied to their
    // batch.
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
        .map_err(|_| Unavailable::CannotOpen)?;
    let _: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|_| Unavailable::CannotOpen)?;
    Ok(())
}
