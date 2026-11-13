//! Storage-discovery safety tests, against disposable temporary trees only: a scan never
//! follows links, never accepts a replaced or linked root, and never changes anything.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64 as Counter;

use super::scanner::{scan_with, ScanLimits};
use super::*;

static NEXT_DIR: Counter = Counter::new(0);

/// A temporary directory removed on drop, including any read-only subfolders.
struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kleari-scan-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp tree");
        Self {
            path: fs::canonicalize(&path).expect("canonical temp tree"),
        }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(&path).expect("create dir");
        path
    }

    fn file(&self, relative: &str, bytes: usize) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, vec![7u8; bytes]).expect("write file");
        path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        restore_permissions(&self.path);
        // Best-effort cleanup of a disposable test directory; a failure only leaves temp files.
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn restore_permissions(path: &Path) {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.is_dir() {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    restore_permissions(&entry.path());
                }
            }
        }
    }
}

fn run(tree: &TempTree) -> ScanOutcome {
    let identity = root_identity(&tree.path).expect("root is a directory");
    scan_with(
        &tree.path,
        identity,
        &AtomicBool::new(false),
        |_| {},
        ScanLimits::default(),
    )
    .expect("scan succeeds")
}

#[test]
fn symlinks_are_never_followed_including_loops_and_escapes() {
    let tree = TempTree::new();
    let outside = TempTree::new();
    outside.file("secret/large.bin", 2_000_000);
    tree.file("inside/a.txt", 1_000);
    std::os::unix::fs::symlink(&outside.path, tree.path.join("inside/escape")).expect("symlink");
    std::os::unix::fs::symlink(&tree.path, tree.path.join("inside/loop")).expect("symlink");
    std::os::unix::fs::symlink(
        outside.path.join("secret/large.bin"),
        tree.path.join("link.bin"),
    )
    .expect("symlink");

    let outcome = run(&tree);
    assert_eq!(outcome.coverage.symlinks_skipped, 3);
    assert_eq!(outcome.totals.files, 1);
    assert!(outcome
        .index
        .iter()
        .all(|file| !file.relative_path.contains("large.bin")));
}

#[test]
fn a_replaced_root_fails_closed() {
    let tree = TempTree::new();
    let root = tree.dir("chosen");
    let identity = root_identity(&root).expect("root");
    // Move the original aside (it keeps its inode), so the replacement must get a new one.
    fs::rename(&root, tree.path.join("original")).expect("move aside");
    fs::create_dir(&root).expect("replace");
    tree.file("chosen/new.txt", 10);
    assert_ne!(root_identity(&root), Some(identity));
    assert_eq!(
        scan(&root, identity, &AtomicBool::new(false), |_| {}),
        Err(ScanError::RootChanged)
    );

    fs::remove_dir_all(&root).expect("remove");
    assert_eq!(
        scan(&root, identity, &AtomicBool::new(false), |_| {}),
        Err(ScanError::RootChanged)
    );

    // A symlink put in place of the root is refused, even pointing at the original folder.
    std::os::unix::fs::symlink(tree.path.join("original"), &root).expect("symlink");
    assert_eq!(
        scan(&root, identity, &AtomicBool::new(false), |_| {}),
        Err(ScanError::RootChanged)
    );
}

#[test]
fn a_symlink_is_not_accepted_as_a_root() {
    let tree = TempTree::new();
    let target = tree.dir("target");
    let link = tree.path.join("link");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    assert!(root_identity(&link).is_none());
    assert!(root_identity(&tree.file("plain.txt", 10)).is_none());
}

/// Everything a write, rename, delete or permission change would alter (not access time).
#[derive(Debug, PartialEq, Eq)]
struct EntryState {
    path: PathBuf,
    inode: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
}

fn snapshot(path: &Path, out: &mut Vec<EntryState>) {
    let metadata = fs::symlink_metadata(path).expect("metadata");
    out.push(EntryState {
        path: path.to_path_buf(),
        inode: metadata.ino(),
        len: metadata.len(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        mode: metadata.mode(),
    });
    if metadata.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(path)
            .expect("list")
            .map(|entry| entry.expect("entry").path())
            .collect();
        entries.sort();
        for entry in entries {
            snapshot(&entry, out);
        }
    }
}

#[test]
fn scanning_changes_nothing_on_disk() {
    let tree = TempTree::new();
    tree.file("a/one.bin", 50_000);
    tree.file("a/b/two.txt", 1_000);
    tree.file(".hidden/three.json", 2_000);
    std::os::unix::fs::symlink("a/one.bin", tree.path.join("link")).expect("symlink");

    let mut before = Vec::new();
    snapshot(&tree.path, &mut before);
    run(&tree);
    let mut after = Vec::new();
    snapshot(&tree.path, &mut after);
    // Access times are excluded: listing a directory may update them; nothing is written.
    assert_eq!(before, after);
}

#[test]
fn state_rejects_unknown_grants_concurrent_scans_and_queries_without_results() {
    let tree = TempTree::new();
    tree.file("a/x.txt", 10);
    let state = StorageState::default();
    assert_eq!(
        state.begin_scan("g99").map(|_| ()),
        Err(StateError::GrantNotFound)
    );

    let (grant_id, display_name) = state.add_grant(&tree.path).expect("grant");
    assert_eq!(grant_id, "g1");
    assert!(tree.path.ends_with(&display_name));
    assert!(matches!(
        state.last_scan(&grant_id),
        Err(StateError::NoScanResult)
    ));

    let (grant, cancel) = state.begin_scan(&grant_id).expect("begin");
    assert_eq!(
        state.begin_scan(&grant_id).map(|_| ()),
        Err(StateError::ScanInProgress)
    );
    assert!(state.cancel(&grant_id));
    assert!(!state.cancel("g99"));
    let outcome = scan(&grant.root, grant.identity, &cancel, |_| {}).expect("scan");
    assert!(outcome.cancelled);
    state.end_scan(&grant_id, Some(outcome));
    assert!(state.last_scan(&grant_id).is_ok());
    assert!(matches!(
        state.last_scan("g2"),
        Err(StateError::NoScanResult)
    ));
    assert!(!state.cancel(&grant_id), "no scan is running any more");
}

#[test]
fn a_file_is_not_accepted_as_a_grant() {
    let tree = TempTree::new();
    let state = StorageState::default();
    assert!(state.add_grant(&tree.file("plain.txt", 10)).is_none());
    assert!(state.add_grant(&tree.path.join("missing")).is_none());
}
