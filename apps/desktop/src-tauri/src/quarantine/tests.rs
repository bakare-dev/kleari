use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::service::{ItemStatus, RestoreOutcome};
use super::*;
use crate::persistence::{BatchRecord, Database, ItemRecord, ItemState};
use crate::storage::fsdir::DirHandle;

/// A disposable tree: `home/` holds items, `data/` is the app's data folder.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("kleari-quarantine-{tag}-{}", std::process::id()));
        let _ = make_writable_and_remove(&root);
        fs::create_dir_all(root.join("home")).expect("home");
        fs::create_dir_all(root.join("data")).expect("data");
        Self {
            root: fs::canonicalize(&root).expect("canonical"),
        }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn store(&self) -> Store {
        Store::new(self.root.join("data"))
    }

    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.home().join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(&path, text).expect("file");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = make_writable_and_remove(&self.root);
    }
}

fn make_writable_and_remove(path: &Path) -> std::io::Result<()> {
    if let Ok(entries) = fs::read_dir(path) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        for entry in entries.flatten() {
            let child = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let _ = fs::set_permissions(&child, fs::Permissions::from_mode(0o700));
                make_writable_and_remove(&child)?;
            }
        }
    }
    fs::remove_dir_all(path).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        }
    })
}

fn identity(path: &Path) -> Identity {
    let parent = path.parent().expect("parent");
    let (dir, _) = DirHandle::open_root(parent).expect("open parent");
    let name =
        std::ffi::CString::new(path.file_name().expect("name").as_encoded_bytes()).expect("c");
    Identity::of(&dir.stat_child(&name).expect("stat"))
}

#[test]
fn an_item_moves_in_and_restores_with_its_contents_intact() {
    let fixture = Fixture::new("roundtrip");
    fixture.write("Library/Caches/com.example/a/b.bin", "payload");
    let item = fixture.home().join("Library/Caches/com.example");
    let expected = identity(&item);
    let store = fixture.store();

    let moved = store.move_in(&item, expected, "b1", "e1").expect("moved");
    assert_eq!(moved, expected);
    assert!(!item.exists());
    assert!(store.holds("b1", "e1", expected));
    assert_eq!(
        fs::read_to_string(store.item_path("b1", "e1").join("a/b.bin")).expect("held"),
        "payload"
    );
    // The quarantine folder is owner-only.
    let mode = fs::metadata(store.path())
        .expect("store")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);

    store
        .restore("b1", "e1", expected, &item)
        .expect("restored");
    assert_eq!(
        fs::read_to_string(item.join("a/b.bin")).expect("back"),
        "payload"
    );
    // The emptied batch folder is removed; the quarantine folder stays.
    assert!(!store.path().join("b1").exists());
    assert!(store.path().exists());
}

/// Replaces the folder at `path` with a new one holding `file`. The new folder is made before
/// the old one is removed, so it always has a different inode (ext4 reuses a freed one at
/// once, which would make a delete-then-create look unchanged).
fn replace_dir(path: &Path, file: &str) {
    let fresh = path.with_file_name(format!(
        "{}.replacement",
        path.file_name().expect("name").to_string_lossy()
    ));
    fs::create_dir(&fresh).expect("replacement");
    fs::write(fresh.join(file), file).expect("file");
    fs::remove_dir_all(path).expect("remove");
    fs::rename(&fresh, path).expect("swap in");
}

#[test]
fn a_replaced_item_is_not_moved() {
    let fixture = Fixture::new("replaced");
    fixture.write("Library/Caches/app/old", "old");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);
    replace_dir(&item, "new");

    assert_eq!(
        fixture.store().move_in(&item, expected, "b1", "e1"),
        Err(Failure::Changed)
    );
    assert_eq!(
        fs::read_to_string(item.join("new")).expect("untouched"),
        "new"
    );
}

#[test]
fn a_symlink_in_place_of_the_item_or_on_its_path_is_never_moved() {
    let fixture = Fixture::new("symlink");
    fixture.write("Library/Caches/app/file", "x");
    fixture.write("elsewhere/precious", "keep");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);
    let store = fixture.store();

    // The item swapped for a symlink.
    fs::rename(&item, fixture.home().join("moved-away")).expect("move away");
    std::os::unix::fs::symlink(fixture.home().join("elsewhere"), &item).expect("link");
    assert_eq!(
        store.move_in(&item, expected, "b1", "e1"),
        Err(Failure::Changed)
    );
    assert!(fixture.home().join("elsewhere/precious").exists());

    // A symlinked folder on the way to the item.
    std::os::unix::fs::symlink(
        fixture.home().join("Library"),
        fixture.home().join("LinkedLibrary"),
    )
    .expect("link");
    let through_link = fixture.home().join("LinkedLibrary/Caches/app");
    assert_eq!(
        store.move_in(&through_link, expected, "b1", "e1"),
        Err(Failure::ParentUnavailable)
    );
    // Nothing reached the quarantine folder.
    assert!(!store.path().join("b1").exists());
}

#[test]
fn a_restore_never_overwrites_what_is_now_at_the_original_place() {
    let fixture = Fixture::new("conflict");
    fixture.write("Library/Caches/app/held", "held");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);
    let store = fixture.store();
    store.move_in(&item, expected, "b1", "e1").expect("moved");

    fixture.write("Library/Caches/app/newer", "newer");
    assert_eq!(Store::original_taken(&item), Some(true));
    assert_eq!(
        store.restore("b1", "e1", expected, &item),
        Err(Failure::Exists)
    );
    assert_eq!(
        fs::read_to_string(item.join("newer")).expect("untouched"),
        "newer"
    );
    assert!(!item.join("held").exists());
    assert!(store.holds("b1", "e1", expected));

    // A file in the way blocks it too.
    fs::remove_dir_all(&item).expect("clear");
    fs::write(&item, "a file").expect("file");
    assert_eq!(
        store.restore("b1", "e1", expected, &item),
        Err(Failure::Exists)
    );
    assert_eq!(fs::read_to_string(&item).expect("untouched"), "a file");
}

#[test]
fn a_restore_into_a_missing_folder_is_refused() {
    let fixture = Fixture::new("parent");
    fixture.write("Library/Logs/app/log.txt", "log");
    let item = fixture.home().join("Library/Logs/app");
    let expected = identity(&item);
    let store = fixture.store();
    store.move_in(&item, expected, "b1", "e1").expect("moved");
    fs::remove_dir_all(fixture.home().join("Library/Logs")).expect("remove parent");

    assert_eq!(Store::original_taken(&item), None);
    assert_eq!(
        store.restore("b1", "e1", expected, &item),
        Err(Failure::ParentUnavailable)
    );
    assert!(store.holds("b1", "e1", expected));
}

/// A walk that fails part-way leaves the item held and reported as failed, never deleted.
#[cfg(target_os = "macos")]
#[test]
fn a_permanent_delete_that_stops_part_way_keeps_the_item_held() {
    let fixture = Fixture::new("partial");
    let db = Database::in_memory();
    let store = fixture.store();
    let item = record(&fixture, &db, &store, "Library/Caches/locked", "a1");
    // A locked file inside it: its removal fails after other entries may be gone.
    let locked = store.item_path("b1", "a1").join("zz-locked");
    fs::write(&locked, "x").expect("file");
    let lock = |flag: &str| {
        let status = std::process::Command::new("/usr/bin/chflags")
            .arg(flag)
            .arg(&locked)
            .status()
            .expect("chflags");
        assert!(status.success());
    };
    lock("uchg");
    let stop = AtomicBool::new(false);
    let summary = delete_items(
        &db,
        &store,
        std::slice::from_ref(&item.id),
        &stop,
        || 5,
        |_| {},
    )
    .expect("ran");
    lock("nouchg");
    assert_eq!(summary.failed, std::slice::from_ref(&item.id));
    assert!(summary.deleted.is_empty());
    assert_eq!(
        db.item("a1").expect("read").map(|record| record.state),
        Some(ItemState::Held)
    );
    assert!(store.item_path("b1", "a1").exists());
}

#[test]
fn a_permanent_delete_removes_read_only_folders_and_never_follows_symlinks() {
    let fixture = Fixture::new("delete");
    fixture.write("go/pkg/mod/example@v1/deep/file.go", "package x");
    let outside = fixture.write("outside/keep.txt", "keep");
    let module = fixture.home().join("go/pkg/mod/example@v1");
    std::os::unix::fs::symlink(fixture.home().join("outside"), module.join("link-out"))
        .expect("link");
    // Go makes module folders read-only.
    fs::set_permissions(module.join("deep"), fs::Permissions::from_mode(0o555)).expect("ro");
    fs::set_permissions(&module, fs::Permissions::from_mode(0o555)).expect("ro");
    let item = fixture.home().join("go/pkg/mod");
    let expected = identity(&item);
    let store = fixture.store();
    store.move_in(&item, expected, "b1", "e1").expect("moved");

    store.delete("b1", "e1", expected).expect("deleted");
    assert!(!store.item_path("b1", "e1").exists());
    assert!(!store.path().join("b1").exists());
    // The symlink was removed as a link; what it pointed to is untouched.
    assert_eq!(fs::read_to_string(&outside).expect("outside"), "keep");
}

#[test]
fn a_held_item_replaced_inside_the_quarantine_folder_is_not_deleted_or_restored() {
    let fixture = Fixture::new("swapped");
    fixture.write("Library/Caches/app/file", "x");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);
    let store = fixture.store();
    store.move_in(&item, expected, "b1", "e1").expect("moved");
    let held = store.item_path("b1", "e1");
    replace_dir(&held, "other");

    assert!(!store.holds("b1", "e1", expected));
    assert_eq!(store.delete("b1", "e1", expected), Err(Failure::Changed));
    assert_eq!(
        store.restore("b1", "e1", expected, &item),
        Err(Failure::Changed)
    );
    assert!(held.join("other").exists());
}

#[test]
fn a_symlinked_data_folder_is_refused_and_nothing_is_written_through_it() {
    let fixture = Fixture::new("datalink");
    fixture.write("Library/Caches/app/file", "x");
    let target = fixture.root.join("target");
    fs::create_dir(&target).expect("target");
    let linked = fixture.root.join("linked-data");
    std::os::unix::fs::symlink(&target, &linked).expect("link");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);

    let store = Store::new(linked);
    assert_eq!(
        store.move_in(&item, expected, "b1", "e1"),
        Err(Failure::StoreUnavailable)
    );
    assert!(item.join("file").exists());
    assert_eq!(fs::read_dir(&target).expect("target").count(), 0);
}

#[test]
fn paths_split_only_when_absolute_and_plain() {
    assert!(split(Path::new("/Users/a/Library/Caches/x")).is_some());
    assert!(split(Path::new("relative/x")).is_none());
    assert!(split(Path::new("/Users/a/../b")).is_none());
    assert!(split(Path::new("/")).is_none());
    // IDs that could escape the batch folder are refused.
    let fixture = Fixture::new("ids");
    fixture.write("Library/Caches/app/file", "x");
    let item = fixture.home().join("Library/Caches/app");
    let expected = identity(&item);
    assert_eq!(
        fixture.store().move_in(&item, expected, "../b1", "e1"),
        Err(Failure::Other)
    );
    assert_eq!(
        fixture.store().move_in(&item, expected, "b1", "e/1"),
        Err(Failure::Other)
    );
    assert_eq!(
        fixture.store().move_in(&item, expected, "b1", "not-hex"),
        Err(Failure::Other)
    );
    assert!(item.exists());
}

fn record(fixture: &Fixture, db: &Database, store: &Store, relative: &str, id: &str) -> ItemRecord {
    let original = fixture.home().join(relative);
    record_at(db, store, &original, "cache", None, id)
}

/// Quarantines a new folder at `original` (anywhere), recorded with `category` and `root`.
fn record_at(
    db: &Database,
    store: &Store,
    original: &Path,
    category: &str,
    root: Option<&Path>,
    id: &str,
) -> ItemRecord {
    fs::create_dir_all(original).expect("folders");
    fs::write(original.join("file"), id).expect("file");
    let original = original.to_path_buf();
    let found = identity(&original);
    let record = ItemRecord {
        id: id.to_owned(),
        batch_id: "b1".to_owned(),
        name: id.to_owned(),
        category: category.to_owned(),
        original: original.clone(),
        root: root.map(Path::to_path_buf),
        bytes: 100,
        dev: found.dev,
        ino: found.ino,
        is_dir: true,
        state: ItemState::Moving,
        updated_ms: 1,
    };
    if db.batches().expect("batches").is_empty() {
        db.begin_batch(
            &BatchRecord {
                id: "b1".to_owned(),
                created_ms: 1,
                origin: "standard".to_owned(),
            },
            &[],
        )
        .expect("batch");
    }
    db.begin_batch_items_for_tests(std::slice::from_ref(&record));
    store.move_in(&original, found, "b1", id).expect("moved");
    db.set_state(id, ItemState::Held, 2).expect("held");
    record
}

#[test]
fn an_original_place_excluded_since_is_not_looked_at_when_listing_or_settling() {
    let fixture = Fixture::new("excluded-original");
    let db = Database::in_memory();
    let store = fixture.store();
    let held = record(&fixture, &db, &store, "Library/Caches/b", "b2");
    // Something is now at its original place: a probe would say "conflict".
    fixture.write("Library/Caches/b/newer", "newer");
    let home = fixture.home();
    let excluded = [held.original.clone()];
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &excluded,
    };
    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&scope)) else {
        panic!("available");
    };
    // Not looked at, so shown as restorable; a restore still never overwrites.
    assert_eq!(batches[0].items[0].status, ItemStatus::Restorable);

    // An interrupted restore whose item is back at the (now excluded) place isn't settled
    // from it.
    fs::remove_dir_all(&held.original).expect("clear");
    fs::rename(store.item_path("b1", "b2"), &held.original).expect("back");
    db.set_state("b2", ItemState::Restoring, 3)
        .expect("restoring");
    assert_eq!(reconcile(&db, &store, Some(&scope), 4), Ok(0));
    let unexcluded = Scope {
        excluded: &[],
        ..scope
    };
    assert_eq!(reconcile(&db, &store, Some(&unexcluded), 5), Ok(1));
}

#[test]
fn the_list_shows_each_items_restore_status_and_restores_record_outcomes() {
    let fixture = Fixture::new("service");
    let db = Database::in_memory();
    let store = fixture.store();
    let a = record(&fixture, &db, &store, "Library/Caches/a", "a1");
    let b = record(&fixture, &db, &store, "Library/Caches/b", "b2");
    let c = record(&fixture, &db, &store, "Library/Caches/c", "c3");
    fixture.write("Library/Caches/b/newer", "newer");
    fs::remove_dir_all(store.item_path("b1", "c3")).expect("gone outside Kleari");
    let home = fixture.home();
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };

    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&scope)) else {
        panic!("available");
    };
    let statuses: Vec<(String, ItemStatus, String)> = batches[0]
        .items
        .iter()
        .map(|item| (item.id.clone(), item.status, item.from.clone()))
        .collect();
    assert_eq!(
        statuses,
        [
            (
                "a1".to_owned(),
                ItemStatus::Restorable,
                "~/Library/Caches".to_owned()
            ),
            (
                "b2".to_owned(),
                ItemStatus::Conflict,
                "~/Library/Caches".to_owned()
            ),
            (
                "c3".to_owned(),
                ItemStatus::Missing,
                "~/Library/Caches".to_owned()
            ),
        ]
    );

    let results = restore_items(
        &db,
        &store,
        &scope,
        &[a.id.clone(), b.id.clone(), c.id.clone(), "nope".to_owned()],
        9,
    )
    .expect("restored");
    let outcomes: Vec<RestoreOutcome> = results.iter().map(|result| result.outcome).collect();
    assert_eq!(
        outcomes,
        [
            RestoreOutcome::Restored,
            RestoreOutcome::Conflict,
            RestoreOutcome::Missing,
            RestoreOutcome::Missing
        ]
    );
    assert!(a.original.join("file").exists());
    assert_eq!(
        db.item("a1").expect("a").map(|item| item.state),
        Some(ItemState::Restored)
    );
    assert_eq!(
        db.item("b2").expect("b").map(|item| item.state),
        Some(ItemState::Held)
    );
}

#[test]
fn a_permanent_delete_stops_before_the_next_item_when_asked() {
    let fixture = Fixture::new("stop");
    let db = Database::in_memory();
    let store = fixture.store();
    let ids: Vec<String> = ["a1", "b2", "c3"]
        .iter()
        .map(|id| record(&fixture, &db, &store, &format!("Library/Caches/{id}"), id).id)
        .collect();
    let stop = AtomicBool::new(false);
    let mut seen = Vec::new();
    let summary = delete_items(
        &db,
        &store,
        &ids,
        &stop,
        || 5,
        |progress| {
            if progress.state == service::DeleteState::Deleted {
                // Asked to stop while the first item finishes.
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            seen.push(progress);
        },
    )
    .expect("deleted");
    assert_eq!(summary.deleted, ["a1"]);
    assert_eq!(summary.kept, ["b2", "c3"]);
    assert!(summary.stopped);
    assert!(!store.item_path("b1", "a1").exists());
    assert!(store.item_path("b1", "b2").exists());
    assert_eq!(
        db.item("b2").expect("b").map(|item| item.state),
        Some(ItemState::Held)
    );
    assert_eq!(seen.len(), 2);
    // The activity log counts the two not reached as not deleted, item by item.
    let event = &db.events_since(0, 10).expect("events")[0];
    assert_eq!((event.items, event.failed), (3, 2));
    let outcomes: Vec<&str> = event
        .lines
        .iter()
        .map(|line| line.outcome.as_str())
        .collect();
    assert_eq!(outcomes, ["deleted", "kept", "kept"]);
}

#[test]
fn interrupted_moves_and_restores_are_resolved_only_from_what_the_folders_show() {
    let fixture = Fixture::new("resolve");
    let db = Database::in_memory();
    let store = fixture.store();
    let held_record = record(&fixture, &db, &store, "Library/Caches/held", "a1");
    let moving = ItemRecord {
        state: ItemState::Moving,
        ..held_record.clone()
    };
    assert_eq!(
        store.resolve_interrupted(&moving, true),
        Some(ItemState::Held)
    );
    // What the quarantine folder proves doesn't depend on the original's place.
    assert_eq!(
        store.resolve_interrupted(&moving, false),
        Some(ItemState::Held)
    );

    // Never moved: nothing in the quarantine folder under its name.
    let never = ItemRecord {
        id: "b9".to_owned(),
        ..moving.clone()
    };
    assert_eq!(
        store.presence(
            "b1",
            "b9",
            Identity {
                dev: never.dev,
                ino: never.ino,
                is_dir: true
            }
        ),
        Presence::Absent
    );
    assert_eq!(
        store.resolve_interrupted(&never, false),
        Some(ItemState::NotMoved)
    );

    // A restore that finished before the record was updated: the item is back.
    let identity = Identity {
        dev: held_record.dev,
        ino: held_record.ino,
        is_dir: true,
    };
    store
        .restore("b1", "a1", identity, &held_record.original)
        .expect("restored");
    let restoring = ItemRecord {
        state: ItemState::Restoring,
        ..held_record.clone()
    };
    assert_eq!(
        store.resolve_interrupted(&restoring, true),
        Some(ItemState::Restored)
    );
    // The same proof at a place outside scope isn't used.
    assert_eq!(store.resolve_interrupted(&restoring, false), None);
    // Gone from quarantine but not at its original place either: no conclusion.
    fs::remove_dir_all(&held_record.original).expect("gone");
    assert_eq!(store.resolve_interrupted(&restoring, true), None);

    // The quarantine folder can't be inspected safely (its data folder is a symlink): a
    // `moving` record is left as it is, never concluded `notMoved`.
    let linked = fixture.root.join("linked-data");
    std::os::unix::fs::symlink(fixture.root.join("data"), &linked).expect("link");
    let unreadable = Store::new(linked);
    assert_eq!(unreadable.presence("b1", "a1", identity), Presence::Unknown);
    assert_eq!(unreadable.resolve_interrupted(&moving, true), None);
}

#[test]
fn the_delete_walk_itself_refuses_an_entry_other_than_the_recorded_one() {
    let fixture = Fixture::new("walkid");
    fixture.write("Library/Caches/one/file", "one");
    fixture.write("Library/Caches/two/file", "two");
    let one = identity(&fixture.home().join("Library/Caches/one"));
    let (dir, _) = DirHandle::open_root(&fixture.home().join("Library/Caches")).expect("open");
    // `two` is under the name it is asked to delete, but `one` is what was confirmed.
    let error = fsops::remove_tree(&dir, c"two", one).expect_err("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(fixture.home().join("Library/Caches/two/file").exists());
    fsops::remove_tree(&dir, c"one", one).expect("deleted");
    assert!(!fixture.home().join("Library/Caches/one").exists());
}

#[test]
fn unresolved_and_unverifiable_items_are_shown_as_such_not_as_restorable_or_missing() {
    let fixture = Fixture::new("statuses");
    let db = Database::in_memory();
    let store = fixture.store();
    record(&fixture, &db, &store, "Library/Caches/pending", "a1");
    record(&fixture, &db, &store, "Library/Caches/impostor", "b2");
    // A move whose outcome wasn't recorded yet.
    db.set_state("a1", ItemState::Moving, 3).expect("moving");
    // Something else now under b2's name in the quarantine folder.
    replace_dir(&store.item_path("b1", "b2"), "impostor");
    let home = fixture.home();
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };

    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&scope)) else {
        panic!("available");
    };
    let statuses: Vec<(String, ItemStatus)> = batches[0]
        .items
        .iter()
        .map(|item| (item.id.clone(), item.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("a1".to_owned(), ItemStatus::Pending),
            ("b2".to_owned(), ItemStatus::Unverified),
        ]
    );
}

#[test]
fn a_record_whose_original_is_outside_its_categorys_places_is_never_restored() {
    let fixture = Fixture::new("tampered");
    let db = Database::in_memory();
    let store = fixture.store();
    // Held legitimately from ~/Library/Caches, then the history is edited so the original
    // points into iCloud Drive: a restore would write there.
    let held = record(&fixture, &db, &store, "Library/Caches/a", "a1");
    let elsewhere = fixture.home().join("Library/Mobile Documents/a");
    fs::create_dir_all(elsewhere.parent().expect("parent")).expect("icloud");
    db.set_original_for_tests("a1", &elsewhere);
    let home = fixture.home();
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };

    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&scope)) else {
        panic!("available");
    };
    assert_eq!(batches[0].items[0].status, ItemStatus::Unverified);
    // Without a home folder nothing can be tied to a scope.
    let QuarantineList::Available { batches, .. } = list(&db, &store, None) else {
        panic!("available");
    };
    assert_eq!(batches[0].items[0].status, ItemStatus::Unverified);

    let results =
        restore_items(&db, &store, &scope, std::slice::from_ref(&held.id), 9).expect("restore");
    assert_eq!(results[0].outcome, RestoreOutcome::NotAllowed);
    assert!(!elsewhere.exists());
    assert!(store.item_path("b1", "a1").join("file").exists());
    assert_eq!(
        db.item("a1").expect("a").map(|item| item.state),
        Some(ItemState::Held)
    );
}

#[test]
fn launch_reconciliation_never_concludes_from_an_original_outside_scope() {
    let fixture = Fixture::new("reconcile-scope");
    let db = Database::in_memory();
    let store = fixture.store();
    let held = record(&fixture, &db, &store, "Library/Caches/a", "a1");
    // An interrupted restore whose record was edited to point into iCloud Drive, where the
    // item (same identity) now is.
    let elsewhere = fixture.home().join("Library/Mobile Documents/a");
    fs::create_dir_all(elsewhere.parent().expect("parent")).expect("icloud");
    fs::rename(store.item_path("b1", "a1"), &elsewhere).expect("moved out");
    db.set_original_for_tests("a1", &elsewhere);
    db.set_state("a1", ItemState::Restoring, 3)
        .expect("restoring");
    let home = fixture.home();
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };
    let state = || db.item("a1").expect("a").map(|item| item.state);

    assert_eq!(reconcile(&db, &store, Some(&scope), 4), Ok(0));
    assert_eq!(reconcile(&db, &store, None, 4), Ok(0));
    assert_eq!(state(), Some(ItemState::Restoring));

    // The same proof at a place its category comes from settles it.
    fs::rename(&elsewhere, &held.original).expect("back");
    db.set_original_for_tests("a1", &held.original);
    assert_eq!(reconcile(&db, &store, Some(&scope), 5), Ok(1));
    assert_eq!(state(), Some(ItemState::Restored));
}

#[test]
fn a_recorded_added_folder_only_names_what_to_choose_it_never_allows_a_restore() {
    let fixture = Fixture::new("needs-folder");
    let db = Database::in_memory();
    let store = fixture.store();
    let home = fixture.home();
    // Approved under an added folder outside the home folder.
    let work = fixture.root.join("work");
    let modules = work.join("app/node_modules");
    let item = record_at(&db, &store, &modules, "node", Some(&work), "a1");
    // An edited record whose root is widened to a top-level folder of the disk (`/private`
    // on macOS, `/tmp` on Linux).
    let shared = fixture.root.join("shared/recovery/node_modules");
    let top: PathBuf = fixture.root.components().take(2).collect();
    let widened = record_at(&db, &store, &shared, "node", Some(&top), "b2");
    // An edited record whose root is the disk itself.
    let anywhere = fixture.root.join("anywhere/node_modules");
    let rooted = record_at(&db, &store, &anywhere, "node", Some(Path::new("/")), "c3");
    let after_restart = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };

    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&after_restart)) else {
        panic!("available");
    };
    let shown: Vec<(ItemStatus, bool, Option<String>)> = batches[0]
        .items
        .iter()
        .map(|item| (item.status, item.choose_folder, item.needed_folder.clone()))
        .collect();
    let display = |path: &Path| crate::system_monitor::display_path(path, Some(&home));
    assert_eq!(
        shown,
        [
            (ItemStatus::NeedsFolder, true, Some(display(&work))),
            // Only the user's choice in the panel allows it, and a folder this broad isn't
            // shown as where it came from.
            (ItemStatus::NeedsFolder, true, None),
            (ItemStatus::Unverified, false, None),
        ]
    );
    let ids = [item.id.clone(), widened.id.clone(), rooted.id.clone()];
    let results = restore_items(&db, &store, &after_restart, &ids, 9).expect("restore");
    let outcomes: Vec<RestoreOutcome> = results.iter().map(|result| result.outcome).collect();
    assert_eq!(
        outcomes,
        [
            RestoreOutcome::NeedsFolder,
            RestoreOutcome::NeedsFolder,
            RestoreOutcome::NotAllowed
        ]
    );
    assert!(!modules.exists() && !shared.exists() && !anywhere.exists());

    // The user chooses the folder the item came from: that item, and only it, goes back.
    let chosen = [work.clone()];
    let scope = Scope {
        home: &home,
        temp: None,
        added: &chosen,
        excluded: &[],
    };
    let results = restore_items(&db, &store, &scope, &ids, 10).expect("restore");
    let outcomes: Vec<RestoreOutcome> = results.iter().map(|result| result.outcome).collect();
    assert_eq!(
        outcomes,
        [
            RestoreOutcome::Restored,
            RestoreOutcome::NeedsFolder,
            RestoreOutcome::NotAllowed
        ]
    );
    assert!(modules.join("file").exists());
    assert!(!shared.exists() && !anywhere.exists());
}

#[test]
fn an_interrupted_move_outside_scope_is_settled_from_the_quarantine_folder() {
    let fixture = Fixture::new("legacy-moving");
    let db = Database::in_memory();
    let store = fixture.store();
    let home = fixture.home();
    // A version 1 record (no root) of a build folder outside the home folder, left `moving`.
    let modules = fixture.root.join("work/app/node_modules");
    record_at(&db, &store, &modules, "node", None, "a1");
    db.set_state("a1", ItemState::Moving, 3).expect("moving");
    let scope = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };
    assert_eq!(reconcile(&db, &store, Some(&scope), 4), Ok(1));
    assert_eq!(
        db.item("a1").expect("a").map(|item| item.state),
        Some(ItemState::Held)
    );
}

#[test]
fn an_interrupted_restore_from_an_added_folder_settles_once_that_folder_is_chosen() {
    let fixture = Fixture::new("restoring-added");
    let db = Database::in_memory();
    let store = fixture.store();
    let home = fixture.home();
    let work = fixture.root.join("work");
    let modules = work.join("app/node_modules");
    record_at(&db, &store, &modules, "node", Some(&work), "a1");
    // The restore finished but the app stopped before recording it.
    fs::rename(store.item_path("b1", "a1"), &modules).expect("back");
    db.set_state("a1", ItemState::Restoring, 3)
        .expect("restoring");
    let at_launch = Scope {
        home: &home,
        temp: None,
        added: &[],
        excluded: &[],
    };
    assert_eq!(reconcile(&db, &store, Some(&at_launch), 4), Ok(0));
    let QuarantineList::Available { batches, .. } = list(&db, &store, Some(&at_launch)) else {
        panic!("available");
    };
    let shown = &batches[0].items[0];
    assert_eq!(
        (shown.status, shown.choose_folder),
        (ItemStatus::Pending, true)
    );

    let chosen = [work.clone()];
    let scope = Scope {
        home: &home,
        temp: None,
        added: &chosen,
        excluded: &[],
    };
    assert_eq!(reconcile(&db, &store, Some(&scope), 5), Ok(1));
    assert_eq!(
        db.item("a1").expect("a").map(|item| item.state),
        Some(ItemState::Restored)
    );
}

fn stat_of(path: &Path) -> crate::storage::fsdir::FileStat {
    let (dir, _) = DirHandle::open_root(path.parent().expect("parent")).expect("open parent");
    let name =
        std::ffi::CString::new(path.file_name().expect("name").as_encoded_bytes()).expect("c");
    dir.stat_child(&name).expect("stat")
}

#[test]
fn a_rollback_settles_the_record_only_when_the_recorded_item_is_proven_in_place() {
    let fixture = Fixture::new("rollback");
    let approved = fixture.write("approved/file", "a");
    let swapped = fixture.write("swapped/file", "s");
    let approved_dir = approved.parent().expect("dir");
    let swapped_dir = swapped.parent().expect("dir");
    let expected = identity(approved_dir);

    // Move rollback: "not moved" needs the approved item back at its place and nothing left
    // in quarantine under the item's name.
    assert_eq!(
        settle_move_rollback(expected, Some(&stat_of(approved_dir)), true),
        Failure::Changed
    );
    // The swapped entry went back, but the approved item isn't proven anywhere: unresolved.
    assert_eq!(
        settle_move_rollback(expected, Some(&stat_of(swapped_dir)), true),
        Failure::Unconfirmed
    );
    assert_eq!(
        settle_move_rollback(expected, None, true),
        Failure::Unconfirmed
    );
    // Something still in quarantine under the item's name: unresolved.
    assert_eq!(
        settle_move_rollback(expected, Some(&stat_of(approved_dir)), false),
        Failure::Unconfirmed
    );

    // Restore rollback: back to held only if quarantine holds the recorded item itself.
    assert_eq!(
        settle_restore_rollback(expected, Some(&stat_of(approved_dir))),
        Failure::Changed
    );
    assert_eq!(
        settle_restore_rollback(expected, Some(&stat_of(swapped_dir))),
        Failure::Unconfirmed
    );
    assert_eq!(
        settle_restore_rollback(expected, None),
        Failure::Unconfirmed
    );
}

#[test]
fn show_in_finder_never_builds_a_path_from_a_malformed_stored_id() {
    let fixture = Fixture::new("reveal");
    let store = fixture.store();
    assert_eq!(
        store.reveal_path("b1", "a1"),
        Some(store.path().join("b1").join("a1"))
    );
    for (batch, item) in [
        ("../../somewhere", "a1"),
        ("b1", "../x"),
        ("b1/c2", "a1"),
        ("", "a1"),
        ("b1", "/etc"),
    ] {
        assert_eq!(store.reveal_path(batch, item), None, "{batch} {item}");
    }
}

#[test]
fn only_one_file_changing_operation_runs_at_a_time() {
    let state = QuarantineState::default();
    let guard = state.begin().expect("first");
    assert!(state.begin().is_err());
    // Settling never runs alongside an operation; it is skipped, not waited for.
    assert!(state.begin_settle().is_err());
    drop(guard);
    assert!(state.begin().is_ok());
}

#[test]
fn a_recorded_folder_is_named_only_when_its_path_looks_like_a_project_folder() {
    use super::service::broad;
    let home = Some(Path::new("/Users/someone"));
    // Decided from the path alone, so these needn't exist (and nothing is read).
    assert!(!broad(Path::new("/opt/work"), home));
    assert!(!broad(Path::new("/Volumes/Disk/Projects"), home));
    assert!(!broad(Path::new("/Users/someone/Work"), home));
    assert!(broad(Path::new("/Volumes/Disk"), home));
    assert!(broad(Path::new("/Volumes"), home));
    assert!(broad(Path::new("/Users"), home));
    assert!(broad(Path::new("/private"), home));
    assert!(broad(Path::new("/Users/someone"), home));
}

#[test]
fn an_operation_waits_behind_a_settle_instead_of_failing() {
    let state = std::sync::Arc::new(QuarantineState::default());
    let settle = state.begin_settle().expect("settle");
    let (started, waiting) = std::sync::mpsc::channel();
    let operation = {
        let state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || {
            started.send(()).expect("started");
            state.begin().map(|_| ()).is_ok()
        })
    };
    waiting.recv().expect("thread started");
    // Still behind the settle a moment later, not refused.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!operation.is_finished());
    drop(settle);
    assert!(operation.join().expect("joined"));
}
