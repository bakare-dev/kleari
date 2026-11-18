//! The Trash, inside Kleari: what is in it (your own and each external disk's), deleting
//! chosen items for good, and Put Back.
//!
//! Listing reads names, sizes and the time each item was moved in, by handle, without
//! following links, and where Finder says each came from (`dsstore`). macOS protects the Trash,
//! so this needs Full Disk Access; without it the page says so. Deleting and Put Back act only
//! on an item still the one listed (same device and inode), through handles: deleting with
//! `Store::delete_in_place`, putting back with `Store::move_back`, which never overwrites.
//! Each is recorded in Activity by count and size only. Emptying the whole Trash is Finder's
//! (Maintenance's Empty Trash task).

pub mod dsstore;

use std::ffi::CString;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use crate::developer::{measure_dir, open_absolute, Links};
use crate::quarantine::{Failure, Identity, Store};

/// More entries than this in one Trash folder are left out of the list (still emptied by Empty
/// Trash).
const MAX_ITEMS: usize = 5_000;
/// The `.DS_Store` read for Put Back; a real one is far smaller.
const MAX_DS_STORE: u64 = 8 * 1024 * 1024;

/// One item as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    /// Something inside couldn't be read: a minimum.
    pub lower_bound: bool,
    pub folder: bool,
    /// A file's kind, from its name (for filtering); `None` for a folder.
    pub file_category: Option<crate::storage::category::FileCategory>,
    /// When it was moved to the Trash (ms since 1970), if known.
    pub trashed_ms: Option<u64>,
    /// Where it came from, shown as `~/…` (or the disk's path), if Finder recorded it.
    pub from: Option<String>,
    /// The external disk whose Trash it is in; `None` for your Mac's.
    pub disk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TrashList {
    Available {
        items: Vec<TrashItem>,
        bytes: u64,
        lower_bound: bool,
        /// More items than are listed (`MAX_ITEMS` per folder).
        more: u64,
    },
    /// macOS didn't let Kleari read the Trash: Full Disk Access is needed.
    NotAllowed,
}

/// What a listed item is, kept to act on it later.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    path: PathBuf,
    identity: Identity,
    bytes: u64,
    /// Where Put Back puts it, if Finder recorded where it came from and that place is allowed.
    origin: Option<PathBuf>,
}

/// The last listing, so the page acts by ID and never sends a path.
#[derive(Default)]
pub struct TrashState {
    entries: Mutex<Vec<Entry>>,
}

impl TrashState {
    fn set(&self, entries: Vec<Entry>) {
        *self.entries.lock().unwrap_or_else(PoisonError::into_inner) = entries;
    }

    fn take(&self, ids: &[String]) -> Vec<Entry> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|entry| ids.contains(&entry.id))
            .cloned()
            .collect()
    }

    /// The path of a listed item, for its picture.
    pub fn path(&self, id: &str) -> Option<PathBuf> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.path.clone())
    }

    fn forget(&self, ids: &[String]) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|entry| !ids.contains(&entry.id));
    }
}

/// A Trash folder and the root of the disk it belongs to (where Finder's origins start).
struct Folder {
    trash: PathBuf,
    disk_root: PathBuf,
    disk: Option<String>,
}

fn folders(home: &Path, uid: u32, volumes: &[PathBuf]) -> Vec<Folder> {
    let mut folders = vec![Folder {
        trash: home.join(".Trash"),
        disk_root: PathBuf::from("/"),
        disk: None,
    }];
    for volume in volumes {
        folders.push(Folder {
            trash: volume.join(".Trashes").join(uid.to_string()),
            disk_root: volume.clone(),
            disk: volume
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
        });
    }
    folders
}

/// Where Finder's record says an item came from, if it is a plain absolute place on the same
/// disk: on your Mac's disk only inside your home folder, on an external disk anywhere on it,
/// and never inside a Trash folder.
fn origin_path(disk_root: &Path, home: &Path, folder: &str, name: &str) -> Option<PathBuf> {
    let relative = Path::new(folder.trim_start_matches('/'));
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return None;
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let path = disk_root.join(relative).join(name);
    let inside_trash = path.components().any(|component| {
        matches!(component, Component::Normal(part) if part == ".Trash" || part == ".Trashes")
    });
    let allowed = if disk_root == Path::new("/") {
        path.starts_with(home) && path != home
    } else {
        path.starts_with(disk_root)
    };
    (allowed && !inside_trash).then_some(path)
}

/// Shown as `~/…` inside the home folder, otherwise as it is.
fn display(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.display().to_string(),
        |rest| format!("~/{}", rest.display()),
    )
}

/// Reads one Trash folder: its items (up to `MAX_ITEMS`), how many more there are, and
/// whether a size is a minimum.
fn read_folder(
    folder: &Folder,
    home: &Path,
    now_ms: u64,
    cancel: &AtomicBool,
    out: &mut Vec<(TrashItem, Entry)>,
) -> u64 {
    let Some((mut dir, _)) = open_absolute(&folder.trash) else {
        return 0;
    };
    let mut names = Vec::new();
    while let Some(Ok(name)) = dir.next_name() {
        names.push(name);
    }
    let origins = CString::new(".DS_Store")
        .ok()
        .and_then(|name| dir.open_file_child(&name).ok())
        .map(|file| {
            let mut bytes = Vec::new();
            // Unreadable: no origins, so Put Back isn't offered.
            let _ = file.take(MAX_DS_STORE).read_to_end(&mut bytes);
            dsstore::origins(&bytes)
        })
        .unwrap_or_default();
    let mut links = Links::default();
    let mut more = 0;
    for name in names {
        let Ok(text) = name.to_str() else {
            continue;
        };
        if text == ".DS_Store" || text == ".localized" {
            continue;
        }
        if out.len() >= MAX_ITEMS {
            more += 1;
            continue;
        }
        let Ok(stat) = dir.stat_child(&name) else {
            continue;
        };
        let measured = if stat.is_dir() {
            match dir.open_child(&name) {
                Ok((child, opened)) if opened.dev == stat.dev && opened.ino == stat.ino => {
                    let measured = measure_dir(child, stat.dev, &mut links, cancel);
                    (measured.bytes, measured.lower_bound)
                }
                _ => (stat.blocks.saturating_mul(512), true),
            }
        } else {
            (stat.blocks.saturating_mul(512), false)
        };
        let origin = origins.get(text).and_then(|origin| {
            let original = origin.name.as_deref().unwrap_or(text);
            origin_path(&folder.disk_root, home, origin.folder.as_deref()?, original)
        });
        let id = crate::ids::new_id(now_ms);
        let trashed_ms = u64::try_from(stat.changed_s)
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000));
        out.push((
            TrashItem {
                id: id.clone(),
                name: text.to_owned(),
                bytes: measured.0,
                lower_bound: measured.1,
                folder: stat.is_dir(),
                file_category: (!stat.is_dir())
                    .then(|| crate::storage::category::category_of(Path::new(text))),
                trashed_ms,
                from: origin
                    .as_deref()
                    .and_then(Path::parent)
                    .map(|parent| display(parent, home)),
                disk: folder.disk.clone(),
            },
            Entry {
                id,
                path: folder.trash.join(text),
                identity: Identity::of(&stat),
                bytes: measured.0,
                origin,
            },
        ));
    }
    more
}

/// Lists the Trash (yours and each external disk's), largest first, and keeps the listing.
pub fn list(
    state: &TrashState,
    home: &Path,
    uid: u32,
    volumes: &[PathBuf],
    now_ms: u64,
    cancel: &AtomicBool,
) -> TrashList {
    // The Trash is protected: without Full Disk Access, reading it is refused outright.
    if std::fs::read_dir(home.join(".Trash"))
        .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    {
        state.set(Vec::new());
        return TrashList::NotAllowed;
    }
    let mut found = Vec::new();
    let mut more = 0;
    for folder in folders(home, uid, volumes) {
        more += read_folder(&folder, home, now_ms, cancel, &mut found);
    }
    found.sort_by(|(a, _), (b, _)| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    let bytes = found
        .iter()
        .fold(0_u64, |total, (item, _)| total.saturating_add(item.bytes));
    let lower_bound = found.iter().any(|(item, _)| item.lower_bound);
    let (items, entries): (Vec<_>, Vec<_>) = found.into_iter().unzip();
    state.set(entries);
    TrashList::Available {
        items,
        bytes,
        lower_bound,
        more,
    }
}

/// Why an item couldn't be deleted or put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrashFailure {
    /// It isn't the item that was listed any more (or is gone).
    Changed,
    /// Finder didn't record where it came from, or that place isn't allowed.
    NoOrigin,
    /// Something is already where it came from; nothing was overwritten.
    Exists,
    /// The folder it came from is gone, or is on another disk.
    FolderGone,
    NeedsAccess,
    Failed,
}

fn trash_failure(failure: Failure) -> TrashFailure {
    match failure {
        Failure::Changed | Failure::Missing => TrashFailure::Changed,
        Failure::Exists => TrashFailure::Exists,
        Failure::ParentUnavailable | Failure::OtherVolume => TrashFailure::FolderGone,
        Failure::NeedsAccess => TrashFailure::NeedsAccess,
        Failure::StoreUnavailable | Failure::Unconfirmed | Failure::Other => TrashFailure::Failed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashOutcome {
    /// The IDs deleted or put back.
    pub done: Vec<String>,
    pub bytes: u64,
    pub failed: Vec<(String, TrashFailure)>,
}

fn act(
    state: &TrashState,
    ids: &[String],
    mut each: impl FnMut(&Entry) -> Result<(), TrashFailure>,
) -> TrashOutcome {
    let mut outcome = TrashOutcome {
        done: Vec::new(),
        bytes: 0,
        failed: Vec::new(),
    };
    let entries = state.take(ids);
    for id in ids {
        match entries.iter().find(|entry| &entry.id == id) {
            None => outcome.failed.push((id.clone(), TrashFailure::Changed)),
            Some(entry) => match each(entry) {
                Ok(()) => {
                    outcome.done.push(id.clone());
                    outcome.bytes = outcome.bytes.saturating_add(entry.bytes);
                }
                Err(failure) => outcome.failed.push((id.clone(), failure)),
            },
        }
    }
    state.forget(&outcome.done);
    outcome
}

/// Deletes the chosen items from the Trash for good.
pub fn delete(state: &TrashState, ids: &[String]) -> TrashOutcome {
    act(state, ids, |entry| {
        Store::delete_in_place(&entry.path, entry.identity).map_err(trash_failure)
    })
}

/// Puts the chosen items back where Finder says they came from, never over something there.
pub fn put_back(state: &TrashState, ids: &[String]) -> TrashOutcome {
    act(state, ids, |entry| {
        let origin = entry.origin.as_deref().ok_or(TrashFailure::NoOrigin)?;
        Store::move_back(&entry.path, entry.identity, origin).map_err(trash_failure)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_back_goes_only_to_plain_places_in_your_home_or_on_the_same_disk() {
        let home = Path::new("/Users/me");
        let root = Path::new("/");
        assert_eq!(
            origin_path(root, home, "Users/me/Documents/", "Report.pdf"),
            Some(PathBuf::from("/Users/me/Documents/Report.pdf"))
        );
        // Outside your home folder on your Mac's disk, back into a Trash, or with tricks in
        // the path: not offered.
        assert_eq!(origin_path(root, home, "Applications/", "App.app"), None);
        assert_eq!(origin_path(root, home, "Users/other/", "x"), None);
        assert_eq!(origin_path(root, home, "Users/me/.Trash/", "x"), None);
        assert_eq!(origin_path(root, home, "Users/me/../other/", "x"), None);
        assert_eq!(origin_path(root, home, "Users/me/", "a/b"), None);
        assert_eq!(origin_path(root, home, "Users/me/", ".."), None);
        // An external disk: anywhere on it, except its Trash.
        let disk = Path::new("/Volumes/Backup");
        assert_eq!(
            origin_path(disk, home, "Projects/", "old"),
            Some(PathBuf::from("/Volumes/Backup/Projects/old"))
        );
        assert_eq!(origin_path(disk, home, ".Trashes/501/", "x"), None);
    }

    #[test]
    fn origins_are_shown_from_the_home_folder() {
        let home = Path::new("/Users/me");
        assert_eq!(
            display(Path::new("/Users/me/Documents"), home),
            "~/Documents"
        );
        assert_eq!(
            display(Path::new("/Volumes/Backup/x"), home),
            "/Volumes/Backup/x"
        );
    }

    /// A fresh folder for one test, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("kleari-trash-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("Trash")).expect("trash");
            std::fs::create_dir_all(dir.join("Documents")).expect("documents");
            // Canonical, as `open_absolute` walks real folders (/var is a link on macOS).
            Self(std::fs::canonicalize(&dir).expect("canonical"))
        }

        fn trashed(&self, name: &str, origin: Option<&str>) -> Entry {
            let path = self.0.join("Trash").join(name);
            std::fs::write(&path, b"contents").expect("write");
            let meta = std::fs::symlink_metadata(&path).expect("stat");
            use std::os::unix::fs::MetadataExt;
            Entry {
                id: name.to_owned(),
                path,
                identity: Identity {
                    dev: meta.dev(),
                    ino: meta.ino(),
                    is_dir: false,
                },
                bytes: 8,
                origin: origin.map(|name| self.0.join("Documents").join(name)),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn put_back_returns_an_item_and_never_overwrites() {
        let scratch = Scratch::new("put-back");
        let state = TrashState::default();
        let report = scratch.trashed("Report 2.pdf", Some("Report.pdf"));
        let clash = scratch.trashed("Notes.txt", Some("Notes.txt"));
        let unknown = scratch.trashed("Loose.txt", None);
        std::fs::write(scratch.0.join("Documents/Notes.txt"), b"newer").expect("there already");
        state.set(vec![report.clone(), clash.clone(), unknown.clone()]);

        let outcome = put_back(
            &state,
            &[
                "Report 2.pdf".to_owned(),
                "Notes.txt".to_owned(),
                "Loose.txt".to_owned(),
            ],
        );
        assert_eq!(outcome.done, ["Report 2.pdf"]);
        assert!(scratch.0.join("Documents/Report.pdf").exists());
        assert!(!report.path.exists());
        assert_eq!(
            outcome.failed,
            [
                ("Notes.txt".to_owned(), TrashFailure::Exists),
                ("Loose.txt".to_owned(), TrashFailure::NoOrigin)
            ]
        );
        // The file already there is untouched, and the trashed one stays in the Trash.
        assert_eq!(
            std::fs::read(scratch.0.join("Documents/Notes.txt")).expect("read"),
            b"newer"
        );
        assert!(clash.path.exists());
    }

    #[test]
    fn delete_removes_only_the_item_that_was_listed() {
        let scratch = Scratch::new("delete");
        let state = TrashState::default();
        let gone = scratch.trashed("Old.zip", None);
        let swapped = scratch.trashed("Swapped.txt", None);
        state.set(vec![gone.clone(), swapped.clone()]);
        // Replaced after the listing: a different file under the same name. Written first and
        // renamed over it, so both exist at once and the new one can't reuse the old inode
        // (Linux file systems hand a freed inode straight back; APFS doesn't).
        let other = swapped.path.with_extension("new");
        std::fs::write(&other, b"someone else's").expect("write");
        std::fs::rename(&other, &swapped.path).expect("replace");

        let outcome = delete(&state, &["Old.zip".to_owned(), "Swapped.txt".to_owned()]);
        assert_eq!(outcome.done, ["Old.zip"]);
        assert_eq!(outcome.bytes, 8);
        assert!(!gone.path.exists());
        assert_eq!(
            outcome.failed,
            [("Swapped.txt".to_owned(), TrashFailure::Changed)]
        );
        assert!(swapped.path.exists());
        // What was deleted is forgotten; asking again finds nothing.
        assert_eq!(
            delete(&state, &["Old.zip".to_owned()]).failed,
            [("Old.zip".to_owned(), TrashFailure::Changed)]
        );
    }

    #[test]
    fn the_page_gets_tagged_lists() {
        let text = serde_json::to_string(&TrashList::NotAllowed).unwrap_or_default();
        assert_eq!(text, r#"{"status":"notAllowed"}"#);
    }
}
