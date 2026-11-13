//! Race-free, no-follow directory access for the scanner.
//!
//! Path-based calls (`read_dir(path)`) re-resolve the path each time, so a folder swapped for
//! a symlink between checking and listing would be followed. Here every folder is opened
//! *relative to its already-open parent* with `O_NOFOLLOW | O_DIRECTORY`, and its identity is
//! read from the opened handle, so a swap fails to open or is detected; it is never followed.
//!
//! This is the only `unsafe` code in storage discovery. Each block calls one POSIX function
//! with arguments that satisfy its contract, as noted in the SAFETY comments.

use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;

/// The parts of `stat` the scanner uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub dev: u64,
    pub ino: u64,
    mode: u32,
    pub nlink: u64,
    /// 512-byte blocks allocated on disk.
    pub blocks: u64,
    /// Last content modification, in seconds since the Unix epoch.
    pub modified_s: i64,
    /// Last access, in seconds since the Unix epoch (updated lazily by macOS).
    pub accessed_s: i64,
}

// `stat` field and `S_IF*` widths differ by platform (for example `mode_t` is u16 on macOS
// and u32 on Linux), so a widening that is needed on one target is a no-op on another.
#[allow(clippy::useless_conversion, clippy::unnecessary_cast)]
impl FileStat {
    fn from_raw(stat: &libc::stat) -> Self {
        // Widened the same way `std::os::unix::fs::MetadataExt` does, so values compare
        // equal to ones captured through `std` (e.g. the root identity).
        Self {
            dev: stat.st_dev as u64,
            ino: stat.st_ino as u64,
            mode: u32::from(stat.st_mode),
            nlink: stat.st_nlink as u64,
            blocks: u64::try_from(stat.st_blocks).unwrap_or(0),
            modified_s: stat.st_mtime as i64,
            accessed_s: stat.st_atime as i64,
        }
    }

    fn file_type(&self) -> u32 {
        self.mode & u32::from(libc::S_IFMT)
    }

    /// The permission bits (`0o7777`).
    pub fn permissions(&self) -> u32 {
        self.mode & 0o7777
    }

    pub fn is_file(&self) -> bool {
        self.file_type() == u32::from(libc::S_IFREG)
    }

    pub fn is_dir(&self) -> bool {
        self.file_type() == u32::from(libc::S_IFDIR)
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type() == u32::from(libc::S_IFLNK)
    }
}

const OPEN_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// An open directory stream. Closed on drop.
pub struct DirHandle {
    dir: NonNull<libc::DIR>,
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn clear_errno() {
    // SAFETY: the errno location is thread-local and always valid to write.
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error() = 0;
    }
    #[cfg(not(target_os = "macos"))]
    unsafe {
        *libc::__errno_location() = 0;
    }
}

impl DirHandle {
    /// Wraps an open directory file descriptor in a stream, closing it on failure.
    fn from_fd(fd: libc::c_int) -> io::Result<(Self, FileStat)> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `fd` is an open descriptor owned by us; `stat` points to writable storage.
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
            let error = last_error();
            // SAFETY: `fd` is open and not yet owned by a stream.
            unsafe { libc::close(fd) };
            return Err(error);
        }
        // SAFETY: fstat succeeded, so `stat` is initialized.
        let stat = FileStat::from_raw(unsafe { &stat.assume_init() });
        // SAFETY: `fd` is an open directory descriptor; on success the stream owns it.
        let Some(dir) = NonNull::new(unsafe { libc::fdopendir(fd) }) else {
            let error = last_error();
            // SAFETY: fdopendir failed, so `fd` is still ours to close.
            unsafe { libc::close(fd) };
            return Err(error);
        };
        Ok((Self { dir }, stat))
    }

    /// Opens `path` as a directory without following a symlink at its final component.
    pub fn open_root(path: &Path) -> io::Result<(Self, FileStat)> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: `path` is a valid NUL-terminated string for the duration of the call.
        let fd = unsafe { libc::open(path.as_ptr(), OPEN_FLAGS) };
        if fd < 0 {
            return Err(last_error());
        }
        Self::from_fd(fd)
    }

    /// Opens the child directory `name` relative to this one, without following symlinks.
    pub fn open_child(&self, name: &CStr) -> io::Result<(Self, FileStat)> {
        // SAFETY: `self.dir` is an open stream; `name` is NUL-terminated and outlives the call.
        let fd = unsafe { libc::openat(libc::dirfd(self.dir.as_ptr()), name.as_ptr(), OPEN_FLAGS) };
        if fd < 0 {
            return Err(last_error());
        }
        Self::from_fd(fd)
    }

    /// `lstat` of the child `name`, relative to this directory (symlinks are not followed).
    pub fn stat_child(&self, name: &CStr) -> io::Result<FileStat> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid open stream, NUL-terminated name, writable `stat`.
        let result = unsafe {
            libc::fstatat(
                libc::dirfd(self.dir.as_ptr()),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            return Err(last_error());
        }
        // SAFETY: fstatat succeeded, so `stat` is initialized.
        Ok(FileStat::from_raw(unsafe { &stat.assume_init() }))
    }

    /// Opens the regular file `name` in this directory for reading, relative to the open
    /// directory and without following a symlink. `O_NONBLOCK` keeps a FIFO from blocking the
    /// open; the caller checks the handle is a regular file.
    pub fn open_file_child(&self, name: &CStr) -> io::Result<std::fs::File> {
        use std::os::fd::FromRawFd;
        const FLAGS: libc::c_int =
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: `self.dir` is an open stream; `name` is NUL-terminated and outlives the call.
        let fd = unsafe { libc::openat(libc::dirfd(self.dir.as_ptr()), name.as_ptr(), FLAGS) };
        if fd < 0 {
            return Err(last_error());
        }
        // SAFETY: `fd` is a newly opened descriptor that nothing else owns.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    /// The target of the symlink `name` in this directory (`readlinkat`), read relative to
    /// the open directory, so a swapped parent can't redirect it.
    pub fn read_link_child(&self, name: &CStr) -> io::Result<std::path::PathBuf> {
        let mut buffer = vec![0_u8; 4096];
        // SAFETY: valid open stream, NUL-terminated name, and `buffer` is writable for its
        // whole length, which is the size passed.
        let written = unsafe {
            libc::readlinkat(
                libc::dirfd(self.dir.as_ptr()),
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        let written = usize::try_from(written).map_err(|_| last_error())?;
        if written >= buffer.len() {
            // Possibly truncated: refuse rather than guess.
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        buffer.truncate(written);
        Ok(std::path::PathBuf::from(OsStr::from_bytes(&buffer)))
    }

    /// The open directory's descriptor, for `*at` calls made relative to it. It stays owned
    /// by this handle and is valid only while the handle lives.
    pub fn raw_fd(&self) -> libc::c_int {
        // SAFETY: `self.dir` is an open stream; dirfd only reads its descriptor.
        unsafe { libc::dirfd(self.dir.as_ptr()) }
    }

    /// The next entry's name, skipping `.` and `..`. `None` at the end of the directory.
    pub fn next_name(&mut self) -> Option<io::Result<CString>> {
        loop {
            clear_errno();
            // SAFETY: `self.dir` is an open stream used only from this thread.
            let entry = unsafe { libc::readdir(self.dir.as_ptr()) };
            if entry.is_null() {
                let error = last_error();
                return (error.raw_os_error() != Some(0)).then_some(Err(error));
            }
            // SAFETY: `entry` is valid until the next readdir/closedir on this stream, and
            // `d_name` is NUL-terminated; the name is copied before either happens.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                return Some(Ok(name.to_owned()));
            }
        }
    }
}

impl Drop for DirHandle {
    fn drop(&mut self) {
        // SAFETY: `self.dir` is an open stream owned by this handle and closed exactly once.
        unsafe { libc::closedir(self.dir.as_ptr()) };
    }
}

/// Displays a file name reversibly: valid UTF-8 as is, each invalid byte as `\x{XX}`, so two
/// distinct names never collapse into the same text (as `to_string_lossy` can).
pub fn display_name(name: &OsStr) -> String {
    let mut out = String::new();
    for chunk in name.as_bytes().utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            out.push_str(&format!("\\x{{{byte:02X}}}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("kleari-fsdir-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        fs::canonicalize(&path).expect("canonical")
    }

    fn c(name: &str) -> CString {
        CString::new(name).expect("name")
    }

    #[test]
    fn lists_entries_without_dot_entries_and_reports_identity() {
        let root = temp_dir("list");
        fs::create_dir(root.join("sub")).expect("sub");
        fs::write(root.join("a.txt"), b"x").expect("file");
        let (mut dir, stat) = DirHandle::open_root(&root).expect("open");
        assert!(stat.is_dir());
        let mut names: Vec<String> = std::iter::from_fn(|| dir.next_name())
            .map(|name| name.expect("entry").into_string().expect("utf8"))
            .collect();
        names.sort();
        assert_eq!(names, ["a.txt", "sub"]);
        let (_, sub) = dir.open_child(&c("sub")).expect("open child");
        assert_eq!(dir.stat_child(&c("sub")).expect("stat"), sub);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn symlinks_are_never_opened_as_directories() {
        let root = temp_dir("nofollow");
        let target = root.join("target");
        fs::create_dir(&target).expect("target");
        std::os::unix::fs::symlink(&target, root.join("link")).expect("symlink");
        // The root itself, given as a symlink, is refused.
        assert!(DirHandle::open_root(&root.join("link")).is_err());
        let (dir, _) = DirHandle::open_root(&root).expect("open");
        // A symlink child is refused even though it points at a directory.
        assert!(dir.open_child(&c("link")).is_err());
        assert!(dir.stat_child(&c("link")).expect("lstat").is_symlink());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_folder_swapped_for_a_symlink_after_checking_is_not_followed() {
        let root = temp_dir("swap");
        let outside = temp_dir("swap-outside");
        fs::create_dir(root.join("child")).expect("child");
        let (dir, _) = DirHandle::open_root(&root).expect("open");
        assert!(dir.stat_child(&c("child")).expect("stat").is_dir());
        // Swap between the check and the open.
        fs::remove_dir(root.join("child")).expect("remove");
        std::os::unix::fs::symlink(&outside, root.join("child")).expect("swap in symlink");
        assert!(dir.open_child(&c("child")).is_err());
        fs::remove_dir_all(&root).expect("cleanup");
        fs::remove_dir_all(&outside).expect("cleanup");
    }

    #[test]
    fn files_open_relative_to_the_directory_and_symlinks_are_refused() {
        use std::io::Read;
        let root = temp_dir("openfile");
        fs::write(root.join("file"), b"hello").expect("file");
        std::os::unix::fs::symlink(root.join("file"), root.join("link")).expect("symlink");
        let (dir, _) = DirHandle::open_root(&root).expect("open");
        let mut text = String::new();
        dir.open_file_child(&c("file"))
            .expect("opens")
            .read_to_string(&mut text)
            .expect("reads");
        assert_eq!(text, "hello");
        assert!(dir.open_file_child(&c("link")).is_err());
        assert!(dir.open_file_child(&c("missing")).is_err());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn symlink_targets_are_read_relative_to_the_open_directory() {
        let root = temp_dir("readlink");
        std::os::unix::fs::symlink("some/target", root.join("link")).expect("symlink");
        fs::create_dir(root.join("folder")).expect("folder");
        let (dir, _) = DirHandle::open_root(&root).expect("open");
        assert_eq!(
            dir.read_link_child(&c("link")).expect("readlink"),
            std::path::PathBuf::from("some/target")
        );
        // Not a symlink, or missing: an error, never a guess.
        assert!(dir.read_link_child(&c("folder")).is_err());
        assert!(dir.read_link_child(&c("missing")).is_err());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn display_names_escape_invalid_bytes_reversibly() {
        assert_eq!(display_name(OsStr::new("Été.mov")), "Été.mov");
        let ff = OsStr::from_bytes(b"x\xff");
        let fe = OsStr::from_bytes(b"x\xfe");
        assert_eq!(display_name(ff), "x\\x{FF}");
        assert_ne!(display_name(ff), display_name(fe));
    }
}
