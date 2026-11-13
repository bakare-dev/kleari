//! The Storage page's Browse view: one folder of the home folder at a time, every entry in it
//! measured, down to single files, so you can find what takes space and choose what to send
//! to Cleanup's review.
//!
//! Read-only. Folders are opened relative to their open parent without following symlinks
//! (`fsdir`), other volumes aren't entered, no file is opened, and folders excluded in
//! Settings are listed by name only, never entered or measured. The page has only opaque IDs;
//! each one's path and identity stay here, and a folder whose identity changed is refused.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::cleanup::policy::{self, Kind};
use crate::developer::{measure_dir, open_absolute, Links, Measured};
use crate::ids::new_id;
use crate::storage::category::{category_of, FileCategory};
use crate::storage::fsdir::FileStat;
use crate::system_monitor::display_path;

/// Entries listed per folder, largest first; the rest are combined.
const MAX_ENTRIES: usize = 2_000;
/// IDs remembered at once; past this, older ones are forgotten (the page lists again).
const MAX_NODES: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Folder,
    File,
    /// A symbolic link: listed, never followed or chosen.
    Link,
    /// Excluded in Settings: listed by name, never entered or measured.
    Excluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub kind: EntryKind,
    pub bytes: u64,
    pub lower_bound: bool,
    pub files: u64,
    pub modified_ms: Option<u64>,
    pub file_category: Option<FileCategory>,
    /// It can be sent to the review (inside the home folder, outside the places Kleari never
    /// touches and outside a package).
    pub selectable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ListingStatus {
    Completed,
    Cancelled,
    /// The folder is gone or was replaced since it was listed: go up and list again.
    Changed,
    /// macOS refused to open it (Full Disk Access, or a folder permission).
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub status: ListingStatus,
    pub id: String,
    pub name: String,
    /// The folder, shown as `~/…`.
    pub location: String,
    /// From the home folder down to this folder.
    pub crumbs: Vec<Crumb>,
    pub bytes: u64,
    pub lower_bound: bool,
    pub entries: Vec<Entry>,
    /// Entries beyond `MAX_ENTRIES`, combined.
    pub more_entries: u64,
    pub more_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingProgress {
    pub done: u32,
    pub total: u32,
}

/// What a chosen entry is, for the review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub name: String,
    pub path: PathBuf,
    pub stat: FileStat,
    pub measured: Measured,
}

#[derive(Debug, Clone)]
struct Node {
    path: PathBuf,
    stat: FileStat,
    parent: Option<String>,
    measured: Option<Measured>,
    kind: EntryKind,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
pub struct ExplorerState {
    nodes: Mutex<HashMap<String, Node>>,
    root: Mutex<Option<String>>,
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl ExplorerState {
    /// Starts a listing, stopping any listing still running (the page moved on).
    pub fn begin(&self) -> Arc<AtomicBool> {
        let mut active = lock(&self.active);
        if let Some(previous) = active.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        cancel
    }

    pub fn cancel(&self) {
        if let Some(cancel) = &*lock(&self.active) {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The chosen entries, by ID, with what their listing measured.
    pub fn chosen(&self, ids: &[String]) -> Vec<Chosen> {
        let nodes = lock(&self.nodes);
        ids.iter()
            .filter_map(|id| nodes.get(id))
            .filter(|node| matches!(node.kind, EntryKind::Folder | EntryKind::File))
            .filter_map(|node| {
                Some(Chosen {
                    name: node.path.file_name()?.to_string_lossy().into_owned(),
                    path: node.path.clone(),
                    stat: node.stat,
                    measured: node.measured.clone()?,
                })
            })
            .collect()
    }

    /// Lists the folder `id`, or the home folder when `None`.
    pub fn list(
        &self,
        home: &Path,
        id: Option<&str>,
        excluded: &[PathBuf],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(ListingProgress),
    ) -> Option<Listing> {
        let (id, node) = match id {
            Some(id) => (id.to_owned(), lock(&self.nodes).get(id).cloned()?),
            None => self.home_node(home)?,
        };
        if node.kind != EntryKind::Folder {
            return None;
        }
        let crumbs = self.crumbs(&id);
        let name = node.path.file_name().map_or_else(
            || "~".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let mut listing = Listing {
            status: ListingStatus::Completed,
            id: id.clone(),
            name: if node.path == home {
                "Home".to_owned()
            } else {
                name
            },
            location: display_path(&node.path, Some(home)),
            crumbs,
            bytes: 0,
            lower_bound: false,
            entries: Vec::new(),
            more_entries: 0,
            more_bytes: 0,
        };
        let Some((mut dir, dev)) = open_absolute(&node.path) else {
            listing.status = ListingStatus::Unreadable;
            return Some(listing);
        };
        if !open_is(&node.path, &node.stat) {
            listing.status = ListingStatus::Changed;
            return Some(listing);
        }
        let names: Vec<CString> = std::iter::from_fn(|| dir.next_name())
            .filter_map(Result::ok)
            .collect();
        let total = u32::try_from(names.len()).unwrap_or(u32::MAX);
        let mut links = Links::default();
        let mut found: Vec<(Entry, Node)> = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                listing.status = ListingStatus::Cancelled;
                break;
            }
            progress(ListingProgress {
                done: u32::try_from(index).unwrap_or(u32::MAX),
                total,
            });
            let Ok(stat) = dir.stat_child(name) else {
                continue;
            };
            let path = node.path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
            let display = name.to_string_lossy().into_owned();
            let excluded_here = crate::preferences::touches_excluded(&path, excluded);
            let (kind, measured) = if excluded_here {
                (EntryKind::Excluded, None)
            } else if stat.is_symlink() {
                (EntryKind::Link, None)
            } else if stat.is_dir() {
                if stat.dev != dev {
                    // Another volume mounted here isn't entered.
                    continue;
                }
                let measured = match dir.open_child(name) {
                    Ok((child, opened)) if opened.ino == stat.ino && opened.dev == stat.dev => {
                        measure_dir(child, dev, &mut links, cancel)
                    }
                    _ => unreadable(&stat),
                };
                (EntryKind::Folder, Some(measured))
            } else if stat.is_file() {
                (EntryKind::File, Some(file(&stat)))
            } else {
                continue;
            };
            let bytes = measured.as_ref().map_or(0, |measured| measured.bytes);
            let lower_bound = measured
                .as_ref()
                .is_some_and(|measured| measured.lower_bound);
            let selectable = matches!(kind, EntryKind::Folder | EntryKind::File)
                && policy::within_bounds(&Kind::UserChosen, &path, home, None, |_| Vec::new());
            let entry = Entry {
                id: new_id(0),
                name: display,
                kind,
                bytes,
                lower_bound,
                files: measured.as_ref().map_or(0, |measured| measured.files),
                modified_ms: measured
                    .as_ref()
                    .map(|measured| measured.newest_modified_s.max(stat.modified_s))
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .filter(|&seconds| seconds > 0)
                    .and_then(|seconds| seconds.checked_mul(1000)),
                file_category: (kind == EntryKind::File).then(|| category_of(&path)),
                selectable,
            };
            found.push((
                entry,
                Node {
                    path,
                    stat,
                    parent: Some(id.clone()),
                    measured,
                    kind,
                },
            ));
        }
        found.sort_by_key(|(entry, _)| std::cmp::Reverse(entry.bytes));
        listing.bytes = found
            .iter()
            .fold(0_u64, |total, (entry, _)| total.saturating_add(entry.bytes));
        listing.lower_bound = found.iter().any(|(entry, _)| entry.lower_bound)
            || listing.status == ListingStatus::Cancelled;
        let mut nodes = lock(&self.nodes);
        if nodes.len().saturating_add(found.len()) > MAX_NODES {
            // Too many remembered: keep only the way down to this folder.
            let keep: Vec<String> = listing
                .crumbs
                .iter()
                .map(|crumb| crumb.id.clone())
                .collect();
            nodes.retain(|key, _| keep.contains(key));
        }
        for (index, (entry, node)) in found.into_iter().enumerate() {
            if index >= MAX_ENTRIES {
                listing.more_entries = listing.more_entries.saturating_add(1);
                listing.more_bytes = listing.more_bytes.saturating_add(entry.bytes);
                continue;
            }
            nodes.insert(entry.id.clone(), node);
            listing.entries.push(entry);
        }
        Some(listing)
    }

    /// The home folder's node, made once per session.
    fn home_node(&self, home: &Path) -> Option<(String, Node)> {
        let mut root = lock(&self.root);
        if let Some(id) = root.as_ref() {
            if let Some(node) = lock(&self.nodes).get(id) {
                return Some((id.clone(), node.clone()));
            }
        }
        let (_, stat) = crate::storage::fsdir::DirHandle::open_root(home).ok()?;
        let id = new_id(0);
        let node = Node {
            path: home.to_path_buf(),
            stat,
            parent: None,
            measured: None,
            kind: EntryKind::Folder,
        };
        lock(&self.nodes).insert(id.clone(), node.clone());
        *root = Some(id.clone());
        Some((id, node))
    }

    fn crumbs(&self, id: &str) -> Vec<Crumb> {
        let nodes = lock(&self.nodes);
        let mut crumbs = Vec::new();
        let mut current = Some(id.to_owned());
        while let Some(key) = current {
            let Some(node) = nodes.get(&key) else {
                break;
            };
            let name = if node.parent.is_none() {
                "Home".to_owned()
            } else {
                node.path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            crumbs.push(Crumb {
                id: key.clone(),
                name,
            });
            current = node.parent.clone();
        }
        crumbs.reverse();
        crumbs
    }
}

fn file(stat: &FileStat) -> Measured {
    Measured {
        bytes: stat.blocks.saturating_mul(512),
        files: 1,
        lower_bound: false,
        parts: Vec::new(),
        newest_modified_s: stat.modified_s,
    }
}

fn unreadable(stat: &FileStat) -> Measured {
    Measured {
        bytes: 0,
        files: 0,
        lower_bound: true,
        parts: Vec::new(),
        newest_modified_s: stat.modified_s,
    }
}

/// Whether `path` is still the folder recorded (same device and inode), not following links.
fn open_is(path: &Path, recorded: &FileStat) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        // The file-system root: nothing above to compare through.
        return crate::storage::fsdir::DirHandle::open_root(path)
            .is_ok_and(|(_, stat)| stat.dev == recorded.dev && stat.ino == recorded.ino);
    };
    let Ok(name) = CString::new(name.as_bytes()) else {
        return false;
    };
    open_absolute(parent)
        .and_then(|(dir, _)| dir.stat_child(&name).ok())
        .is_some_and(|now| now.is_dir() && now.dev == recorded.dev && now.ino == recorded.ino)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn home(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("kleari-explore-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("home");
        fs::canonicalize(&root).expect("real path")
    }

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(path, vec![1_u8; bytes]).expect("write");
    }

    #[test]
    fn lists_down_to_single_files_and_marks_what_can_be_chosen() {
        let home = home("drill");
        write(&home.join("Projects/site/video.mov"), 400_000);
        write(&home.join("Projects/site/notes.txt"), 1_000);
        write(&home.join("Downloads/big.zip"), 200_000);
        write(&home.join("Library/Caches/com.example/blob"), 50_000);
        write(&home.join("Library/Preferences/x.plist"), 10);
        write(&home.join("Private/secret"), 10);
        std::os::unix::fs::symlink(home.join("Projects"), home.join("link")).expect("link");
        let excluded = [home.join("Private")];
        let state = ExplorerState::default();
        let cancel = AtomicBool::new(false);

        let root = state
            .list(&home, None, &excluded, &cancel, &mut |_| {})
            .expect("home listed");
        assert_eq!(root.status, ListingStatus::Completed);
        assert_eq!(root.crumbs.len(), 1);
        let by = |listing: &Listing, name: &str| {
            listing
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} listed"))
        };
        // Largest first; the top-level home folders themselves can't be chosen.
        assert_eq!(root.entries[0].name, "Projects");
        assert!(by(&root, "Projects").selectable);
        assert!(!by(&root, "Downloads").selectable);
        assert!(!by(&root, "Library").selectable);
        assert_eq!(by(&root, "Private").kind, EntryKind::Excluded);
        assert_eq!(by(&root, "Private").bytes, 0);
        assert_eq!(by(&root, "link").kind, EntryKind::Link);
        assert!(!by(&root, "link").selectable);

        let projects = state
            .list(
                &home,
                Some(&by(&root, "Projects").id),
                &excluded,
                &cancel,
                &mut |_| {},
            )
            .expect("listed");
        let site = state
            .list(
                &home,
                Some(&by(&projects, "site").id),
                &excluded,
                &cancel,
                &mut |_| {},
            )
            .expect("listed");
        assert_eq!(
            site.crumbs
                .iter()
                .map(|crumb| crumb.name.as_str())
                .collect::<Vec<_>>(),
            ["Home", "Projects", "site"]
        );
        let video = by(&site, "video.mov");
        assert_eq!(video.kind, EntryKind::File);
        assert_eq!(video.file_category, Some(FileCategory::Video));
        assert!(video.selectable);

        // In the Library, only below Caches, Logs or Developer.
        let library = state
            .list(
                &home,
                Some(&by(&root, "Library").id),
                &excluded,
                &cancel,
                &mut |_| {},
            )
            .expect("listed");
        assert!(!by(&library, "Caches").selectable);
        let caches = state
            .list(
                &home,
                Some(&by(&library, "Caches").id),
                &excluded,
                &cancel,
                &mut |_| {},
            )
            .expect("listed");
        assert!(by(&caches, "com.example").selectable);
        let preferences = state
            .list(
                &home,
                Some(&by(&library, "Preferences").id),
                &excluded,
                &cancel,
                &mut |_| {},
            )
            .expect("listed");
        assert!(!by(&preferences, "x.plist").selectable);

        // What is chosen comes back with its path and size, only for files and folders.
        let chosen = state.chosen(&[video.id.clone(), by(&root, "link").id]);
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].path, home.join("Projects/site/video.mov"));
        assert!(chosen[0].measured.bytes >= 400_000);

        // A folder replaced since it was listed is refused. The new folder is made while the
        // old one still exists, so it can't reuse its inode (Linux may reuse a freed one).
        let site_id = by(&projects, "site").id;
        fs::create_dir_all(home.join("Projects/site-new")).expect("new folder");
        fs::remove_dir_all(home.join("Projects/site")).expect("remove");
        fs::rename(home.join("Projects/site-new"), home.join("Projects/site")).expect("replace");
        let changed = state
            .list(&home, Some(&site_id), &excluded, &cancel, &mut |_| {})
            .expect("answered");
        assert_eq!(changed.status, ListingStatus::Changed);
        fs::remove_dir_all(&home).expect("clean up");
    }

    #[test]
    fn nothing_inside_a_package_can_be_chosen() {
        let home = Path::new("/Users/someone");
        let chosen = |relative: &str| {
            policy::within_bounds(&Kind::UserChosen, &home.join(relative), home, None, |_| {
                Vec::new()
            })
        };
        assert!(chosen("Pictures/Photos Library.photoslibrary"));
        assert!(!chosen(
            "Pictures/Photos Library.photoslibrary/originals/a.heic"
        ));
        assert!(chosen("Applications/Tool.app"));
        assert!(!chosen("Applications/Tool.app/Contents"));
        assert!(!chosen(".Trash/old"));
        assert!(!chosen(".ssh/id_ed25519"));
        assert!(!chosen("Library/Application Support/App"));
        assert!(chosen("Movies/holiday.mov"));
        assert!(!chosen("Movies"));
    }
}
