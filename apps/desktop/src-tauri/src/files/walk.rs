//! The walk the Files tools share: every regular file in the home folder you could choose
//! yourself, read-only. Folders are opened relative to their open parent without following
//! symlinks; other volumes, the Library, the Trash, packages (`.app`, `.photoslibrary`, …),
//! `node_modules`, `.git`, private dotfiles and folders excluded in Settings are not entered.

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cleanup::scan::{hidden_skipped, skipped_folder};
use crate::developer::open_absolute;
use crate::storage::fsdir::{DirHandle, FileStat};

/// Folders below the home folder walked at most.
const MAX_DEPTH: usize = 64;
/// Files looked at, at most, so a huge home folder can't run forever.
pub const MAX_FILES: u64 = 3_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Walked {
    pub files: u64,
    /// Folders macOS didn't let Kleari open.
    pub unreadable: u64,
    /// Stopped at `MAX_FILES`.
    pub truncated: bool,
}

/// Calls `visit` for each regular file below `root` (the home folder or a folder in it).
pub fn walk(
    root: &Path,
    excluded: &[PathBuf],
    cancel: &AtomicBool,
    visit: &mut dyn FnMut(&Path, &FileStat),
) -> Walked {
    let mut walked = Walked::default();
    if crate::preferences::touches_excluded(root, excluded) {
        return walked;
    }
    let Some((dir, dev)) = open_absolute(root) else {
        walked.unreadable = 1;
        return walked;
    };
    let mut stack: Vec<(DirHandle, PathBuf)> = vec![(dir, root.to_path_buf())];
    while let Some((mut dir, path)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let names: Vec<CString> = std::iter::from_fn(|| dir.next_name())
            .filter_map(Result::ok)
            .collect();
        for name in names {
            let bytes = name.to_bytes();
            if hidden_skipped(bytes, false) || bytes == b".Trash" {
                continue;
            }
            let child = path.join(OsStr::from_bytes(bytes));
            if crate::preferences::touches_excluded(&child, excluded) {
                continue;
            }
            let Ok(stat) = dir.stat_child(&name) else {
                continue;
            };
            if stat.is_symlink() || stat.dev != dev {
                continue;
            }
            if stat.is_file() {
                walked.files += 1;
                if walked.files > MAX_FILES {
                    walked.truncated = true;
                    return walked;
                }
                visit(&child, &stat);
            } else if stat.is_dir() && stack.len() < MAX_DEPTH && !skipped_folder(bytes) {
                match dir.open_child(&name) {
                    Ok((opened, opened_stat))
                        if opened_stat.dev == stat.dev && opened_stat.ino == stat.ino =>
                    {
                        stack.push((opened, child));
                    }
                    _ => walked.unreadable += 1,
                }
            }
        }
    }
    walked
}

/// Opens the file at `path` read-only, only if it is still the file `stat` describes (same
/// device and inode), without following symlinks.
pub fn open_same(path: &Path, stat: &FileStat) -> Option<std::fs::File> {
    let parent = path.parent()?;
    let name = CString::new(path.file_name()?.as_bytes()).ok()?;
    let (dir, _) = open_absolute(parent)?;
    let now = dir.stat_child(&name).ok()?;
    if !now.is_file() || now.dev != stat.dev || now.ino != stat.ino {
        return None;
    }
    let file = dir.open_file_child(&name).ok()?;
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().ok()?;
    (metadata.dev() == stat.dev && metadata.ino() == stat.ino).then_some(file)
}
