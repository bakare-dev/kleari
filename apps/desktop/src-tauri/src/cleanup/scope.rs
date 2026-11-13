//! The folders a Cleanup scan reads and whether Kleari may read them.
//!
//! macOS protects Downloads, Desktop, Documents and Movies: the first read asks the user in a
//! system prompt. Opening a page must never prompt, so these folders are only read after the
//! user chooses Grant access, and the answer is remembered in the history. A folder added
//! through the macOS folder panel is granted by that choice. Everything else the scan reads
//! is outside those protections (though macOS may still refuse some items without Full Disk
//! Access, which the scan reports).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ScanMode;
use crate::persistence::{AccessState, Database};
use crate::storage::fsdir::DirHandle;
use crate::system_monitor::display_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtectedFolder {
    Downloads,
    Desktop,
    Documents,
    Movies,
}

impl ProtectedFolder {
    pub const ALL: [Self; 4] = [
        Self::Downloads,
        Self::Desktop,
        Self::Documents,
        Self::Movies,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Downloads => "Downloads",
            Self::Desktop => "Desktop",
            Self::Documents => "Documents",
            Self::Movies => "Movies",
        }
    }

    /// The key its access is remembered under.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Downloads => "downloads",
            Self::Desktop => "desktop",
            Self::Documents => "documents",
            Self::Movies => "movies",
        }
    }

    pub fn path(self, home: &Path) -> PathBuf {
        home.join(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// Not protected, and it exists.
    Available,
    /// It doesn't exist on this Mac.
    Missing,
    /// You granted access (or picked the folder in the macOS panel).
    Granted,
    /// Full Disk Access is on, which covers it without a prompt.
    #[serde(rename = "fullDiskAccess")]
    ByFullDisk,
    /// Protected, and Kleari hasn't asked yet.
    NotRequested,
    /// You declined in the macOS prompt, or turned it off in System Settings.
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderId {
    Caches,
    Logs,
    Developer,
    Projects,
    Downloads,
    Desktop,
    Documents,
    Movies,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeFolder {
    pub id: FolderId,
    /// Shown as `~/…`.
    pub path: String,
    pub access: Access,
    /// The scan modes that read it.
    pub modes: Vec<ScanMode>,
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn fixed(id: FolderId, path: PathBuf, home: &Path, modes: Vec<ScanMode>) -> ScopeFolder {
    ScopeFolder {
        id,
        access: if exists(&path) {
            Access::Available
        } else {
            Access::Missing
        },
        path: display_path(&path, Some(home)),
        modes,
    }
}

fn protected_id(folder: ProtectedFolder) -> FolderId {
    match folder {
        ProtectedFolder::Downloads => FolderId::Downloads,
        ProtectedFolder::Desktop => FolderId::Desktop,
        ProtectedFolder::Documents => FolderId::Documents,
        ProtectedFolder::Movies => FolderId::Movies,
    }
}

/// Access to a protected folder: Full Disk Access covers all of them without a prompt;
/// otherwise the answer remembered from Grant access. An unreadable history counts as not
/// asked: the folder isn't read.
pub fn access_of(db: &Database, folder: ProtectedFolder, full_disk_access: bool) -> Access {
    if full_disk_access {
        return Access::ByFullDisk;
    }
    match db.folder_access(folder.key()) {
        Ok(Some(AccessState::Granted)) => Access::Granted,
        Ok(Some(AccessState::Denied)) => Access::Denied,
        Ok(None) | Err(_) => Access::NotRequested,
    }
}

/// Whether Full Disk Access is on (proven by opening a folder only it unlocks).
pub fn has_full_disk_access(home: &Path) -> bool {
    crate::storage::volume::full_disk_access(Some(home))
        == crate::storage::volume::AccessStatus::Granted
}

/// Every folder a scan may read, in the order the page lists them.
pub fn folders(
    home: &Path,
    db: &Database,
    added: &[PathBuf],
    full_disk_access: bool,
) -> Vec<ScopeFolder> {
    use ScanMode::{Deep, Dev, Standard};
    let mut folders = vec![
        fixed(
            FolderId::Caches,
            home.join("Library/Caches"),
            home,
            vec![Standard, Deep],
        ),
        fixed(
            FolderId::Logs,
            home.join("Library/Logs"),
            home,
            vec![Standard, Deep],
        ),
        fixed(
            FolderId::Developer,
            home.join("Library/Developer"),
            home,
            vec![Dev],
        ),
        fixed(FolderId::Projects, home.to_path_buf(), home, vec![Dev]),
    ];
    for folder in ProtectedFolder::ALL {
        let path = folder.path(home);
        folders.push(ScopeFolder {
            id: protected_id(folder),
            access: if exists(&path) {
                access_of(db, folder, full_disk_access)
            } else {
                Access::Missing
            },
            path: display_path(&path, Some(home)),
            modes: vec![Deep],
        });
    }
    for folder in added {
        folders.push(ScopeFolder {
            id: FolderId::Added,
            path: display_path(folder, Some(home)),
            access: if exists(folder) {
                Access::Granted
            } else {
                Access::Missing
            },
            modes: vec![Dev, Deep],
        });
    }
    folders
}

/// Reads the protected folder once, which makes macOS ask the user if it hasn't yet, and
/// remembers the answer. Blocks until the user answers the prompt.
pub fn request_access(home: &Path, db: &Database, folder: ProtectedFolder, now_ms: u64) -> Access {
    let path = folder.path(home);
    let access = match DirHandle::open_root(&path) {
        Ok((mut dir, stat)) if stat.is_dir() => match dir.next_name() {
            Some(Err(error))
                if matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES)) =>
            {
                Access::Denied
            }
            _ => Access::Granted,
        },
        Ok(_) => Access::Missing,
        Err(error) if matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES)) => {
            Access::Denied
        }
        Err(_) => Access::Missing,
    };
    let state = match access {
        Access::Granted => Some(AccessState::Granted),
        Access::Denied => Some(AccessState::Denied),
        _ => None,
    };
    if let Some(state) = state {
        // Not remembered if the history can't be written: the next visit asks again.
        let _ = db.set_folder_access(folder.key(), state, now_ms);
    }
    access
}

/// Where a Deep scan looks for large and old files: the protected folders you granted (or
/// all of them with Full Disk Access) and the folders you added.
pub fn large_roots(
    home: &Path,
    db: &Database,
    added: &[PathBuf],
    full_disk_access: bool,
) -> Vec<PathBuf> {
    ProtectedFolder::ALL
        .into_iter()
        .filter(|&folder| {
            matches!(
                access_of(db, folder, full_disk_access),
                Access::Granted | Access::ByFullDisk
            )
        })
        .map(|folder| folder.path(home))
        // A folder that isn't on this Mac isn't searched (nor reported as unreadable).
        .filter(|path| exists(path))
        .chain(added.iter().cloned())
        .collect()
}
