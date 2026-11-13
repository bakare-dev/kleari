//! Sizes one folder read-only for the Developer page: allocated bytes, file count and its
//! largest parts. Works from an already-open directory handle (`storage::fsdir`): every
//! folder below it is opened relative to its open parent without following symlinks, with
//! an identity check, and other volumes are not entered. No file is opened. Memory is
//! bounded: the open folders on the current path, the three largest parts and one running
//! total for the rest.

use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::storage::fsdir::{display_name, DirHandle, FileStat};
use crate::system_monitor::non_local_mounts;

/// Folders open at once along the current path.
const MAX_DEPTH: usize = 128;
/// Largest parts listed per folder; the rest are combined.
const PARTS: usize = 3;
/// Hard-link identities remembered across one scan.
pub const MAX_HARD_LINKS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// A folder or file directly inside; empty for the combined rest.
    pub name: String,
    pub bytes: u64,
    /// How many entries a combined row covers (1 otherwise).
    pub items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub bytes: u64,
    pub files: u64,
    /// Something couldn't be read or counted (or the scan was cancelled): a minimum.
    pub lower_bound: bool,
    /// The largest entries directly inside, largest first, then the rest combined.
    pub parts: Vec<Part>,
    /// The latest modification time of any file inside, in seconds since the Unix epoch (0
    /// when empty). Cleanup uses it to tell old logs from ones still written; not sent.
    #[serde(skip)]
    pub newest_modified_s: i64,
}

/// Hard links seen during one scan. A file linked from several places (pnpm links packages
/// from its store into each project) is counted once; past the cap, links aren't counted at
/// all and the total becomes a minimum.
#[derive(Default)]
pub struct Links {
    seen: HashSet<(u64, u64)>,
}

/// The largest few entries and one running total for the rest, so a folder with a huge
/// number of direct entries costs constant memory.
#[derive(Default)]
struct Top {
    largest: Vec<Part>,
    rest_bytes: u64,
    rest_items: u64,
}

impl Top {
    fn offer(&mut self, part: Part) {
        let position = self.largest.partition_point(|kept| {
            (kept.bytes, std::cmp::Reverse(&kept.name))
                >= (part.bytes, std::cmp::Reverse(&part.name))
        });
        if position < PARTS {
            self.largest.insert(position, part);
            if self.largest.len() > PARTS {
                if let Some(dropped) = self.largest.pop() {
                    self.rest_bytes = self.rest_bytes.saturating_add(dropped.bytes);
                    self.rest_items += dropped.items;
                }
            }
        } else {
            self.rest_bytes = self.rest_bytes.saturating_add(part.bytes);
            self.rest_items += part.items;
        }
    }

    fn into_parts(self) -> Vec<Part> {
        let mut parts = self.largest;
        if self.rest_items > 0 {
            parts.push(Part {
                name: String::new(),
                bytes: self.rest_bytes,
                items: self.rest_items,
            });
        }
        parts
    }
}

fn name_of(name: &CString) -> String {
    display_name(OsStr::from_bytes(name.to_bytes()))
}

/// Opens `relative` below the open folder `base` one component at a time, relative to each
/// open parent, refusing symlinks, identity changes and other volumes (`dev`). `None` if
/// anything along the way isn't a plain folder on that volume.
pub fn open_beneath(base: &DirHandle, dev: u64, relative: &Path) -> Option<DirHandle> {
    let mut current: Option<DirHandle> = None;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        let name = CString::new(name.as_bytes()).ok()?;
        let parent = current.as_ref().unwrap_or(base);
        let stat = parent.stat_child(&name).ok()?;
        if !stat.is_dir() || stat.dev != dev {
            return None;
        }
        let (child, opened) = parent.open_child(&name).ok()?;
        if !opened.is_dir() || opened.dev != stat.dev || opened.ino != stat.ino {
            return None;
        }
        current = Some(child);
    }
    current
}

/// `path` is under a mount the mount table reports as non-local, or the table can't be read.
fn non_local(path: &Path) -> bool {
    non_local_mounts().is_none_or(|mounts| mounts.iter().any(|mount| path.starts_with(mount)))
}

/// Opens the absolute folder `path` from `/`, one component at a time relative to each open
/// parent, refusing a symlink anywhere along the way (so the lexical path is the real one and
/// a mount-table check on it holds) and checking each folder's identity. Unlike
/// `open_beneath`, the volume may change along the way (a folder on another local disk).
///
/// The mount table is read again before each component, and once more after a step that
/// lands on another volume, before anything inside it is read, so a non-local volume mounted
/// during the walk is refused. Nothing is opened if the table can't be read. Returns the
/// folder and its volume.
pub fn open_absolute(path: &Path) -> Option<(DirHandle, u64)> {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let (mut dir, mut stat) = DirHandle::open_root(Path::new("/")).ok()?;
    let mut current = PathBuf::from("/");
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        current.push(name);
        if non_local(&current) {
            return None;
        }
        let name = CString::new(name.as_bytes()).ok()?;
        let child = dir.stat_child(&name).ok()?;
        if !child.is_dir() {
            return None;
        }
        let (next, opened) = dir.open_child(&name).ok()?;
        if !opened.is_dir() || opened.dev != child.dev || opened.ino != child.ino {
            return None;
        }
        if opened.dev != stat.dev && non_local(&current) {
            return None;
        }
        dir = next;
        stat = opened;
    }
    Some((dir, stat.dev))
}

/// Sizes the open folder `root`, which is on volume `dev`.
pub fn measure_dir(root: DirHandle, dev: u64, links: &mut Links, cancel: &AtomicBool) -> Measured {
    let mut measured = Measured {
        bytes: 0,
        files: 0,
        lower_bound: false,
        parts: Vec::new(),
        newest_modified_s: 0,
    };
    let mut top = Top::default();
    // The direct child being walked, finished into `top` when its subtree is done.
    let mut current: Option<Part> = None;
    let mut stack: Vec<DirHandle> = vec![root];
    loop {
        if cancel.load(Ordering::Relaxed) {
            measured.lower_bound = true;
            break;
        }
        let Some(dir) = stack.last_mut() else {
            break;
        };
        let next = match dir.next_name() {
            None => None,
            Some(Ok(name)) => Some(name),
            Some(Err(_)) => {
                measured.lower_bound = true;
                None
            }
        };
        let Some(name) = next else {
            stack.pop();
            // Back at the root: the direct child just walked is complete.
            if stack.len() == 1 {
                if let Some(part) = current.take() {
                    top.offer(part);
                }
            }
            continue;
        };
        let depth = stack.len();
        let Some(dir) = stack.last() else {
            break;
        };
        let Ok(child) = dir.stat_child(&name) else {
            measured.lower_bound = true;
            continue;
        };
        if child.is_symlink() {
            continue;
        }
        measured.newest_modified_s = measured.newest_modified_s.max(child.modified_s);
        if child.is_dir() {
            if child.dev != dev {
                continue;
            }
            if depth >= MAX_DEPTH {
                measured.lower_bound = true;
                continue;
            }
            let opened = dir.open_child(&name).ok().filter(|(_, opened)| {
                opened.is_dir() && opened.dev == child.dev && opened.ino == child.ino
            });
            let Some((handle, _)) = opened else {
                measured.lower_bound = true;
                continue;
            };
            if depth == 1 {
                current = Some(Part {
                    name: name_of(&name),
                    bytes: 0,
                    items: 1,
                });
            }
            stack.push(handle);
            continue;
        }
        let Some(bytes) = count_file(&child, links, &mut measured) else {
            continue;
        };
        if depth == 1 {
            top.offer(Part {
                name: name_of(&name),
                bytes,
                items: 1,
            });
        } else if let Some(part) = current.as_mut() {
            part.bytes = part.bytes.saturating_add(bytes);
        }
    }
    if let Some(part) = current.take() {
        top.offer(part);
    }
    measured.parts = top.into_parts();
    measured
}

/// Counts a non-directory entry once. `None` if it isn't counted (a hard link already seen,
/// or past the tracking limit, which makes the total a minimum).
fn count_file(stat: &FileStat, links: &mut Links, measured: &mut Measured) -> Option<u64> {
    if stat.nlink > 1 {
        let identity = (stat.dev, stat.ino);
        if links.seen.contains(&identity) {
            return None;
        }
        if links.seen.len() >= MAX_HARD_LINKS {
            measured.lower_bound = true;
            return None;
        }
        links.seen.insert(identity);
    }
    let bytes = stat.blocks.saturating_mul(512);
    measured.bytes = measured.bytes.saturating_add(bytes);
    measured.files += 1;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("kleari-measure-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp");
        fs::canonicalize(&path).expect("canonical")
    }

    fn write(path: &Path, bytes: usize) -> u64 {
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(path, vec![1_u8; bytes]).expect("file");
        fs::metadata(path).expect("metadata").blocks() * 512
    }

    #[test]
    fn opening_beneath_refuses_symlinks_along_the_way() {
        let root = temp("beneath");
        write(&root.join("real/cache/f"), 10_000);
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).expect("symlink");
        let (base, stat) = DirHandle::open_root(&root).expect("open");
        assert!(open_beneath(&base, stat.dev, Path::new("real/cache")).is_some());
        // A symlinked parent, a missing folder, `..` and another device are all refused.
        assert!(open_beneath(&base, stat.dev, Path::new("link/cache")).is_none());
        assert!(open_beneath(&base, stat.dev, Path::new("real/missing")).is_none());
        assert!(open_beneath(&base, stat.dev, Path::new("real/../real")).is_none());
        assert!(open_beneath(&base, stat.dev.wrapping_add(1), Path::new("real")).is_none());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn absolute_paths_open_only_without_symlinked_ancestors() {
        let root = temp("absolute");
        write(&root.join("real/cache/f"), 10_000);
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).expect("symlink");
        let (_, dev) = open_absolute(&root.join("real/cache")).expect("real");
        assert_eq!(dev, fs::metadata(&root).expect("metadata").dev());
        assert!(open_absolute(&root.join("link/cache")).is_none());
        assert!(open_absolute(&root.join("link")).is_none());
        assert!(open_absolute(&root.join("real/cache/f")).is_none());
        assert!(open_absolute(Path::new("relative/path")).is_none());
        assert!(open_absolute(&root.join("real/../real")).is_none());
        assert!(open_absolute(Path::new("/")).is_some());
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
