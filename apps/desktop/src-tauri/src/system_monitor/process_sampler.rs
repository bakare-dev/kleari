//! OS adapter for the Processes page: reads every process through `sysinfo` (name, owner,
//! executable, CPU time, memory), memory and CPU time that macOS withholds from Kleari
//! through Apple's `/bin/ps` (ADR-010, macOS only), thread counts and identities through
//! `proc_pidinfo`, and account names through `getpwuid_r`. No shell, no privileges of
//! Kleari's own. Executable paths are kept in memory only, to show where a process runs
//! from; they are never stored or logged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind,
};

use super::process_list::{build_rows, Context, ListedProcess, ProcessList, RunningApp};
use super::processes::{baseline_of, CpuBaseline, RawProcess};
#[cfg(target_os = "macos")]
use super::ps;
use super::running_apps::process_identity;
use super::startup::non_local_mounts;
use super::unix_time_ms;

/// An app the latest list offered to quit, kept in the backend. `quit_app` accepts only these,
/// so the frontend can't name an app it wasn't shown, and never sends a path.
#[derive(Debug, Clone, PartialEq, Eq)]
// The bundle is checked by the macOS quit path; on other targets (Linux CI) only tests read it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct QuitTarget {
    pub pid: u32,
    /// The app's resolved bundle as listed.
    pub bundle: PathBuf,
}

/// Symlinks followed while resolving one path before giving up.
const MAX_SYMLINK_HOPS: usize = 40;

/// Resolves `path` through symlinks (`/Applications/Safari.app` links into
/// `/System/Cryptexes`) without touching anything at or below a mount in `non_local` (network
/// shares, automounts), so a refresh doesn't contact or stall on another computer through a
/// known mount. Such a path gives `None`, as does anything that can't be resolved
/// unambiguously. A volume mounted after `non_local` was read could still be looked up once
/// (ADR-009, residual mount race).
///
/// The walk uses `storage::fsdir` handles: every folder is opened relative to its open parent
/// with no-follow and identity checks, children are inspected with `fstatat(NOFOLLOW)` and
/// symlinks read with `readlinkat`, all through the open parent. Nothing is looked up by name
/// from the root, so a parent swapped for a symlink mid-walk can't redirect it, and the path
/// checked against the mounts is always the one actually reached.
pub(super) fn resolve_local(path: &Path, non_local: &[PathBuf]) -> Option<PathBuf> {
    use std::collections::VecDeque;
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;

    use crate::storage::fsdir::DirHandle;

    fn parts(path: &Path) -> Vec<OsString> {
        path.components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.to_owned()),
                Component::ParentDir => Some(OsString::from("..")),
                Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
            })
            .collect()
    }

    if !path.is_absolute() {
        return None;
    }
    // The open folders from the root down, with the real path of the deepest one.
    let (root, _) = DirHandle::open_root(Path::new("/")).ok()?;
    let mut open = vec![root];
    let mut resolved = PathBuf::from("/");
    let mut pending: VecDeque<OsString> = parts(path).into();
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        if part == ".." {
            if open.len() > 1 {
                open.pop();
                resolved.pop();
            }
            continue;
        }
        let next = resolved.join(&part);
        if non_local.iter().any(|mount| next.starts_with(mount)) {
            return None;
        }
        let name = CString::new(part.as_bytes()).ok()?;
        let parent = open.last()?;
        let stat = parent.stat_child(&name).ok()?;
        if stat.is_symlink() {
            hops += 1;
            if hops > MAX_SYMLINK_HOPS {
                return None;
            }
            let target = parent.read_link_child(&name).ok()?;
            if target.is_absolute() {
                open.truncate(1);
                resolved = PathBuf::from("/");
            }
            for part in parts(&target).into_iter().rev() {
                pending.push_front(part);
            }
            continue;
        }
        if !stat.is_dir() {
            // Only the last component may be something other than a folder.
            if pending.is_empty() {
                return Some(next);
            }
            return None;
        }
        let (child, opened) = parent.open_child(&name).ok()?;
        // The folder opened must be the one just checked; a swap in between fails closed.
        if !opened.is_dir() || opened.dev != stat.dev || opened.ino != stat.ino {
            return None;
        }
        open.push(child);
        resolved = next;
    }
    Some(resolved)
}

/// Resolves an app bundle for the quit checks, reading the mount table first. `None` if the
/// table can't be read, so nothing is offered when Kleari can't tell what is local.
// Only the macOS quit path calls it; on other targets (Linux CI) it is unused.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn resolve_bundle(path: &Path) -> Option<PathBuf> {
    resolve_local(path, &non_local_mounts()?)
}

/// A longer gap between samples (first visit, pause, hidden window) withholds CPU, so a
/// long-period average is never shown as current. The page polls every 2 s.
const MAX_LIVE_WINDOW: Duration = Duration::from_secs(6);

pub struct ProcessListSampler {
    /// `None` where sysinfo can't read this system (it supports macOS, and Linux for CI):
    /// there is then no list (`sample` returns `None`).
    system: Option<System>,
    baseline: CpuBaseline,
    last: Option<Instant>,
    users: HashMap<u32, Option<String>>,
    /// Quit tokens issued by the latest sample. Replaced every sample.
    quit_targets: HashMap<u64, QuitTarget>,
}

impl ProcessListSampler {
    pub fn new() -> Self {
        Self {
            system: System::new().ok(),
            baseline: CpuBaseline::new(),
            last: None,
            users: HashMap::new(),
            quit_targets: HashMap::new(),
        }
    }

    /// The app the latest list offered under `token`, if it was for `pid`.
    pub fn quit_target(&self, pid: u32, token: u64) -> Option<QuitTarget> {
        self.quit_targets
            .get(&token)
            .filter(|target| target.pid == pid)
            .cloned()
    }

    /// Samples every process; `apps` are this account's regular apps, read on the main
    /// thread by the caller. `None` where sysinfo can't read this system.
    pub fn sample(
        &mut self,
        apps: &[RunningApp],
        home: Option<&std::path::Path>,
    ) -> Option<ProcessList> {
        let system = self.system.as_mut()?;
        system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_user(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        let now = Instant::now();
        let window = self
            .last
            .map(|last| now.duration_since(last))
            .filter(|window| !window.is_zero() && *window <= MAX_LIVE_WINDOW);
        self.last = Some(now);
        let window_ms = window.map(|window| u64::try_from(window.as_millis()).unwrap_or(u64::MAX));

        // Only the macOS `ps` fill below changes it.
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut processes: Vec<ListedProcess> = system
            .processes()
            .values()
            .map(|process| ListedProcess {
                raw: RawProcess {
                    pid: process.pid().as_u32(),
                    name: process.name().to_string_lossy().into_owned(),
                    start_time: process.start_time(),
                    cpu_time_ms: process.accumulated_cpu_time(),
                    memory: process.memory(),
                    app: None,
                },
                uid: process.user_id().map(|uid| **uid),
                exe: process.exe().map(PathBuf::from),
            })
            .collect();
        // macOS withholds memory and CPU time of root's and other accounts' processes from
        // unprivileged apps (reported as 0). Apple's setuid `ps` fills them in (ADR-010).
        // Other targets keep their own readings.
        #[cfg(target_os = "macos")]
        if processes.iter().any(|process| process.raw.memory == 0) {
            if let Some(readings) = ps::read() {
                for process in processes
                    .iter_mut()
                    .filter(|process| process.raw.memory == 0)
                {
                    if let Some(reading) = readings.get(&process.raw.pid) {
                        process.raw.memory = reading.memory_bytes;
                        process.raw.cpu_time_ms = reading.cpu_time_ms;
                        process.uid = process.uid.or(Some(reading.uid));
                    }
                }
            }
        }
        for uid in processes.iter().filter_map(|process| process.uid) {
            self.users.entry(uid).or_insert_with(|| account_name(uid));
        }
        let users = &self.users;
        let user_name = |uid: u32| users.get(&uid).cloned().flatten();
        // Read once per sample, without file-system I/O; unreadable means nothing resolves.
        let non_local = non_local_mounts();
        let resolve = |path: &Path| resolve_local(path, non_local.as_deref()?);
        let context = Context {
            own_pid: std::process::id(),
            // SAFETY: getuid has no preconditions and cannot fail.
            own_uid: unsafe { libc::getuid() },
            home,
            logical_cpus: system.cpus().len(),
            user_name: &user_name,
            threads: &thread_count,
            identity: &process_identity,
            canonical: &resolve,
        };
        let previous = window_ms.map(|window_ms| (&self.baseline, window_ms));
        let (rows, unreadable_count) = build_rows(&processes, apps, previous, &context);
        self.quit_targets = rows
            .iter()
            .filter_map(|row| {
                Some((
                    row.quit_token?,
                    QuitTarget {
                        pid: row.pid,
                        bundle: row.quit_bundle.clone()?,
                    },
                ))
            })
            .collect();
        let raw: Vec<RawProcess> = processes.into_iter().map(|process| process.raw).collect();
        self.baseline = baseline_of(&raw);

        let usage = system.global_cpu_usage();
        Some(ProcessList {
            sampled_at_ms: unix_time_ms(),
            window_ms,
            logical_cpus: u32::try_from(context.logical_cpus).unwrap_or(u32::MAX),
            total_cpu_percent: window_ms.and(
                usage
                    .is_finite()
                    .then(|| f64::from(usage).clamp(0.0, 100.0)),
            ),
            memory_total_bytes: system.total_memory(),
            memory_used_bytes: system.used_memory().min(system.total_memory()),
            total_count: u32::try_from(raw.len()).unwrap_or(u32::MAX),
            unreadable_count,
            rows,
        })
    }
}

impl Default for ProcessListSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// The account name for `uid`, or `None` if it has none.
fn account_name(uid: u32) -> Option<String> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer refers to writable storage of the stated size that outlives the
    // call; on success `result` points to `entry`, whose strings live in `buffer`.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    // SAFETY: `result` is non-null, so `entry` is initialized and `pw_name` is a
    // NUL-terminated string inside `buffer`, which is still alive.
    let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) };
    let name = name.to_string_lossy().trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Threads in `pid`; macOS refuses this for other accounts' processes, which gives `None`.
#[cfg(target_os = "macos")]
fn thread_count(pid: u32) -> Option<u32> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    // SAFETY: `info` is writable storage of exactly `size` bytes for PROC_PIDTASKINFO.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
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
    u32::try_from(info.pti_threadnum)
        .ok()
        .filter(|&threads| threads > 0)
}

#[cfg(not(target_os = "macos"))]
fn thread_count(_pid: u32) -> Option<u32> {
    None
}
