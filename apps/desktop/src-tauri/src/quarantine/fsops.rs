//! The only filesystem calls in Kleari that change anything. Each works relative to
//! directories already opened without following symlinks (`storage::fsdir`), so a path
//! swapped after it was checked can't redirect them:
//! - `rename_exclusive` moves an entry between two open folders on the same volume and fails
//!   if the destination name exists (`renameatx_np` with `RENAME_EXCL` on macOS,
//!   `renameat2` with `RENAME_NOREPLACE` on Linux). It never overwrites and never copies.
//! - `make_dir` creates one owner-only folder inside an open folder.
//! - `remove_tree` permanently deletes one entry inside an open folder. Folders are walked
//!   through handles opened with no-follow and on the same volume; a symlink inside is
//!   removed as a link, never followed.
//!
//! Each `unsafe` block calls one POSIX function with arguments that satisfy its contract.

use std::ffi::{CStr, CString};
use std::io;

use crate::storage::fsdir::{DirHandle, FileStat};

fn check(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Moves `from_name` in `from` to `to_name` in `to`. Fails with `AlreadyExists` rather than
/// replacing anything at the destination, and with `CrossesDevices` (EXDEV) across volumes.
pub fn rename_exclusive(
    from: &DirHandle,
    from_name: &CStr,
    to: &DirHandle,
    to_name: &CStr,
) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    // SAFETY: both descriptors belong to open directory streams that outlive the call, and
    // both names are NUL-terminated.
    let result = unsafe {
        libc::renameatx_np(
            from.raw_fd(),
            from_name.as_ptr(),
            to.raw_fd(),
            to_name.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(target_os = "linux")]
    // SAFETY: as above.
    let result = unsafe {
        libc::renameat2(
            from.raw_fd(),
            from_name.as_ptr(),
            to.raw_fd(),
            to_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    check(result)
}

/// Creates the folder `name` inside `parent`, readable only by this account.
pub fn make_dir(parent: &DirHandle, name: &CStr) -> io::Result<()> {
    // SAFETY: open directory descriptor and NUL-terminated name.
    check(unsafe { libc::mkdirat(parent.raw_fd(), name.as_ptr(), 0o700) })
}

fn unlink(parent: &DirHandle, name: &CStr, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: open directory descriptor and NUL-terminated name.
    check(unsafe { libc::unlinkat(parent.raw_fd(), name.as_ptr(), flags) })
}

/// Removes the folder `name` in `parent` only if it is empty (`ENOTEMPTY` otherwise).
pub fn remove_empty_dir(parent: &DirHandle, name: &CStr) -> io::Result<()> {
    unlink(parent, name, libc::AT_REMOVEDIR)
}

/// Gives this account full access to an open folder it owns, so its entries can be deleted
/// (Go's module cache, for one, makes its folders read-only).
// `mode_t` is u16 on macOS and u32 on Linux, so the conversion is a no-op on one of them.
#[allow(clippy::useless_conversion)]
fn allow_owner(dir: &DirHandle, stat: &FileStat) -> io::Result<()> {
    let wanted = stat.permissions() | 0o700;
    if wanted == stat.permissions() {
        return Ok(());
    }
    let mode =
        libc::mode_t::try_from(wanted).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: open directory descriptor; fchmod only changes that folder's mode bits.
    check(unsafe { libc::fchmod(dir.raw_fd(), mode) })
}

/// Folders open at once while deleting, one per level.
const MAX_DEPTH: usize = 256;

/// Every name in `dir`, read before anything is removed: deleting entries while a directory
/// stream is being read can make some filesystems skip entries.
fn names(dir: &mut DirHandle) -> io::Result<Vec<CString>> {
    std::iter::from_fn(|| dir.next_name()).collect()
}

/// A folder being emptied: its open handle, its name in the folder above, and the entries in
/// it still to delete.
struct Frame {
    dir: DirHandle,
    name: CString,
    pending: Vec<CString>,
}

/// Opens the folder `name` in `parent` for deletion: same volume, the identity `lstat`
/// reported, and writable by this account.
fn open_for_removal(parent: &DirHandle, name: &CStr, stat: &FileStat) -> io::Result<Frame> {
    let (mut dir, opened) = parent.open_child(name)?;
    if opened.dev != stat.dev || opened.ino != stat.ino {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    allow_owner(&dir, &opened)?;
    let pending = names(&mut dir)?;
    Ok(Frame {
        dir,
        name: name.to_owned(),
        pending,
    })
}

/// Permanently deletes `name` inside `parent`, which must still be the entry with `expected`
/// identity (device, inode, folder or not): anything else under that name is left alone.
/// Everything below it must be on the same volume (anything else is left and reported as an
/// error). Stops at the first error, so a failure leaves the rest in place.
///
/// A folder is emptied only through the handle opened and checked against `expected`, so a
/// replacement put under its name can't be emptied. Two windows remain, as POSIX has no
/// unlink by handle: a file swapped between its last `lstat` and `unlinkat`, and an empty
/// folder swapped in just before the final `rmdir` (which removes only an empty folder).
pub fn remove_tree(parent: &DirHandle, name: &CStr, expected: super::Identity) -> io::Result<()> {
    let stat = parent.stat_child(name)?;
    if !expected.matches(&stat) {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let dev = expected.dev;
    if !stat.is_dir() {
        return unlink(parent, name, 0);
    }
    let mut stack = vec![open_for_removal(parent, name, &stat)?];
    loop {
        let depth = stack.len();
        let Some(top) = stack.last_mut() else {
            break;
        };
        let Some(child) = top.pending.pop() else {
            // Empty now: close it, then remove it from the folder above.
            let Some(done) = stack.pop() else {
                break;
            };
            drop(done.dir);
            let above = stack.last().map_or(parent, |frame| &frame.dir);
            if stack.is_empty() && !expected.matches(&above.stat_child(&done.name)?) {
                // Its name now holds something else; that is left alone.
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            unlink(above, &done.name, libc::AT_REMOVEDIR)?;
            continue;
        };
        let child_stat = top.dir.stat_child(&child)?;
        if child_stat.dev != dev {
            return Err(io::Error::from(io::ErrorKind::CrossesDevices));
        }
        if !child_stat.is_dir() {
            // Files, and symlinks themselves (never what they point to).
            unlink(&top.dir, &child, 0)?;
            continue;
        }
        if depth >= MAX_DEPTH {
            return Err(io::Error::other("folders nested too deeply"));
        }
        let frame = open_for_removal(&top.dir, &child, &child_stat)?;
        stack.push(frame);
    }
    Ok(())
}
