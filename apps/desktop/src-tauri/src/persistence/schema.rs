//! Numbered schema migrations. `PRAGMA user_version` records the last one applied; each runs
//! in its own transaction, together with the version bump, so a failure leaves the previous
//! version intact. Migrations are only ever appended, never edited once released.

use rusqlite::Connection;

use super::Unavailable;

pub(super) const MIGRATIONS: &[&str] = &[
    // 1: quarantine batches and items, operation outcomes, protected-folder access.
    "CREATE TABLE quarantine_batch (
         id TEXT PRIMARY KEY NOT NULL,
         created_ms INTEGER NOT NULL,
         origin TEXT NOT NULL
     );
     CREATE TABLE quarantine_item (
         id TEXT PRIMARY KEY NOT NULL,
         batch_id TEXT NOT NULL REFERENCES quarantine_batch (id),
         name TEXT NOT NULL,
         category TEXT NOT NULL,
         original BLOB NOT NULL,
         bytes INTEGER NOT NULL,
         dev INTEGER NOT NULL,
         ino INTEGER NOT NULL,
         is_dir INTEGER NOT NULL,
         state TEXT NOT NULL,
         updated_ms INTEGER NOT NULL
     );
     CREATE INDEX quarantine_item_state ON quarantine_item (state);
     CREATE TABLE history_event (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         at_ms INTEGER NOT NULL,
         kind TEXT NOT NULL,
         batch_id TEXT,
         items INTEGER NOT NULL,
         bytes INTEGER NOT NULL,
         failed INTEGER NOT NULL,
         outcome TEXT NOT NULL
     );
     CREATE TABLE folder_access (
         folder TEXT PRIMARY KEY NOT NULL,
         state TEXT NOT NULL,
         updated_ms INTEGER NOT NULL
     );",
    // 2: the added folder a build folder was approved under, when it was outside the home
    // folder, so its original place can be checked before a restore.
    "ALTER TABLE quarantine_item ADD COLUMN root BLOB;",
    // 3: what each event was about (a scan mode, a folder, an app's name) and which
    // quarantined items it concerned, for the Activity page; preferences; folders scans skip.
    "CREATE TABLE setting (
         key TEXT PRIMARY KEY NOT NULL,
         value TEXT NOT NULL
     );
     CREATE TABLE excluded_folder (
         path BLOB PRIMARY KEY NOT NULL,
         added_ms INTEGER NOT NULL
     );
     ALTER TABLE history_event ADD COLUMN subject TEXT;
     CREATE TABLE history_event_item (
         event_id INTEGER NOT NULL REFERENCES history_event (id),
         item_id TEXT NOT NULL,
         outcome TEXT NOT NULL
     );
     CREATE INDEX history_event_item_event ON history_event_item (event_id);
     CREATE INDEX history_event_at ON history_event (at_ms);",
];

pub(super) fn version(conn: &Connection) -> Result<i64, Unavailable> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| Unavailable::Corrupt)
}

/// Refuses a database a newer build migrated past what this one knows.
pub fn refuse_newer(conn: &Connection) -> Result<(), Unavailable> {
    let known = i64::try_from(MIGRATIONS.len()).map_err(|_| Unavailable::MigrationFailed)?;
    if version(conn)? > known {
        return Err(Unavailable::NewerVersion);
    }
    Ok(())
}

/// Applies every migration after the database's version. A database at a version this build
/// doesn't know is refused, never downgraded.
pub fn migrate(conn: &mut Connection) -> Result<(), Unavailable> {
    migrate_with(conn, MIGRATIONS)
}

pub(super) fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<(), Unavailable> {
    let current = version(conn)?;
    let known = i64::try_from(migrations.len()).map_err(|_| Unavailable::MigrationFailed)?;
    if current > known {
        return Err(Unavailable::NewerVersion);
    }
    let start = usize::try_from(current).map_err(|_| Unavailable::Corrupt)?;
    for (index, sql) in migrations.iter().enumerate().skip(start) {
        let next = i64::try_from(index + 1).map_err(|_| Unavailable::MigrationFailed)?;
        let tx = conn
            .transaction()
            .map_err(|_| Unavailable::MigrationFailed)?;
        tx.execute_batch(sql)
            .map_err(|_| Unavailable::MigrationFailed)?;
        // PRAGMA takes no bound parameters; `next` is an integer this function computed.
        tx.execute_batch(&format!("PRAGMA user_version = {next}"))
            .map_err(|_| Unavailable::MigrationFailed)?;
        tx.commit().map_err(|_| Unavailable::MigrationFailed)?;
    }
    Ok(())
}
