//! Startup-volume capacity and the space macOS manages, for the Storage page. Read-only:
//! capacity comes from Foundation's volume resource values, swap and the sleep image from the
//! sizes of the files in their two folders, and Full Disk Access from opening (never listing)
//! folders that only that permission unlocks. Nothing here walks the disk.

use std::io;
use std::path::Path;

use serde::Serialize;

use super::fsdir::DirHandle;
use crate::system_monitor::{Reading, UnavailableReason};

/// Swap files live on the VM volume; the sleep image in `/private/var/vm`. (`statfs` can't
/// be used: on APFS it reports the whole container for every volume.)
const SWAP_AND_SLEEP_FOLDERS: [&str; 2] = ["/System/Volumes/VM", "/private/var/vm"];
/// Entries read per folder; macOS keeps a handful of files there.
const MAX_SWAP_ENTRIES: usize = 256;

/// Folders under the home folder that macOS opens only with Full Disk Access, and that it
/// never prompts for, so probing them can't put a dialog in front of the user.
const FULL_DISK_ACCESS_PROBES: [&str; 3] = ["Library/Mail", "Library/Safari", "Library/Messages"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSpace {
    pub name: String,
    pub total_bytes: u64,
    /// Available as Finder counts it: free space plus space macOS can purge on its own.
    pub available_bytes: u64,
    /// `total_bytes - available_bytes`.
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessStatus {
    Granted,
    NotGranted,
    /// None of the probe folders exist, so Kleari can't tell.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    pub volume: Reading<VolumeSpace>,
    /// Finder's available space minus space free right now: an estimate of what macOS can
    /// purge (caches, snapshots and other purgeable files).
    pub purgeable_bytes: Reading<u64>,
    /// Used space on the VM volume: swap files and the sleep image.
    pub swap_and_sleep_bytes: Reading<u64>,
    pub full_disk_access: AccessStatus,
}

/// Raw volume values as Foundation reports them; any may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawVolume {
    pub name: Option<String>,
    pub total: Option<u64>,
    /// `NSURLVolumeAvailableCapacityKey`: free right now.
    pub free: Option<u64>,
    /// `NSURLVolumeAvailableCapacityForImportantUsageKey`: free plus purgeable.
    pub important: Option<u64>,
}

const fn unavailable<T>(reason: UnavailableReason) -> Reading<T> {
    Reading::Unavailable { reason }
}

/// Maps raw readings; a missing or zero total is never shown as a real volume.
pub fn to_overview(
    raw: &RawVolume,
    vm_used: Option<u64>,
    full_disk_access: AccessStatus,
) -> StorageOverview {
    let swap_and_sleep_bytes = vm_used
        .map_or(unavailable(UnavailableReason::NotReported), |value| {
            Reading::Available { value }
        });
    let (Some(name), Some(total), Some(free)) = (raw.name.clone(), raw.total, raw.free) else {
        return StorageOverview {
            volume: unavailable(UnavailableReason::NotReported),
            purgeable_bytes: unavailable(UnavailableReason::NotReported),
            swap_and_sleep_bytes,
            full_disk_access,
        };
    };
    if total == 0 || name.is_empty() {
        return StorageOverview {
            volume: unavailable(UnavailableReason::NotReported),
            purgeable_bytes: unavailable(UnavailableReason::NotReported),
            swap_and_sleep_bytes,
            full_disk_access,
        };
    }
    let free = free.min(total);
    // Without the purgeable-aware figure, fall back to free space and say purgeable space
    // is unknown, rather than reporting it as zero.
    let (available, purgeable_bytes) = match raw.important {
        Some(important) => {
            let available = important.clamp(free, total);
            (
                available,
                Reading::Available {
                    value: available - free,
                },
            )
        }
        None => (free, unavailable(UnavailableReason::NotReported)),
    };
    StorageOverview {
        volume: Reading::Available {
            value: VolumeSpace {
                name,
                total_bytes: total,
                available_bytes: available,
                used_bytes: total - available,
            },
        },
        purgeable_bytes,
        swap_and_sleep_bytes,
        full_disk_access,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Opened,
    Denied,
    Missing,
}

fn probe(path: &Path) -> Probe {
    match DirHandle::open_root(path) {
        // Opened and closed straight away: nothing inside is listed or read.
        Ok(_) => Probe::Opened,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Probe::Denied,
        Err(_) => Probe::Missing,
    }
}

/// One opened probe proves access; a denial with none opened proves its absence.
pub fn decide_access(probes: &[Probe]) -> AccessStatus {
    if probes.contains(&Probe::Opened) {
        AccessStatus::Granted
    } else if probes.contains(&Probe::Denied) {
        AccessStatus::NotGranted
    } else {
        AccessStatus::Unknown
    }
}

/// Whether Full Disk Access is on, by opening (never listing) folders only it unlocks and that
/// macOS never prompts for. Cleanup uses it too: with it, protected folders need no prompt.
pub(crate) fn full_disk_access(home: Option<&Path>) -> AccessStatus {
    let Some(home) = home else {
        return AccessStatus::Unknown;
    };
    let probes: Vec<Probe> = FULL_DISK_ACCESS_PROBES
        .iter()
        .map(|relative| probe(&home.join(relative)))
        .collect();
    decide_access(&probes)
}

/// Space taken by the regular files directly inside `folders`, without following symlinks or
/// entering subfolders. `None` if no folder could be listed; a partial listing is a minimum.
fn files_on_disk(folders: &[&Path]) -> Option<u64> {
    let mut total: Option<u64> = None;
    for folder in folders {
        let Ok((mut dir, _)) = DirHandle::open_root(folder) else {
            continue;
        };
        let mut bytes = 0_u64;
        for _ in 0..MAX_SWAP_ENTRIES {
            let Some(Ok(name)) = dir.next_name() else {
                break;
            };
            let Ok(stat) = dir.stat_child(&name) else {
                continue;
            };
            if !stat.is_dir() && !stat.is_symlink() {
                bytes = bytes.saturating_add(stat.blocks.saturating_mul(512));
            }
        }
        total = Some(total.unwrap_or(0).saturating_add(bytes));
    }
    total
}

/// Reads capacity, managed space and the Full Disk Access status.
pub fn read(home: Option<&Path>) -> StorageOverview {
    let folders = SWAP_AND_SLEEP_FOLDERS.map(Path::new);
    to_overview(
        &macos::read_volume(),
        files_on_disk(&folders),
        full_disk_access(home),
    )
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{
        NSArray, NSNumber, NSString, NSURLResourceKey,
        NSURLVolumeAvailableCapacityForImportantUsageKey, NSURLVolumeAvailableCapacityKey,
        NSURLVolumeLocalizedNameKey, NSURLVolumeTotalCapacityKey, NSURL,
    };

    use super::RawVolume;

    fn number(value: Option<Retained<AnyObject>>) -> Option<u64> {
        let value = value?;
        let number = value.downcast_ref::<NSNumber>()?;
        u64::try_from(number.longLongValue()).ok()
    }

    pub fn read_volume() -> RawVolume {
        let url = NSURL::fileURLWithPath(&NSString::from_str("/"));
        // SAFETY: the keys are Foundation's own constants, valid for the process lifetime.
        let (total, free, important, name): (
            &NSURLResourceKey,
            &NSURLResourceKey,
            &NSURLResourceKey,
            &NSURLResourceKey,
        ) = unsafe {
            (
                NSURLVolumeTotalCapacityKey,
                NSURLVolumeAvailableCapacityKey,
                NSURLVolumeAvailableCapacityForImportantUsageKey,
                NSURLVolumeLocalizedNameKey,
            )
        };
        let keys = NSArray::from_slice(&[total, free, important, name]);
        let Ok(values) = url.resourceValuesForKeys_error(&keys) else {
            return RawVolume::default();
        };
        RawVolume {
            name: values
                .objectForKey(name)
                .and_then(|value| value.downcast_ref::<NSString>().map(ToString::to_string)),
            total: number(values.objectForKey(total)),
            free: number(values.objectForKey(free)),
            important: number(values.objectForKey(important)),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    use super::RawVolume;

    pub fn read_volume() -> RawVolume {
        RawVolume::default()
    }
}
