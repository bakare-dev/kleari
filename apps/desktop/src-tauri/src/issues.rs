//! Issue alerts (ADR-021): things worth your attention, read-only and on this Mac. Checked when
//! the Dashboard asks, and every 15 minutes while Kleari runs, when a new warning is notified
//! (at most once a day each, if issue notifications are on).
//!
//! Nothing here changes anything; each issue links to where it can be dealt with.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::system_monitor::registry;

/// The startup disk is low below this share free…
const LOW_DISK_SHARE: f64 = 0.10;
/// …or below this many bytes free.
const LOW_DISK_BYTES: u64 = 15_000_000_000;
/// A connected drive is low below this share free.
const LOW_DRIVE_SHARE: f64 = 0.10;
/// The Trash is worth emptying from this size.
const TRASH_BYTES: u64 = 5_000_000_000;
/// A Bluetooth device's battery is low at or below this percent.
const LOW_DEVICE_BATTERY: i64 = 20;
/// Draining faster than this many watts on battery is "fast".
const DRAIN_WATTS: f64 = 15.0;
/// An app not opened for this many days may be worth removing.
const UNUSED_APP_DAYS: f64 = 180.0;
/// Unused apps listed, at most.
const MAX_UNUSED_APPS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    LowDiskSpace,
    LowDriveSpace,
    HeavyMemory,
    BatteryDrain,
    LowDeviceBattery,
    FullTrash,
    UnusedApps,
}

impl IssueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowDiskSpace => "lowDiskSpace",
            Self::LowDriveSpace => "lowDriveSpace",
            Self::HeavyMemory => "heavyMemory",
            Self::BatteryDrain => "batteryDrain",
            Self::LowDeviceBattery => "lowDeviceBattery",
            Self::FullTrash => "fullTrash",
            Self::UnusedApps => "unusedApps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Worth doing something about soon (notified).
    Warning,
    /// Worth a look when convenient (shown, not notified).
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: IssueKind,
    pub severity: Severity,
    /// What it is about: a drive's or device's name, or apps' names joined by ", ".
    pub subject: Option<String>,
    /// Bytes free or held, a percent, or a count, as the kind says.
    pub value: Option<u64>,
    /// A second number: total bytes, or watts.
    pub total: Option<u64>,
}

/// Free and total bytes of the volume holding `path`.
// statfs field widths differ by platform (u64 on macOS, others on Linux CI), so a conversion
// needed on one target is a no-op on another.
#[allow(clippy::useless_conversion)]
fn space(path: &Path) -> Option<(u64, u64)> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stats` is writable storage for one statfs.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs succeeded and filled `stats`.
    let stats = unsafe { stats.assume_init() };
    let block = u64::from(stats.f_bsize);
    Some((
        u64::try_from(stats.f_bavail).ok()?.saturating_mul(block),
        u64::try_from(stats.f_blocks).ok()?.saturating_mul(block),
    ))
}

fn low(free: u64, total: u64, share: f64) -> bool {
    #[allow(clippy::cast_precision_loss)]
    let fraction = if total == 0 {
        1.0
    } else {
        free as f64 / total as f64
    };
    fraction < share
}

/// Every check, read now.
pub fn check(home: &Path, uid: u32) -> Vec<Issue> {
    let mut issues = Vec::new();
    if let Some((free, total)) = space(home) {
        if low(free, total, LOW_DISK_SHARE) || free < LOW_DISK_BYTES {
            issues.push(Issue {
                kind: IssueKind::LowDiskSpace,
                severity: Severity::Warning,
                subject: None,
                value: Some(free),
                total: Some(total),
            });
        }
    }
    for volume in crate::maintenance::external_volumes() {
        let Some((free, total)) = space(&volume) else {
            continue;
        };
        if total > 0 && low(free, total, LOW_DRIVE_SHARE) {
            issues.push(Issue {
                kind: IssueKind::LowDriveSpace,
                severity: Severity::Warning,
                subject: volume
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                value: Some(free),
                total: Some(total),
            });
        }
    }
    // kern.memorystatus_vm_pressure_level: 1 normal, 2 warning, 4 critical.
    if let Some(level) =
        crate::system_monitor::sysctl_integer("kern.memorystatus_vm_pressure_level")
    {
        if level >= 2 {
            issues.push(Issue {
                kind: IssueKind::HeavyMemory,
                severity: if level >= 4 {
                    Severity::Warning
                } else {
                    Severity::Info
                },
                subject: None,
                value: Some(level),
                total: None,
            });
        }
    }
    if let Some(watts) = drain_watts() {
        if watts > DRAIN_WATTS {
            issues.push(Issue {
                kind: IssueKind::BatteryDrain,
                severity: Severity::Warning,
                subject: None,
                value: None,
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                total: Some(watts.round() as u64),
            });
        }
    }
    for (name, values) in registry::each(
        "AppleDeviceManagementHIDEventService",
        "Product",
        &["BatteryPercent"],
    ) {
        let Some(Some(percent)) = values.first() else {
            continue;
        };
        if (0..=LOW_DEVICE_BATTERY).contains(percent) {
            issues.push(Issue {
                kind: IssueKind::LowDeviceBattery,
                severity: Severity::Warning,
                subject: name,
                value: u64::try_from(*percent).ok(),
                total: None,
            });
        }
    }
    let trash = trash_bytes(home, uid);
    if trash >= TRASH_BYTES {
        issues.push(Issue {
            kind: IssueKind::FullTrash,
            severity: Severity::Info,
            subject: None,
            value: Some(trash),
            total: None,
        });
    }
    let unused = unused_apps(home);
    if !unused.is_empty() {
        issues.push(Issue {
            kind: IssueKind::UnusedApps,
            severity: Severity::Info,
            value: u64::try_from(unused.len()).ok(),
            subject: Some(unused.join(", ")),
            total: None,
        });
    }
    issues
}

/// Watts drawn from the battery now, when on battery: `-InstantAmperage × Voltage`.
fn drain_watts() -> Option<f64> {
    let values = registry::integers(
        "AppleSmartBattery",
        &["InstantAmperage", "Voltage", "ExternalConnected"],
    )?;
    let (amperage, voltage) = (
        values.first().copied().flatten()?,
        values.get(1).copied().flatten()?,
    );
    // ExternalConnected is a boolean, which reads as no integer; plugged in shows as current in.
    if amperage >= 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    Some((-amperage) as f64 * voltage as f64 / 1_000_000.0)
}

/// Bytes in your Trash on the startup disk and connected drives (what can be read).
fn trash_bytes(home: &Path, uid: u32) -> u64 {
    let cancel = AtomicBool::new(false);
    let mut folders: Vec<PathBuf> = vec![home.join(".Trash")];
    folders.extend(
        crate::maintenance::external_volumes()
            .into_iter()
            .map(|volume| volume.join(".Trashes").join(uid.to_string())),
    );
    let mut links = crate::developer::Links::default();
    folders
        .iter()
        .filter_map(|folder| crate::developer::open_absolute(folder))
        .map(|(dir, dev)| crate::developer::measure_dir(dir, dev, &mut links, &cancel).bytes)
        .fold(0, u64::saturating_add)
}

/// Apps (not Apple's) that Spotlight says haven't been opened for `UNUSED_APP_DAYS`, oldest
/// first. An app Spotlight has no date for isn't listed.
fn unused_apps(home: &Path) -> Vec<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let mut unused: Vec<(f64, String)> = Vec::new();
    for folder in crate::apps::app_folders(Some(home)) {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "app")
                || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                continue;
            }
            let Some((used, apple)) = last_used(&path) else {
                continue;
            };
            let days = (now - used) / 86_400.0;
            if !apple && days >= UNUSED_APP_DAYS {
                let name = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                unused.push((used, name));
            }
        }
    }
    unused.sort_by(|a, b| a.0.total_cmp(&b.0));
    unused
        .into_iter()
        .take(MAX_UNUSED_APPS)
        .map(|(_, name)| name)
        .collect()
}

/// When Spotlight last saw the app opened (seconds since 1970), and whether it is Apple's.
#[cfg(target_os = "macos")]
fn last_used(path: &Path) -> Option<(f64, bool)> {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_foundation::{CFDate, CFRetained, CFString, CFType};

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn MDItemCreate(allocator: *const c_void, path: &CFString) -> Option<NonNull<CFType>>;
        fn MDItemCopyAttribute(item: &CFType, name: &CFString) -> Option<NonNull<CFType>>;
    }
    /// Seconds from 1970 to 2001, CFAbsoluteTime's reference date.
    const FROM_1970_TO_2001: f64 = 978_307_200.0;

    let text = CFString::from_str(path.to_str()?);
    // SAFETY: a null allocator selects the default; `text` is a valid CFString. The result
    // follows the Create rule, so it is owned here.
    let item = unsafe { MDItemCreate(std::ptr::null(), &text) }?;
    // SAFETY: MDItemCreate returned an owned, non-null MDItemRef.
    let item: CFRetained<CFType> = unsafe { CFRetained::from_raw(item) };
    let copy = |name: &str| -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(name);
        // SAFETY: `item` is a live MDItemRef; the result follows the Copy rule.
        let value = unsafe { MDItemCopyAttribute(&item, &name) }?;
        // SAFETY: an owned, non-null CF object.
        Some(unsafe { CFRetained::from_raw(value) })
    };
    let bundle = copy("kMDItemCFBundleIdentifier")
        .and_then(|value| value.downcast_ref::<CFString>().map(ToString::to_string))
        .unwrap_or_default();
    let used = copy("kMDItemLastUsedDate")?;
    let date = used.downcast_ref::<CFDate>()?;
    Some((
        date.absolute_time() + FROM_1970_TO_2001,
        bundle.to_lowercase().starts_with("com.apple."),
    ))
}

#[cfg(not(target_os = "macos"))]
fn last_used(_path: &Path) -> Option<(f64, bool)> {
    None
}

/// How often the background check runs.
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// The same warning is notified at most this often.
const NOTIFY_AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// What a warning says in a notification.
fn notification(issue: &Issue) -> (String, String) {
    let gb = |bytes: Option<u64>| {
        #[allow(clippy::cast_precision_loss)]
        let value = bytes.unwrap_or(0) as f64 / 1e9;
        format!("{value:.1} GB")
    };
    let subject = issue.subject.clone().unwrap_or_default();
    match issue.kind {
        IssueKind::LowDiskSpace => (
            "Your startup disk is almost full".to_owned(),
            format!(
                "{} free. Kleari can show what takes space.",
                gb(issue.value)
            ),
        ),
        IssueKind::LowDriveSpace => (
            format!("{subject} is almost full"),
            format!("{} free on this drive.", gb(issue.value)),
        ),
        IssueKind::HeavyMemory => (
            "Memory is under heavy pressure".to_owned(),
            "Apps may slow down. Processes shows what uses the most.".to_owned(),
        ),
        IssueKind::BatteryDrain => (
            "Your battery is draining fast".to_owned(),
            format!(
                "About {} W on battery. Processes shows what is busy.",
                issue.total.unwrap_or(0)
            ),
        ),
        IssueKind::LowDeviceBattery => (
            format!("{subject} battery is low"),
            format!("{}% left.", issue.value.unwrap_or(0)),
        ),
        IssueKind::FullTrash => (
            "The Trash is getting large".to_owned(),
            format!("{} in the Trash.", gb(issue.value)),
        ),
        IssueKind::UnusedApps => ("Apps you haven't opened in months".to_owned(), subject),
    }
}

/// Checks every 15 minutes while Kleari runs and notifies new warnings (at most once a day
/// each) when issue notifications are on. Info-level issues are only shown on the Dashboard.
pub fn watch(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let mut notified: std::collections::HashMap<
            (IssueKind, Option<String>),
            std::time::Instant,
        > = std::collections::HashMap::new();
        loop {
            std::thread::sleep(WATCH_EVERY);
            let history = app.state::<crate::history::History>();
            let preferences = crate::preferences::load(&history.db).preferences;
            if !preferences.notify_issues {
                continue;
            }
            let Some(home) = crate::storage::home_dir() else {
                continue;
            };
            // SAFETY: getuid has no preconditions.
            let uid = unsafe { libc::getuid() };
            for issue in check(&home, uid) {
                if issue.severity != Severity::Warning {
                    continue;
                }
                let key = (issue.kind, issue.subject.clone());
                if notified
                    .get(&key)
                    .is_some_and(|when| when.elapsed() < NOTIFY_AGAIN_AFTER)
                {
                    continue;
                }
                let (title, body) = notification(&issue);
                crate::notifications::send(
                    &format!("issue-{}", issue.kind.as_str()),
                    &title,
                    &body,
                    preferences.notify_sound,
                );
                notified.insert(key, std::time::Instant::now());
            }
        }
    });
}

/// The issues now, for the Dashboard.
#[tauri::command]
pub async fn get_issues() -> Result<Vec<Issue>, crate::commands::CommandError> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = crate::cleanup_commands::home()?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        Ok(check(&home, uid))
    })
    .await
    .map_err(crate::cleanup_commands::blocking_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_is_low_below_a_tenth_free() {
        assert!(low(5, 100, LOW_DISK_SHARE));
        assert!(!low(20, 100, LOW_DISK_SHARE));
        assert!(!low(0, 0, LOW_DISK_SHARE));
    }

    #[test]
    fn checking_reads_this_mac_without_changing_anything() {
        let home = std::env::temp_dir();
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let issues = check(&home, uid);
        // Each kind appears at most once, except drives and devices, one per item.
        for kind in [
            IssueKind::LowDiskSpace,
            IssueKind::HeavyMemory,
            IssueKind::BatteryDrain,
            IssueKind::FullTrash,
            IssueKind::UnusedApps,
        ] {
            assert!(
                issues.iter().filter(|issue| issue.kind == kind).count() <= 1,
                "{kind:?}"
            );
        }
        assert!(space(&home).is_some_and(|(free, total)| total > 0 && free <= total));
    }
}
