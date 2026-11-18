//! The Maintenance page (ADR-020): tasks macOS can do but hides, each a fixed AppleScript run
//! by `/usr/bin/osascript` when you choose it. Tasks that need administrator rights say
//! `with administrator privileges`, so macOS asks for your password each time; Kleari never
//! sees it, installs no helper and keeps no rights afterwards. No command is built from data:
//! every script is a constant here.
//!
//! What a task would clear is measured first, read-only, so the page can show it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::developer::{measure_dir, open_absolute, Links};

const OSASCRIPT: &str = "/usr/bin/osascript";
/// The longest a task may run (thinning snapshots or reindexing can take minutes).
const TIME_LIMIT: Duration = Duration::from_secs(15 * 60);
const MAX_OUTPUT: u64 = 64 * 1024;
/// Rotated system logs and crash reports older than this are cleared.
pub const LOG_AGE_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Task {
    /// Purges inactive memory (`purge`).
    FreeRam,
    /// Clears the DNS cache.
    FlushDns,
    /// Asks Time Machine to thin local snapshots, which hold purgeable space.
    ThinSnapshots,
    /// Rebuilds the Spotlight index of the startup disk.
    ReindexSpotlight,
    /// Deletes other apps' caches in `/Library/Caches` (never Apple's).
    ClearSystemCaches,
    /// Deletes rotated system logs and crash reports older than 30 days.
    ClearSystemLogs,
    /// Deletes `/Users/Deleted Users`, the disk images macOS keeps of removed accounts.
    RemoveDeletedUsers,
    /// Asks Finder to empty the Trash, on every disk.
    EmptyTrash,
}

impl Task {
    pub const ALL: [Self; 8] = [
        Self::FreeRam,
        Self::FlushDns,
        Self::ThinSnapshots,
        Self::ReindexSpotlight,
        Self::ClearSystemCaches,
        Self::ClearSystemLogs,
        Self::RemoveDeletedUsers,
        Self::EmptyTrash,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FreeRam => "freeRam",
            Self::FlushDns => "flushDns",
            Self::ThinSnapshots => "thinSnapshots",
            Self::ReindexSpotlight => "reindexSpotlight",
            Self::ClearSystemCaches => "clearSystemCaches",
            Self::ClearSystemLogs => "clearSystemLogs",
            Self::RemoveDeletedUsers => "removeDeletedUsers",
            Self::EmptyTrash => "emptyTrash",
        }
    }

    /// It deletes something for good (the page asks for a separate confirmation).
    pub const fn deletes(self) -> bool {
        matches!(
            self,
            Self::ClearSystemCaches
                | Self::ClearSystemLogs
                | Self::RemoveDeletedUsers
                | Self::EmptyTrash
        )
    }

    /// macOS asks for an administrator's password.
    pub const fn needs_admin(self) -> bool {
        !matches!(self, Self::EmptyTrash)
    }

    /// The AppleScript run for the task: constants only.
    pub const fn script(self) -> &'static str {
        match self {
            Self::FreeRam => r#"do shell script "/usr/sbin/purge" with administrator privileges"#,
            Self::FlushDns => {
                r#"do shell script "/usr/bin/dscacheutil -flushcache; /usr/bin/killall -HUP mDNSResponder" with administrator privileges"#
            }
            Self::ThinSnapshots => {
                r#"do shell script "/usr/bin/tmutil thinlocalsnapshots / 999999999999 4" with administrator privileges"#
            }
            Self::ReindexSpotlight => {
                r#"do shell script "/usr/bin/mdutil -E /" with administrator privileges"#
            }
            Self::ClearSystemCaches => {
                r#"do shell script "/usr/bin/find -x /Library/Caches -mindepth 1 -maxdepth 1 ! -name 'com.apple.*' -exec /bin/rm -rf {} +" with administrator privileges"#
            }
            Self::ClearSystemLogs => {
                r#"do shell script "/usr/bin/find -x /private/var/log -type f -name '*.gz' -mtime +30 -delete; /usr/bin/find -x /private/var/log -type f -name '*.bz2' -mtime +30 -delete; /usr/bin/find -x /Library/Logs/DiagnosticReports -type f -mtime +30 -delete" with administrator privileges"#
            }
            Self::RemoveDeletedUsers => {
                r#"do shell script "/bin/rm -rf '/Users/Deleted Users'" with administrator privileges"#
            }
            Self::EmptyTrash => r#"tell application "Finder" to empty trash"#,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Done,
    /// You cancelled macOS's password or permission prompt.
    Cancelled,
    /// It ran but didn't succeed, or took too long.
    Failed,
}

/// Runs the task's script. Blocks until it ends or `TIME_LIMIT` passes.
pub fn run(task: Task) -> Outcome {
    let Ok(mut child) = Command::new(OSASCRIPT)
        .args(["-e", task.script()])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Outcome::Failed;
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Outcome::Failed;
    };
    // Read on another thread so a full pipe can't stall osascript while this one waits.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.take(MAX_OUTPUT).read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < TIME_LIMIT => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let errors = reader.join().unwrap_or_default();
    match status {
        Some(status) if status.success() => Outcome::Done,
        // AppleScript's "User canceled." error.
        Some(_) if errors.contains("(-128)") => Outcome::Cancelled,
        _ => Outcome::Failed,
    }
}

/// What a task would clear or affect, measured read-only beforehand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub task: Task,
    /// Space it would free, when it can be measured.
    pub bytes: Option<u64>,
    /// Something couldn't be read, so the size is a minimum.
    pub lower_bound: bool,
    /// How many things it concerns (local snapshots, deleted accounts, trash folders).
    pub count: Option<u64>,
    /// It deletes something for good.
    pub deletes: bool,
    /// macOS asks for an administrator's password.
    pub needs_admin: bool,
}

/// Measures what each task concerns. `uid` finds external disks' Trash folders.
pub fn info(home: &Path, uid: u32, cancel: &AtomicBool) -> Vec<TaskInfo> {
    let mut links = Links::default();
    let mut measure = |path: &Path, skip_apple: bool| -> (u64, bool, u64) {
        let Some((mut dir, dev)) = open_absolute(path) else {
            return (0, true, 0);
        };
        let names: Vec<_> = std::iter::from_fn(|| dir.next_name())
            .filter_map(Result::ok)
            .collect();
        let (mut bytes, mut lower, mut count) = (0_u64, false, 0_u64);
        for name in names {
            if skip_apple && name.to_bytes().starts_with(b"com.apple.") {
                continue;
            }
            let Ok(stat) = dir.stat_child(&name) else {
                lower = true;
                continue;
            };
            if stat.is_symlink() || stat.dev != dev {
                continue;
            }
            count += 1;
            if stat.is_dir() {
                match dir.open_child(&name) {
                    Ok((child, _)) => {
                        let measured = measure_dir(child, dev, &mut links, cancel);
                        bytes = bytes.saturating_add(measured.bytes);
                        lower |= measured.lower_bound;
                    }
                    Err(_) => lower = true,
                }
            } else {
                bytes = bytes.saturating_add(stat.blocks.saturating_mul(512));
            }
        }
        (bytes, lower, count)
    };
    let (trash_bytes, trash_lower, mut trash_count) = measure(&home.join(".Trash"), false);
    let mut trash = (trash_bytes, trash_lower);
    for volume in external_volumes() {
        let (bytes, lower, count) = measure(&volume.join(".Trashes").join(uid.to_string()), false);
        trash = (trash.0.saturating_add(bytes), trash.1 || lower);
        trash_count += count;
    }
    let caches = measure(Path::new("/Library/Caches"), true);
    let deleted = measure(Path::new("/Users/Deleted Users"), false);
    let logs = old_logs(cancel);
    Task::ALL
        .iter()
        .map(|&task| {
            let (bytes, lower_bound, count) = match task {
                Task::EmptyTrash => (Some(trash.0), trash.1, Some(trash_count)),
                Task::ClearSystemCaches => (Some(caches.0), caches.1, Some(caches.2)),
                Task::RemoveDeletedUsers => (Some(deleted.0), deleted.1, Some(deleted.2)),
                Task::ClearSystemLogs => (Some(logs.0), logs.1, Some(logs.2)),
                Task::ThinSnapshots => (None, false, local_snapshots()),
                _ => (None, false, None),
            };
            TaskInfo {
                task,
                bytes,
                lower_bound,
                count,
                deletes: task.deletes(),
                needs_admin: task.needs_admin(),
            }
        })
        .collect()
}

/// Local disks mounted in `/Volumes` (not network shares), for their Trash folders.
pub(crate) fn external_volumes() -> Vec<PathBuf> {
    let Some(non_local) = crate::system_monitor::non_local_mounts() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir("/Volumes") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| !non_local.iter().any(|mount| path.starts_with(mount)))
        .filter(|path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()))
        .collect()
}

/// Rotated system logs and crash reports older than `LOG_AGE_DAYS`: bytes, minimum, count.
fn old_logs(cancel: &AtomicBool) -> (u64, bool, u64) {
    let now = std::time::SystemTime::now();
    let old = |meta: &std::fs::Metadata| {
        meta.modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() / 86_400 > LOG_AGE_DAYS)
    };
    let mut total = (0_u64, false, 0_u64);
    let mut visit = |folder: &Path, matches: &dyn Fn(&Path) -> bool| {
        let mut stack = vec![folder.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                total.1 = true;
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let Ok(meta) = entry.path().symlink_metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(entry.path());
                } else if meta.is_file() && matches(&entry.path()) && old(&meta) {
                    use std::os::unix::fs::MetadataExt;
                    total.0 = total.0.saturating_add(meta.blocks().saturating_mul(512));
                    total.2 += 1;
                }
            }
        }
    };
    visit(Path::new("/private/var/log"), &|path| {
        path.extension()
            .is_some_and(|extension| extension == "gz" || extension == "bz2")
    });
    visit(Path::new("/Library/Logs/DiagnosticReports"), &|_| true);
    total
}

/// How many local Time Machine snapshots the startup disk holds (`tmutil listlocalsnapshots /`,
/// fixed arguments, no privileges). `None` if it can't be read.
fn local_snapshots() -> Option<u64> {
    let output = Command::new("/usr/bin/tmutil")
        .args(["listlocalsnapshots", "/"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        u64::try_from(
            text.lines()
                .filter(|line| line.contains("com.apple.TimeMachine."))
                .count(),
        )
        .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_constants_with_admin_only_where_needed_and_never_touch_apples_caches() {
        for task in Task::ALL {
            let script = task.script();
            assert_eq!(
                script.contains("with administrator privileges"),
                task.needs_admin(),
                "{task:?}"
            );
            // Absolute tool paths only, so nothing on PATH can stand in.
            if script.starts_with("do shell script") {
                for command in script
                    .trim_start_matches("do shell script \"")
                    .split(" with administrator privileges")
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('"')
                    .split("; ")
                {
                    assert!(command.starts_with('/'), "{task:?}: {command}");
                }
            }
        }
        assert!(Task::ClearSystemCaches
            .script()
            .contains("! -name 'com.apple.*'"));
        assert!(Task::ClearSystemCaches.script().contains("-maxdepth 1"));
        assert!(Task::ClearSystemLogs.script().contains("-mtime +30"));
        assert!(!Task::FreeRam.deletes() && Task::EmptyTrash.deletes());
    }

    #[test]
    fn tasks_serialize_to_stable_names() {
        for task in Task::ALL {
            assert_eq!(
                serde_json::to_string(&task).expect("json"),
                format!("\"{}\"", task.as_str())
            );
        }
    }
}
