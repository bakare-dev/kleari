use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::schema::{migrate_with, version, MIGRATIONS};
use super::*;

fn temp(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("kleari-history-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("temp");
    std::fs::canonicalize(&path).expect("canonical")
}

fn item(id: &str, batch: &str, state: ItemState) -> ItemRecord {
    ItemRecord {
        id: id.to_owned(),
        batch_id: batch.to_owned(),
        name: format!("{id}.cache"),
        category: "cache".to_owned(),
        original: PathBuf::from(format!("/Users/someone/Library/Caches/{id}")),
        root: None,
        // Above i64::MAX: stored bit for bit.
        bytes: u64::MAX - 7,
        dev: u64::MAX,
        ino: 1 << 63,
        is_dir: true,
        state,
        updated_ms: 10,
    }
}

fn batch(id: &str, created_ms: u64) -> BatchRecord {
    BatchRecord {
        id: id.to_owned(),
        created_ms,
        origin: "standard".to_owned(),
    }
}

#[test]
fn a_new_history_is_migrated_to_the_latest_version_and_reopens_unchanged() {
    let folder = temp("fresh").join("data");
    let db = Database::open(&folder);
    assert_eq!(db.unavailable(), None);
    db.begin_batch(&batch("b1", 5), &[item("i1", "b1", ItemState::Held)])
        .expect("written");
    drop(db);

    let conn = Connection::open(folder.join(FILE_NAME)).expect("open");
    assert_eq!(
        version(&conn).expect("version"),
        i64::try_from(MIGRATIONS.len()).expect("len")
    );
    drop(conn);
    // Reopening applies nothing new and keeps the data.
    let db = Database::open(&folder);
    assert_eq!(db.held_items().expect("items").len(), 1);

    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&folder.join(FILE_NAME)), 0o600);
    std::fs::remove_dir_all(folder.parent().expect("parent")).expect("cleanup");
}

#[test]
fn a_version_1_history_keeps_its_items_and_gains_an_empty_root() {
    let folder = temp("v1");
    let path = folder.join(FILE_NAME);
    let mut conn = Connection::open(&path).expect("open");
    migrate_with(&mut conn, &MIGRATIONS[..1]).expect("v1");
    conn.execute_batch(
        "INSERT INTO quarantine_batch VALUES ('b1', 5, 'standard');
         INSERT INTO quarantine_item VALUES
             ('e1', 'b1', 'App', 'cache', CAST('/Users/someone/Library/Caches/App' AS BLOB),
              10, 1, 2, 1, 'held', 6);",
    )
    .expect("v1 item");
    drop(conn);

    let db = Database::open(&folder);
    assert_eq!(db.unavailable(), None);
    let items = db.held_items().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].root, None);
    assert_eq!(
        items[0].original,
        PathBuf::from("/Users/someone/Library/Caches/App")
    );
    // A recorded root reads back as written.
    let rooted = ItemRecord {
        id: "e2".to_owned(),
        root: Some(PathBuf::from("/opt/work")),
        ..item("e2", "b1", ItemState::Held)
    };
    db.begin_batch_items_for_tests(std::slice::from_ref(&rooted));
    assert_eq!(db.item("e2").expect("read"), Some(rooted));
    drop(db);
    std::fs::remove_dir_all(&folder).expect("cleanup");
}

#[test]
fn a_corrupt_file_is_reported_and_left_untouched() {
    let folder = temp("corrupt");
    let path = folder.join(FILE_NAME);
    let garbage = b"this is not a database, but someone's records may be in it".repeat(200);
    std::fs::write(&path, &garbage).expect("garbage");
    let db = Database::open(&folder);
    assert_eq!(db.unavailable(), Some(Unavailable::Corrupt));
    // Every operation fails closed.
    assert_eq!(db.held_items(), Err(Unavailable::Corrupt));
    assert_eq!(
        db.begin_batch(&batch("b", 1), &[]),
        Err(Unavailable::Corrupt)
    );
    drop(db);
    assert_eq!(std::fs::read(&path).expect("read"), garbage);
    std::fs::remove_dir_all(&folder).expect("cleanup");
}

#[test]
fn a_database_from_a_newer_version_is_refused_not_downgraded() {
    let folder = temp("newer");
    let path = folder.join(FILE_NAME);
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch("CREATE TABLE future (x INTEGER); PRAGMA user_version = 99;")
        .expect("future");
    drop(conn);
    let db = Database::open(&folder);
    assert_eq!(db.unavailable(), Some(Unavailable::NewerVersion));
    drop(db);
    let conn = Connection::open(&path).expect("reopen");
    assert_eq!(version(&conn).expect("version"), 99);
    // Refused before configuring: the journal mode the newer build chose is kept.
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode");
    assert_eq!(journal, "delete");
    assert!(!folder.join(format!("{FILE_NAME}-wal")).exists());
    std::fs::remove_dir_all(&folder).expect("cleanup");
}

#[test]
fn a_failed_migration_rolls_back_to_the_previous_version() {
    let mut conn = Connection::open_in_memory().expect("memory");
    let migrations = [
        MIGRATIONS[0],
        "CREATE TABLE ok (x INTEGER); THIS IS NOT SQL;",
    ];
    assert_eq!(
        migrate_with(&mut conn, &migrations),
        Err(Unavailable::MigrationFailed)
    );
    assert_eq!(version(&conn).expect("version"), 1);
    // The failed migration's first statement was rolled back with it.
    let ok_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'ok'",
            [],
            |row| row.get(0),
        )
        .expect("query");
    assert_eq!(ok_exists, 0);
}

#[test]
fn items_round_trip_bit_for_bit_and_states_change() {
    let db = Database::in_memory();
    db.begin_batch(
        &batch("b1", 100),
        &[
            item("i1", "b1", ItemState::Moving),
            item("i2", "b1", ItemState::Moving),
        ],
    )
    .expect("batch");
    let stored = db.item("i1").expect("read").expect("present");
    assert_eq!(stored, item("i1", "b1", ItemState::Moving));
    db.set_state("i1", ItemState::Held, 20).expect("held");
    db.set_state("i2", ItemState::NotMoved, 20)
        .expect("not moved");
    let held = db.held_items().expect("held");
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].id.as_str(), held[0].state, held[0].updated_ms),
        ("i1", ItemState::Held, 20)
    );
    // An unknown item is an error, never a silent success.
    assert_eq!(
        db.set_state("missing", ItemState::Held, 1),
        Err(Unavailable::WriteFailed)
    );
    assert_eq!(db.batches().expect("batches"), [batch("b1", 100)]);
}

#[test]
fn an_item_for_a_missing_batch_is_rejected_with_the_whole_batch() {
    let db = Database::in_memory();
    let result = db.begin_batch(
        &batch("b1", 1),
        &[
            item("i1", "b1", ItemState::Moving),
            item("i2", "nope", ItemState::Moving),
        ],
    );
    assert_eq!(result, Err(Unavailable::WriteFailed));
    // The transaction was rolled back: neither the batch nor the first item exist.
    assert!(db.batches().expect("batches").is_empty());
    assert_eq!(db.item("i1").expect("read"), None);
}

#[test]
fn interrupted_moves_are_resolved_from_the_quarantine_folder() {
    let db = Database::in_memory();
    db.begin_batch(
        &batch("b1", 1),
        &[
            item("moved", "b1", ItemState::Moving),
            item("stayed", "b1", ItemState::Moving),
            item("unknown", "b1", ItemState::Moving),
            item("back", "b1", ItemState::Restoring),
            item("done", "b1", ItemState::Held),
        ],
    )
    .expect("batch");
    let resolved = db
        .reconcile(
            |record| match record.id.as_str() {
                "moved" => Some(ItemState::Held),
                "stayed" => Some(ItemState::NotMoved),
                "back" => Some(ItemState::Restored),
                // The folder couldn't be inspected: no conclusion is drawn.
                _ => None,
            },
            50,
        )
        .expect("reconciled");
    assert_eq!(resolved, 3);
    assert_eq!(
        db.item("unknown").expect("read").map(|i| i.state),
        Some(ItemState::Moving)
    );
    assert_eq!(
        db.item("back").expect("read").map(|i| i.state),
        Some(ItemState::Restored)
    );
    assert_eq!(
        db.item("moved").expect("read").map(|i| i.state),
        Some(ItemState::Held)
    );
    assert_eq!(
        db.item("stayed").expect("read").map(|i| i.state),
        Some(ItemState::NotMoved)
    );
    assert_eq!(
        db.item("done").expect("read").map(|i| i.state),
        Some(ItemState::Held)
    );
}

#[test]
fn folder_access_is_remembered_and_updated() {
    let db = Database::in_memory();
    assert_eq!(db.folder_access("downloads").expect("read"), None);
    db.set_folder_access("downloads", AccessState::Denied, 1)
        .expect("denied");
    db.set_folder_access("downloads", AccessState::Granted, 2)
        .expect("granted");
    assert_eq!(
        db.folder_access("downloads").expect("read"),
        Some(AccessState::Granted)
    );
}

#[test]
fn events_record_counts_and_outcomes_without_paths() {
    let db = Database::in_memory();
    db.record_event(&Event {
        at_ms: 1,
        kind: EventKind::Quarantine,
        batch_id: Some("b1".to_owned()),
        items: 3,
        bytes: 10,
        failed: 1,
        outcome: "partial",
        subject: None,
        lines: Vec::new(),
    })
    .expect("event");
    assert_eq!(
        db.events().expect("events"),
        [("quarantine".to_owned(), "partial".to_owned(), 3)]
    );
}

#[test]
fn events_read_back_newest_first_with_their_items_and_are_pruned_by_age() {
    let db = Database::in_memory();
    db.begin_batch(
        &batch("b1", 5),
        &[
            item("e1", "b1", ItemState::Held),
            item("e2", "b1", ItemState::Held),
        ],
    )
    .expect("batch");
    let mut moved = Event::new(EventKind::Quarantine, 10, "partial", 2, 7);
    moved.lines = vec![("e1".to_owned(), "moved"), ("e2".to_owned(), "notMoved")];
    db.record_event(&moved).expect("moved");
    let mut scan = Event::new(EventKind::Scan, 20, "completed", 45, 1_000);
    scan.subject = Some("deep".to_owned());
    db.record_event(&scan).expect("scan");

    let events = db.events_since(0, 10).expect("events");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.kind, event.subject.clone()))
            .collect::<Vec<_>>(),
        [
            (EventKind::Scan, Some("deep".to_owned())),
            (EventKind::Quarantine, None)
        ]
    );
    let lines: Vec<(&str, &str)> = events[1]
        .lines
        .iter()
        .map(|line| (line.name.as_str(), line.outcome.as_str()))
        .collect();
    assert_eq!(lines, [("e1.cache", "moved"), ("e2.cache", "notMoved")]);
    assert_eq!(
        events[1].lines[0].original,
        PathBuf::from("/Users/someone/Library/Caches/e1")
    );
    // A window and a limit.
    assert_eq!(db.events_since(15, 10).expect("since").len(), 1);
    assert_eq!(db.events_since(0, 1).expect("limit").len(), 1);

    // Pruning drops old events and their lines, never the quarantine records.
    assert_eq!(db.prune_events(15), Ok(1));
    assert_eq!(db.events_since(0, 10).expect("left").len(), 1);
    assert_eq!(db.held_items().expect("held").len(), 2);
}

#[test]
fn the_activity_view_names_items_with_their_folder_and_caps_the_lines() {
    let db = Database::in_memory();
    let items: Vec<ItemRecord> = (0..25)
        .map(|n| item(&format!("e{n:02}"), "b1", ItemState::Held))
        .collect();
    db.begin_batch(&batch("b1", 5), &items).expect("batch");
    let mut moved = Event::new(EventKind::Quarantine, 10, "completed", 25, 1);
    moved.lines = items
        .iter()
        .map(|item| (item.id.clone(), "moved"))
        .collect();
    db.record_event(&moved).expect("moved");

    let crate::activity::ActivityList::Available {
        events,
        truncated,
        retention_days,
    } = crate::activity::list(&db, Some(Path::new("/Users/someone")), 0, 90)
    else {
        panic!("available");
    };
    assert!(!truncated);
    assert_eq!(retention_days, 90);
    assert_eq!(events[0].lines.len(), 20);
    assert_eq!(events[0].more_lines, 5);
    assert_eq!(events[0].lines[0].name, "e00.cache");
    assert_eq!(events[0].lines[0].folder, "~/Library/Caches");
    assert_eq!(events[0].lines[0].outcome, "moved");
    // Outside the home folder only the folder's own name is shown, never the full path.
    let crate::activity::ActivityList::Available { events, .. } =
        crate::activity::list(&db, Some(Path::new("/Users/other")), 0, 90)
    else {
        panic!("available");
    };
    assert_eq!(events[0].lines[0].folder, "…/Caches");
    // Outside the kept window nothing is listed.
    let crate::activity::ActivityList::Available { events, .. } =
        crate::activity::list(&db, None, 11, 90)
    else {
        panic!("available");
    };
    assert!(events.is_empty());
}

#[test]
fn settings_and_excluded_folders_are_kept_and_replaced() {
    let db = Database::in_memory();
    db.set_setting("history.retentionDays", "90").expect("set");
    db.set_setting("history.retentionDays", "30")
        .expect("replace");
    assert_eq!(
        db.settings().expect("settings"),
        [("history.retentionDays".to_owned(), "30".to_owned())]
    );
    let photos = Path::new("/Users/someone/Pictures/Photos Library.photoslibrary");
    assert_eq!(db.add_excluded_folder(photos, 1), Ok(true));
    assert_eq!(db.add_excluded_folder(photos, 2), Ok(false));
    assert_eq!(db.excluded_folders().expect("list"), [photos.to_path_buf()]);
    db.remove_excluded_folder(photos).expect("remove");
    assert!(db.excluded_folders().expect("empty").is_empty());

    db.set_folder_access("downloads", AccessState::Granted, 1)
        .expect("granted");
    db.forget_folder_access("downloads").expect("forget");
    assert_eq!(db.folder_access("downloads"), Ok(None));
}
