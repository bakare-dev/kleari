//! Startup and background items Kleari can read without privileges: launch agents and
//! daemons installed in the standard LaunchAgents and LaunchDaemons folders. macOS's
//! Login Items list (Background Task Management) needs administrator access and is not read;
//! the UI points to System Settings for it. Read-only: each plist is read, never changed.
//!
//! Privacy: only a display name, the launchd label, the scope and two statuses leave this
//! module.
//! Program paths are used to check that the program still exists and are then dropped.
//!
//! Offline: the existence check never touches a network volume. Paths on mounts macOS doesn't
//! report as local are not checked, and the walk to the program never follows a symlink, so it
//! can't be redirected onto a share either.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;

use serde::Serialize;

use super::processes::app_name_from_exe;
use crate::storage::fsdir::DirHandle;

/// Plists larger than this are skipped as unreadable; real launchd plists are a few KB.
const MAX_PLIST_BYTES: u64 = 256 * 1024;
/// Items read per folder; the result says when a folder held more.
const MAX_ITEMS_PER_FOLDER: usize = 500;
const MAX_LABEL_CHARS: usize = 200;

/// What Kleari could establish about the program a plist starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgramStatus {
    /// The program file exists at the absolute path the plist names.
    Present,
    /// The path's folders exist but the program does not: usually left by a removed app.
    Missing,
    /// Not checked: no program, a relative path, a symlink on the way, a network or unknown
    /// volume, or an error such as no permission.
    NotChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupScope {
    /// ~/Library/LaunchAgents: runs when you log in.
    UserAgent,
    /// /Library/LaunchAgents: runs when any user logs in.
    AllUsersAgent,
    /// /Library/LaunchDaemons: runs at startup, outside any user session.
    SystemDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    /// The app the program belongs to when it lives inside an app bundle, else the label.
    pub name: String,
    pub label: String,
    pub scope: StartupScope,
    pub program: ProgramStatus,
    /// The plist itself sets `Disabled`. Overrides made in System Settings are not visible
    /// without administrator access, so `false` does not prove the item is allowed.
    pub disabled_in_plist: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItems {
    pub items: Vec<StartupItem>,
    /// Plists that could not be read or parsed (permissions, size, malformed, symlinked).
    pub unreadable_count: u64,
    /// Folders that exist but could not be listed (permissions or I/O); a missing folder is
    /// not counted, because it simply has no items.
    pub unreadable_folder_count: u64,
    /// A folder held more than `MAX_ITEMS_PER_FOLDER` plists; the rest were not read.
    pub truncated: bool,
}

/// Mount points of volumes macOS doesn't report as local (network shares, automounts). `None`
/// when the mount table can't be read, in which case no program is checked. Also used by the
/// Processes page to resolve app bundles without touching network volumes.
#[cfg(target_os = "macos")]
pub(crate) fn non_local_mounts() -> Option<Vec<PathBuf>> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    // getmntinfo fills one buffer shared by the whole process, so a call on another thread
    // could rewrite it while this one reads it. Scans and page reads run on several threads;
    // only one reads the table at a time.
    static MOUNT_TABLE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // The guard protects no data, so a poisoned lock is still safe to take.
    let _reading = MOUNT_TABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut mounts: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: MNT_NOWAIT returns the kernel's cached table without asking any file system
    // (so no network I/O); on success `mounts` points to `count` statfs records in libc's
    // buffer, which no other call can change while the lock is held, and they are copied out
    // before returning.
    let count = unsafe { libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT) };
    if count <= 0 || mounts.is_null() {
        return None;
    }
    // SAFETY: getmntinfo reported `count` valid, initialized records at `mounts`.
    let records = unsafe { std::slice::from_raw_parts(mounts, usize::try_from(count).ok()?) };
    Some(
        records
            .iter()
            .filter(|record| {
                // SAFETY: both fields are NUL-terminated C strings inside the fixed-size arrays.
                let kind = unsafe { CStr::from_ptr(record.f_fstypename.as_ptr()) };
                // Mask with the u32 flag field's type; the constant is a small positive value.
                let local = record.f_flags & (libc::MNT_LOCAL as u32) != 0;
                !local || kind.to_bytes() == b"autofs"
            })
            .map(|record| {
                // SAFETY: as above.
                let on = unsafe { CStr::from_ptr(record.f_mntonname.as_ptr()) };
                PathBuf::from(std::ffi::OsStr::from_bytes(on.to_bytes()))
            })
            .collect(),
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn non_local_mounts() -> Option<Vec<PathBuf>> {
    // Other targets (Linux CI) have no network shares under test; the walk below still never
    // follows symlinks.
    Some(Vec::new())
}

/// Checks whether `program` exists without following symlinks or entering a non-local mount.
pub fn program_status(program: Option<&Path>, non_local: Option<&[PathBuf]>) -> ProgramStatus {
    let (Some(program), Some(non_local)) = (program, non_local) else {
        return ProgramStatus::NotChecked;
    };
    if !program.is_absolute()
        || program
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || non_local
            .iter()
            .any(|mount| mount != Path::new("/") && program.starts_with(mount))
    {
        return ProgramStatus::NotChecked;
    }
    // Walk with directory handles: each folder is opened *relative to the already-open parent*
    // with O_NOFOLLOW | O_DIRECTORY (storage's race-free fsdir), and the program is lstat'ed
    // relative to its folder. A folder swapped for a symlink mid-walk fails to open (ELOOP or
    // ENOTDIR) instead of being followed, so no path is ever re-resolved by name.
    let Ok(names) = program
        .components()
        .skip(1)
        .map(|part| CString::new(part.as_os_str().as_bytes()))
        .collect::<Result<Vec<CString>, _>>()
    else {
        return ProgramStatus::NotChecked;
    };
    let Some((file, folders)) = names.split_last() else {
        return ProgramStatus::NotChecked;
    };
    let Ok((mut directory, _)) = DirHandle::open_root(Path::new("/")) else {
        return ProgramStatus::NotChecked;
    };
    for folder in folders {
        directory = match directory.open_child(folder) {
            Ok((child, _)) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return ProgramStatus::Missing,
            // ELOOP (a symlink), ENOTDIR (not a folder), permissions: not proof either way.
            Err(_) => return ProgramStatus::NotChecked,
        };
    }
    match directory.stat_child(file) {
        Ok(stat) if stat.is_symlink() || stat.is_dir() => ProgramStatus::NotChecked,
        Ok(_) => ProgramStatus::Present,
        Err(error) if error.kind() == io::ErrorKind::NotFound => ProgramStatus::Missing,
        Err(_) => ProgramStatus::NotChecked,
    }
}

pub fn read() -> StartupItems {
    let mut folders: Vec<(PathBuf, StartupScope)> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        folders.push((
            Path::new(&home).join("Library/LaunchAgents"),
            StartupScope::UserAgent,
        ));
    }
    folders.push((
        PathBuf::from("/Library/LaunchAgents"),
        StartupScope::AllUsersAgent,
    ));
    folders.push((
        PathBuf::from("/Library/LaunchDaemons"),
        StartupScope::SystemDaemon,
    ));
    read_folders(&folders, non_local_mounts().as_deref())
}

pub fn read_folders(
    folders: &[(PathBuf, StartupScope)],
    non_local: Option<&[PathBuf]>,
) -> StartupItems {
    let mut result = StartupItems {
        items: Vec::new(),
        unreadable_count: 0,
        unreadable_folder_count: 0,
        truncated: false,
    };
    for (folder, scope) in folders {
        read_folder(folder, *scope, non_local, &mut result);
    }
    result.items.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    result
}

fn read_folder(
    folder: &Path,
    scope: StartupScope,
    non_local: Option<&[PathBuf]>,
    result: &mut StartupItems,
) {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        // A missing folder (common for ~/Library/LaunchAgents) simply has no items.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            result.unreadable_folder_count += 1;
            return;
        }
    };
    let mut read = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            result.unreadable_count += 1;
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("plist") {
            continue;
        }
        if read == MAX_ITEMS_PER_FOLDER {
            result.truncated = true;
            break;
        }
        read += 1;
        match read_item(&path, scope, non_local) {
            Some(item) => result.items.push(item),
            None => result.unreadable_count += 1,
        }
    }
}

fn read_item(
    path: &Path,
    scope: StartupScope,
    non_local: Option<&[PathBuf]>,
) -> Option<StartupItem> {
    // O_NOFOLLOW refuses a symlink at open time (no check-then-use gap), and O_NONBLOCK keeps a
    // FIFO from blocking the open. The opened handle is then checked to be a regular file.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLIST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PLIST_BYTES).read_to_end(&mut bytes).ok()?;
    parse_item(&bytes, scope, non_local)
}

/// Parses one launchd plist (XML or binary). Pure apart from checking whether the program
/// exists.
pub fn parse_item(
    bytes: &[u8],
    scope: StartupScope,
    non_local: Option<&[PathBuf]>,
) -> Option<StartupItem> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    let dictionary = value.as_dictionary()?;
    let label: String = dictionary
        .get("Label")?
        .as_string()?
        .trim()
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect();
    if label.is_empty() {
        return None;
    }
    let program = dictionary
        .get("Program")
        .and_then(plist::Value::as_string)
        .or_else(|| {
            dictionary
                .get("ProgramArguments")
                .and_then(plist::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(plist::Value::as_string)
        })
        .map(Path::new);
    let name = program
        .and_then(app_name_from_exe)
        .unwrap_or_else(|| label.clone());
    let program_status = program_status(program, non_local);
    let disabled_in_plist = dictionary
        .get("Disabled")
        .and_then(plist::Value::as_boolean)
        .unwrap_or(false);
    Some(StartupItem {
        name,
        label,
        scope,
        program: program_status,
        disabled_in_plist,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    /// A disposable folder, removed on drop.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "kleari-startup-test-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create temp folder");
            // Canonical, because macOS's temp folder sits behind the /var symlink, which the
            // program check (correctly) refuses to follow.
            Self(fs::canonicalize(&path).expect("canonical temp folder"))
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).expect("write plist");
            path
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn plist(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>{body}</dict></plist>"#
        )
    }

    const LOCAL: Option<&[PathBuf]> = Some(&[]);

    #[test]
    fn programs_are_not_checked_on_network_mounts_through_symlinks_or_when_relative() {
        let folder = TempFolder::new();
        let real = folder.write("real-tool", "");
        let link = folder.0.join("linked-dir");
        std::os::unix::fs::symlink(&folder.0, &link).expect("symlink");

        // Under a non-local mount: never touched, even though the file exists.
        let mounts = [folder.0.clone()];
        assert_eq!(
            program_status(Some(&real), Some(&mounts)),
            ProgramStatus::NotChecked
        );
        // A symlink anywhere on the way stops the check rather than following it.
        assert_eq!(
            program_status(Some(&link.join("real-tool")), LOCAL),
            ProgramStatus::NotChecked
        );
        // A program that is itself a symlink is not followed either.
        let program_link = folder.0.join("program-link");
        std::os::unix::fs::symlink(&real, &program_link).expect("symlink");
        assert_eq!(
            program_status(Some(&program_link), LOCAL),
            ProgramStatus::NotChecked
        );
        for path in ["bin/tool", "/tmp/../etc/passwd"] {
            assert_eq!(
                program_status(Some(Path::new(path)), LOCAL),
                ProgramStatus::NotChecked,
                "{path}"
            );
        }
        // No program, or an unreadable mount table, is not a guess either way.
        assert_eq!(program_status(None, LOCAL), ProgramStatus::NotChecked);
        assert_eq!(program_status(Some(&real), None), ProgramStatus::NotChecked);
        // A file where a folder should be can't be opened as a folder (ENOTDIR): not checked.
        assert_eq!(
            program_status(Some(&real.join("inside")), LOCAL),
            ProgramStatus::NotChecked
        );
        // A directory is not a program.
        assert_eq!(
            program_status(Some(&folder.0), LOCAL),
            ProgramStatus::NotChecked
        );
        assert_eq!(program_status(Some(&real), LOCAL), ProgramStatus::Present);
    }

    #[test]
    fn symlinked_plists_are_not_followed() {
        let folder = TempFolder::new();
        let target = folder.write(
            "real.data",
            &plist("<key>Label</key><string>com.example.linked</string>"),
        );
        std::os::unix::fs::symlink(&target, folder.0.join("linked.plist")).expect("symlink");
        let result = read_folders(&[(folder.0.clone(), StartupScope::UserAgent)], LOCAL);
        assert!(result.items.is_empty());
        assert_eq!(result.unreadable_count, 1);
    }

    #[test]
    fn serializes_statuses_without_paths() {
        let item = parse_item(
            plist("<key>Label</key><string>com.example.helper</string><key>Program</key><string>/Applications/Example.app/Contents/MacOS/helper</string>").as_bytes(),
            StartupScope::UserAgent,
            None,
        )
        .expect("parses");
        let json = serde_json::to_string(&item).expect("serializes");
        assert!(!json.contains("/Applications"));
        assert!(json.contains(r#""scope":"userAgent""#));
        assert!(json.contains(r#""program":"notChecked""#));
    }
}
