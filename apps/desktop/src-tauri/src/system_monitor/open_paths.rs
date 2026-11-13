//! What your apps have open right now: every file open in, and the working folder of, each
//! of your processes. A project is "in use" (active) while any of it is open, in an editor,
//! a terminal, a dev server or anything else, and only then.
//!
//! Read through `proc_pidinfo` and `proc_pidfdinfo`, like the process sampler: no shell and
//! no privileges. macOS refuses other accounts' processes, which are skipped; they can't be
//! the user's own work in progress. Listing the processes at all failing is `Unavailable`,
//! and callers then treat everything as in use (fail closed).

use std::path::{Path, PathBuf};

/// The processes couldn't be listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

/// Paths open right now: open files and working folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenPaths {
    paths: Vec<PathBuf>,
}

impl OpenPaths {
    /// Nothing open: for an item that belongs to no project.
    pub const NOTHING: Self = Self { paths: Vec::new() };

    #[cfg(test)]
    pub fn from_paths(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    /// Whether `root`, or anything inside it, is open. `root` is compared as given and as
    /// its real path, since the kernel reports real paths (`/private/tmp`, not `/tmp`).
    pub fn touches(&self, root: &Path) -> bool {
        let real = std::fs::canonicalize(root).ok();
        self.paths.iter().any(|open| {
            open.starts_with(root) || real.as_deref().is_some_and(|real| open.starts_with(real))
        })
    }
}

/// Whether `project` is in use now. Without a reading (`None`, the processes couldn't be
/// listed) every project counts as in use, so nothing in one is suggested or moved.
pub fn in_use(open: Option<&OpenPaths>, project: &Path) -> bool {
    open.is_none_or(|open| open.touches(project))
}

/// Reads what is open now.
#[cfg(target_os = "macos")]
pub fn read() -> Result<OpenPaths, Unavailable> {
    let mut paths = Vec::new();
    for pid in all_pids()? {
        if let Some(cwd) = working_folder(pid) {
            paths.push(cwd);
        }
        open_files(pid, &mut paths);
    }
    paths.sort();
    paths.dedup();
    Ok(OpenPaths { paths })
}

#[cfg(not(target_os = "macos"))]
pub fn read() -> Result<OpenPaths, Unavailable> {
    Err(Unavailable)
}

/// `struct proc_fileinfo` from <sys/proc_info.h>; libc doesn't define it.
// Read only as part of the record's layout.
#[allow(dead_code)]
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: i64,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfowithpath` from <sys/proc_info.h>; libc doesn't define it.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileInfo,
    pvip: libc::vnode_info_path,
}

/// `PROC_PIDFDVNODEPATHINFO` from <sys/proc_info.h>; libc doesn't define it.
#[cfg(target_os = "macos")]
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;

#[cfg(target_os = "macos")]
fn all_pids() -> Result<Vec<libc::pid_t>, Unavailable> {
    // Room for processes started between the count and the listing.
    let mut capacity = 512_usize;
    loop {
        let mut pids: Vec<libc::pid_t> = vec![0; capacity];
        let bytes = libc::c_int::try_from(pids.len() * std::mem::size_of::<libc::pid_t>())
            .map_err(|_| Unavailable)?;
        // SAFETY: `pids` is writable storage of exactly `bytes` bytes.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).map_err(|_| Unavailable)?;
        if count == 0 {
            return Err(Unavailable);
        }
        if count < capacity {
            pids.truncate(count);
            pids.retain(|&pid| pid > 0);
            return Ok(pids);
        }
        capacity = capacity.checked_mul(2).ok_or(Unavailable)?;
    }
}

/// The NUL-terminated path in a `vnode_info_path`, if any.
#[cfg(target_os = "macos")]
fn path_of(info: &libc::vnode_info_path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let bytes: Vec<u8> = info
        .vip_path
        .iter()
        .flatten()
        .map(|&c| c.to_ne_bytes()[0])
        .take_while(|&b| b != 0)
        .collect();
    (bytes.first() == Some(&b'/')).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
}

#[cfg(target_os = "macos")]
fn working_folder(pid: libc::pid_t) -> Option<PathBuf> {
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_vnodepathinfo>()).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    // SAFETY: `info` is writable storage of exactly `size` bytes for PROC_PIDVNODEPATHINFO.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: proc_pidinfo filled all `size` bytes.
    let info = unsafe { info.assume_init() };
    path_of(&info.pvi_cdir)
}

#[cfg(target_os = "macos")]
fn open_files(pid: libc::pid_t, paths: &mut Vec<PathBuf>) {
    let entry = std::mem::size_of::<libc::proc_fdinfo>();
    // A first call with no buffer gives the space the descriptor table needs.
    // SAFETY: a null buffer of size 0 asks only for the size.
    let needed =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let Ok(needed) = usize::try_from(needed) else {
        return;
    };
    if needed == 0 {
        return;
    }
    // Room for descriptors opened since.
    let count = needed / entry + 32;
    let mut fds: Vec<libc::proc_fdinfo> = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        count
    ];
    let Ok(bytes) = libc::c_int::try_from(count * entry) else {
        return;
    };
    // SAFETY: `fds` is writable storage of exactly `bytes` bytes of proc_fdinfo.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            bytes,
        )
    };
    let Ok(written) = usize::try_from(written) else {
        return;
    };
    fds.truncate(written / entry);
    let Ok(size) = libc::c_int::try_from(std::mem::size_of::<VnodeFdInfoWithPath>()) else {
        return;
    };
    let vnode = u32::try_from(libc::PROX_FDTYPE_VNODE).unwrap_or(u32::MAX);
    for fd in fds.iter().filter(|fd| fd.proc_fdtype == vnode) {
        let mut info = std::mem::MaybeUninit::<VnodeFdInfoWithPath>::zeroed();
        // SAFETY: `info` is writable storage of exactly `size` bytes for
        // PROC_PIDFDVNODEPATHINFO, whose layout VnodeFdInfoWithPath mirrors (checked in tests).
        let got = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd.proc_fd,
                PROC_PIDFDVNODEPATHINFO,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if got != size {
            continue;
        }
        // SAFETY: proc_pidfdinfo filled all `size` bytes.
        let info = unsafe { info.assume_init() };
        if let Some(path) = path_of(&info.pvip) {
            paths.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_in_use_only_while_something_in_it_is_open() {
        let open = OpenPaths::from_paths(vec![
            PathBuf::from("/Users/me/code/api/src/main.rs"),
            PathBuf::from("/Users/me/code/web"),
        ]);
        assert!(open.touches(Path::new("/Users/me/code/api")));
        assert!(open.touches(Path::new("/Users/me/code/web")));
        assert!(!open.touches(Path::new("/Users/me/code/old-site")));
        // A sibling whose name starts the same isn't inside it.
        assert!(!open.touches(Path::new("/Users/me/code/ap")));
        assert!(!in_use(Some(&open), Path::new("/Users/me/code/old-site")));
    }

    #[test]
    fn without_a_reading_every_project_counts_as_in_use() {
        assert!(in_use(None, Path::new("/Users/me/code/old-site")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_fd_path_record_matches_the_sdk_layout() {
        // sizeof(struct vnode_fdinfowithpath) and offsetof(pvip) in the macOS SDK.
        assert_eq!(std::mem::size_of::<VnodeFdInfoWithPath>(), 1200);
        assert_eq!(std::mem::offset_of!(VnodeFdInfoWithPath, pvip), 24);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_this_test_holds_open_reads_as_in_use() {
        let dir = std::env::temp_dir().join(format!("kleari-open-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        let held = dir.join("held.txt");
        let _file = std::fs::File::create(&held).expect("temporary file");
        let idle =
            std::env::temp_dir().join(format!("kleari-open-paths-idle-{}", std::process::id()));
        std::fs::create_dir_all(&idle).expect("temporary folder");
        let open = read().expect("processes listed");
        assert!(
            open.touches(&dir),
            "an open file inside makes the folder in use"
        );
        assert!(
            !open.touches(&idle),
            "a folder nothing has open isn't in use"
        );
        drop(_file);
        std::fs::remove_dir_all(&dir).expect("clean up");
        std::fs::remove_dir_all(&idle).expect("clean up");
    }
}
