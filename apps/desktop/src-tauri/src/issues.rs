//! Issue alerts (ADR-021): things worth your attention, read-only and on this Mac. Checked when
//! the Dashboard or the menu-bar popover asks, and by a background watcher while Kleari runs,
//! which notifies each alert group you turned on in Settings › Notifications.
//!
//! Reading (`read`) and deciding (`assess`) are separate, so every threshold is tested on
//! plain facts. Nothing here changes anything; each issue links to where it can be dealt with.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::preferences::AlertGroup;
use crate::system_monitor::registry;

/// The startup disk is low below this share free…
const LOW_DISK_SHARE: f64 = 0.15;
/// …or below this many bytes free.
const LOW_DISK_BYTES: u64 = 15_000_000_000;
/// …and critically low below this share free.
const CRITICAL_DISK_SHARE: f64 = 0.05;
/// A connected drive is low below this share free.
const LOW_DRIVE_SHARE: f64 = 0.10;
/// The Trash is worth emptying from this size.
const TRASH_BYTES: u64 = 5_000_000_000;
/// A Bluetooth device's battery is low at or below this percent.
const LOW_DEVICE_BATTERY: i64 = 20;
/// Draining faster than this many watts on battery is "fast".
const DRAIN_WATTS: f64 = 15.0;
/// The Mac's battery is low at or below this percent while on battery…
const LOW_BATTERY: f64 = 20.0;
/// …and critically low at or below this.
const CRITICAL_BATTERY: f64 = 10.0;
/// The battery is hot from this temperature (°C).
const HOT_BATTERY_CELSIUS: f64 = 45.0;
/// A battery near the end of its design life: this share of its design cycles used.
const CYCLES_SHARE: f64 = 0.9;
/// The CPU is busy when the 15-minute load average reaches this share of its cores.
const BUSY_CPU_SHARE: f64 = 0.9;
/// More login items and agents than this slow startup down.
const MANY_LOGIN_ITEMS: usize = 15;
/// A Cleanup scan is due after this many days without one.
const CLEANUP_DUE_DAYS: u64 = 7;
/// An app not opened for this many days may be worth removing.
const UNUSED_APP_DAYS: f64 = 180.0;
/// Unused apps listed, at most.
const MAX_UNUSED_APPS: usize = 10;

const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    LowDiskSpace,
    CriticalDiskSpace,
    LowDriveSpace,
    HeavyMemory,
    CriticalMemory,
    BusyCpu,
    BatteryDrain,
    BatteryLow,
    BatteryCritical,
    BatteryService,
    BatteryCycles,
    BatteryHot,
    LowDeviceBattery,
    FullTrash,
    UnusedApps,
    ManyLoginItems,
    CleanupDue,
}

impl IssueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowDiskSpace => "lowDiskSpace",
            Self::CriticalDiskSpace => "criticalDiskSpace",
            Self::LowDriveSpace => "lowDriveSpace",
            Self::HeavyMemory => "heavyMemory",
            Self::CriticalMemory => "criticalMemory",
            Self::BusyCpu => "busyCpu",
            Self::BatteryDrain => "batteryDrain",
            Self::BatteryLow => "batteryLow",
            Self::BatteryCritical => "batteryCritical",
            Self::BatteryService => "batteryService",
            Self::BatteryCycles => "batteryCycles",
            Self::BatteryHot => "batteryHot",
            Self::LowDeviceBattery => "lowDeviceBattery",
            Self::FullTrash => "fullTrash",
            Self::UnusedApps => "unusedApps",
            Self::ManyLoginItems => "manyLoginItems",
            Self::CleanupDue => "cleanupDue",
        }
    }

    /// The Settings switch that notifies it.
    pub const fn group(self) -> AlertGroup {
        match self {
            Self::LowDiskSpace
            | Self::CriticalDiskSpace
            | Self::LowDriveSpace
            | Self::FullTrash => AlertGroup::Storage,
            Self::HeavyMemory | Self::CriticalMemory | Self::BusyCpu => AlertGroup::Performance,
            Self::BatteryDrain
            | Self::BatteryLow
            | Self::BatteryCritical
            | Self::BatteryService
            | Self::BatteryCycles
            | Self::BatteryHot
            | Self::LowDeviceBattery => AlertGroup::Battery,
            Self::ManyLoginItems => AlertGroup::LoginItems,
            Self::UnusedApps | Self::CleanupDue => AlertGroup::Reminders,
        }
    }

    /// How long before the same alert is notified again while it lasts.
    const fn repeat(self) -> Duration {
        match self {
            Self::UnusedApps | Self::CleanupDue | Self::BatteryCycles | Self::ManyLoginItems => {
                Duration::from_secs(7 * 24 * 60 * 60)
            }
            _ => Duration::from_secs(24 * 60 * 60),
        }
    }

    /// Notified again as soon as it comes back after clearing (the next discharge, the next
    /// time it heats up), not only after `repeat`.
    const fn resets_when_cleared(self) -> bool {
        matches!(
            self,
            Self::BatteryLow | Self::BatteryCritical | Self::BatteryHot | Self::CriticalMemory
        )
    }

    /// Notified only once it has lasted two checks in a row ("remains elevated").
    const fn needs_to_last(self) -> bool {
        matches!(self, Self::HeavyMemory)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Worth doing something about soon.
    Warning,
    /// Worth a look when convenient.
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: IssueKind,
    pub severity: Severity,
    /// What it is about: a drive's or device's name, or apps' names joined by ", ".
    pub subject: Option<String>,
    /// Bytes free or held, a percent, °C, a count or days, as the kind says.
    pub value: Option<u64>,
    /// A second number: total bytes, watts, cores or design cycles.
    pub total: Option<u64>,
}

impl Issue {
    const fn new(kind: IssueKind, severity: Severity) -> Self {
        Self {
            kind,
            severity,
            subject: None,
            value: None,
            total: None,
        }
    }
}

/// The Mac's own battery, as the checks need it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BatteryFacts {
    pub percent: Option<f64>,
    /// On battery power (not plugged in).
    pub discharging: bool,
    /// Plugged in and full.
    pub charged: bool,
    pub celsius: Option<f64>,
    pub cycles: Option<u64>,
    pub design_cycles: Option<u64>,
    pub service_recommended: bool,
}

/// Everything the checks look at, read at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// Free and total bytes of the startup disk.
    pub disk: Option<(u64, u64)>,
    /// Connected drives: name, free and total bytes.
    pub drives: Vec<(Option<String>, u64, u64)>,
    /// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warning, 4 critical.
    pub memory_pressure: Option<u64>,
    /// The 15-minute load average and the logical cores.
    pub load: Option<(f64, u64)>,
    pub drain_watts: Option<f64>,
    /// `None` on a Mac without a battery.
    pub battery: Option<BatteryFacts>,
    /// Bluetooth devices: name and battery percent.
    pub devices: Vec<(Option<String>, i64)>,
    pub trash_bytes: u64,
    pub unused_apps: Vec<String>,
    /// Login items and agents that start in your session; `None` if they weren't read.
    pub login_items: Option<usize>,
    /// Days since the last Cleanup scan (or since Kleari started counting); `None` unknown.
    pub days_since_scan: Option<u64>,
}

fn share_free(free: u64, total: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    if total == 0 {
        1.0
    } else {
        free as f64 / total as f64
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole(value: f64) -> u64 {
    value.round().max(0.0) as u64
}

/// The issues these facts make. Pure, so every threshold is tested.
pub fn assess(facts: &Facts) -> Vec<Issue> {
    let mut issues = Vec::new();
    if let Some((free, total)) = facts.disk {
        let share = share_free(free, total);
        let kind = if share < CRITICAL_DISK_SHARE {
            Some(IssueKind::CriticalDiskSpace)
        } else if share < LOW_DISK_SHARE || free < LOW_DISK_BYTES {
            Some(IssueKind::LowDiskSpace)
        } else {
            None
        };
        if let Some(kind) = kind {
            issues.push(Issue {
                value: Some(free),
                total: Some(total),
                ..Issue::new(kind, Severity::Warning)
            });
        }
    }
    for (name, free, total) in &facts.drives {
        if *total > 0 && share_free(*free, *total) < LOW_DRIVE_SHARE {
            issues.push(Issue {
                subject: name.clone(),
                value: Some(*free),
                total: Some(*total),
                ..Issue::new(IssueKind::LowDriveSpace, Severity::Warning)
            });
        }
    }
    match facts.memory_pressure {
        Some(level) if level >= 4 => issues.push(Issue {
            value: Some(level),
            ..Issue::new(IssueKind::CriticalMemory, Severity::Warning)
        }),
        Some(level) if level >= 2 => issues.push(Issue {
            value: Some(level),
            ..Issue::new(IssueKind::HeavyMemory, Severity::Info)
        }),
        _ => {}
    }
    if let Some((load, cores)) = facts.load {
        #[allow(clippy::cast_precision_loss)]
        let share = if cores == 0 { 0.0 } else { load / cores as f64 };
        if share >= BUSY_CPU_SHARE {
            issues.push(Issue {
                value: Some(whole(share * 100.0)),
                total: Some(cores),
                ..Issue::new(IssueKind::BusyCpu, Severity::Info)
            });
        }
    }
    if let Some(watts) = facts.drain_watts.filter(|&watts| watts > DRAIN_WATTS) {
        issues.push(Issue {
            total: Some(whole(watts)),
            ..Issue::new(IssueKind::BatteryDrain, Severity::Warning)
        });
    }
    if let Some(battery) = facts.battery {
        if let Some(percent) = battery.percent.filter(|_| battery.discharging) {
            let kind = if percent <= CRITICAL_BATTERY {
                Some(IssueKind::BatteryCritical)
            } else if percent <= LOW_BATTERY {
                Some(IssueKind::BatteryLow)
            } else {
                None
            };
            if let Some(kind) = kind {
                issues.push(Issue {
                    value: Some(whole(percent)),
                    ..Issue::new(kind, Severity::Warning)
                });
            }
        }
        if battery.service_recommended {
            issues.push(Issue::new(IssueKind::BatteryService, Severity::Warning));
        }
        if let (Some(cycles), Some(design)) = (battery.cycles, battery.design_cycles) {
            #[allow(clippy::cast_precision_loss)]
            if design > 0 && cycles as f64 >= design as f64 * CYCLES_SHARE {
                issues.push(Issue {
                    value: Some(cycles),
                    total: Some(design),
                    ..Issue::new(IssueKind::BatteryCycles, Severity::Info)
                });
            }
        }
        if let Some(celsius) = battery.celsius.filter(|&c| c >= HOT_BATTERY_CELSIUS) {
            issues.push(Issue {
                value: Some(whole(celsius)),
                ..Issue::new(IssueKind::BatteryHot, Severity::Warning)
            });
        }
    }
    for (name, percent) in &facts.devices {
        if (0..=LOW_DEVICE_BATTERY).contains(percent) {
            issues.push(Issue {
                subject: name.clone(),
                value: u64::try_from(*percent).ok(),
                ..Issue::new(IssueKind::LowDeviceBattery, Severity::Warning)
            });
        }
    }
    if facts.trash_bytes >= TRASH_BYTES {
        issues.push(Issue {
            value: Some(facts.trash_bytes),
            ..Issue::new(IssueKind::FullTrash, Severity::Info)
        });
    }
    if !facts.unused_apps.is_empty() {
        issues.push(Issue {
            value: u64::try_from(facts.unused_apps.len()).ok(),
            subject: Some(facts.unused_apps.join(", ")),
            ..Issue::new(IssueKind::UnusedApps, Severity::Info)
        });
    }
    if let Some(count) = facts.login_items.filter(|&count| count > MANY_LOGIN_ITEMS) {
        issues.push(Issue {
            value: u64::try_from(count).ok(),
            ..Issue::new(IssueKind::ManyLoginItems, Severity::Info)
        });
    }
    if let Some(days) = facts
        .days_since_scan
        .filter(|&days| days >= CLEANUP_DUE_DAYS)
    {
        issues.push(Issue {
            value: Some(days),
            ..Issue::new(IssueKind::CleanupDue, Severity::Info)
        });
    }
    issues
}

/// Free and total bytes of the volume holding `path`.
// statfs field widths differ by platform (u64 on macOS, others on Linux CI), so a conversion
// needed on one target is a no-op on another.
#[allow(clippy::useless_conversion, clippy::unnecessary_fallible_conversions)]
fn space(path: &Path) -> Option<(u64, u64)> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stats` is writable storage for one statfs.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs succeeded and filled `stats`.
    let stats = unsafe { stats.assume_init() };
    let block = u64::try_from(stats.f_bsize).ok()?;
    Some((
        u64::try_from(stats.f_bavail).ok()?.saturating_mul(block),
        u64::try_from(stats.f_blocks).ok()?.saturating_mul(block),
    ))
}

/// The 15-minute load average (`getloadavg`), with the logical cores.
fn load() -> Option<(f64, u64)> {
    let mut averages = [0.0_f64; 3];
    // SAFETY: `averages` is writable storage for the three values asked for.
    let read = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
    let cores = crate::system_monitor::sysctl_integer("hw.logicalcpu")?;
    (read == 3 && averages[2].is_finite()).then_some((averages[2], cores))
}

/// The battery's charge and state now: cheap (one IOKit read), for the quick checks.
fn battery_now() -> Option<(f64, bool, bool)> {
    use crate::system_monitor::{ChargeState, Reading};
    match crate::system_monitor::read_battery_now() {
        Reading::Available { value } => Some((
            value.charge_percent,
            value.state == ChargeState::Discharging,
            value.state == ChargeState::Charged,
        )),
        Reading::Unavailable { .. } => None,
    }
}

fn battery_facts() -> Option<BatteryFacts> {
    use crate::system_monitor::Reading;
    let (percent, discharging, charged) = battery_now()?;
    let health = match crate::system_monitor::read_battery_health() {
        Reading::Available { value } => Some(value),
        Reading::Unavailable { .. } => None,
    };
    let celsius = match crate::system_monitor::read_battery_temperature() {
        Reading::Available { value } => Some(value),
        Reading::Unavailable { .. } => None,
    };
    Some(BatteryFacts {
        percent: Some(percent),
        discharging,
        charged,
        celsius,
        cycles: health.as_ref().and_then(|health| health.cycle_count),
        design_cycles: health.as_ref().and_then(|health| health.design_cycle_count),
        service_recommended: health.is_some_and(|health| health.service_recommended),
    })
}

/// Login items and agents that start in your session, by label.
fn session_items() -> Option<Vec<(String, String)>> {
    use crate::system_monitor::StartupScope;
    let startup = crate::system_monitor::read_startup_items();
    if startup.unreadable_folder_count > 0 && startup.items.is_empty() {
        return None;
    }
    Some(
        startup
            .items
            .into_iter()
            .filter(|item| item.scope != StartupScope::SystemDaemon && !item.disabled_in_plist)
            .map(|item| (item.label, item.name))
            .collect(),
    )
}

/// Days since the last Cleanup scan. With none kept, since the reminder started counting (the
/// first check), so a new install isn't reminded at once.
fn days_since_scan(db: &crate::persistence::Database, now_ms: u64) -> Option<u64> {
    const SINCE: &str = "reminders.since";
    let last = db.last_event_ms(crate::persistence::EventKind::Scan).ok()?;
    let since = match last {
        Some(at) => at,
        None => {
            let stored = db
                .settings()
                .ok()?
                .into_iter()
                .find(|(key, _)| key == SINCE)
                .and_then(|(_, value)| value.parse::<u64>().ok());
            match stored {
                Some(at) => at,
                None => {
                    db.set_setting(SINCE, &now_ms.to_string()).ok()?;
                    now_ms
                }
            }
        }
    };
    Some(now_ms.saturating_sub(since) / DAY_MS)
}

/// Reads every fact now (about a second: the Trash is measured, Spotlight asked per app).
pub fn read(home: &Path, uid: u32, db: Option<&crate::persistence::Database>) -> Facts {
    let drives = crate::maintenance::external_volumes()
        .into_iter()
        .filter_map(|volume| {
            let (free, total) = space(&volume)?;
            Some((
                volume
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                free,
                total,
            ))
        })
        .collect();
    let devices = registry::each(
        "AppleDeviceManagementHIDEventService",
        "Product",
        &["BatteryPercent"],
    )
    .into_iter()
    .filter_map(|(name, values)| Some((name, values.first().copied().flatten()?)))
    .collect();
    Facts {
        disk: space(home),
        drives,
        memory_pressure: crate::system_monitor::sysctl_integer(
            "kern.memorystatus_vm_pressure_level",
        ),
        load: load(),
        drain_watts: drain_watts(),
        battery: battery_facts(),
        devices,
        trash_bytes: trash_bytes(home, uid),
        unused_apps: unused_apps(home),
        login_items: session_items().map(|items| items.len()),
        days_since_scan: db
            .and_then(|db| days_since_scan(db, crate::system_monitor::unix_time_ms())),
    }
}

/// Every check, read now.
pub fn check(home: &Path, uid: u32, db: Option<&crate::persistence::Database>) -> Vec<Issue> {
    assess(&read(home, uid, db))
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

/// The quick checks (battery charge) run this often; everything else every `FULL_EVERY`.
const QUICK_EVERY: Duration = Duration::from_secs(60);
const FULL_EVERY: u32 = 15;

/// What a notification says.
fn notification(issue: &Issue) -> (String, String) {
    let gb = |bytes: Option<u64>| {
        #[allow(clippy::cast_precision_loss)]
        let value = bytes.unwrap_or(0) as f64 / 1e9;
        format!("{value:.1} GB")
    };
    let subject = issue.subject.clone().unwrap_or_default();
    let value = issue.value.unwrap_or(0);
    match issue.kind {
        IssueKind::LowDiskSpace => (
            "Your startup disk is getting full".to_owned(),
            format!("{} free. Kleari can show what takes space.", gb(issue.value)),
        ),
        IssueKind::CriticalDiskSpace => (
            "Your startup disk is almost full".to_owned(),
            format!(
                "Only {} free. macOS and apps may stop working properly.",
                gb(issue.value)
            ),
        ),
        IssueKind::LowDriveSpace => (
            format!("{subject} is almost full"),
            format!("{} free on this drive.", gb(issue.value)),
        ),
        IssueKind::HeavyMemory => (
            "Memory stays under pressure".to_owned(),
            "Apps may slow down. Processes shows what uses the most.".to_owned(),
        ),
        IssueKind::CriticalMemory => (
            "Memory pressure is critical".to_owned(),
            "Apps may stall. Quit what you don't need; Processes shows what uses the most."
                .to_owned(),
        ),
        IssueKind::BusyCpu => (
            "Your Mac has been busy for a while".to_owned(),
            "The processor has been nearly fully used for 15 minutes. Processes shows what."
                .to_owned(),
        ),
        IssueKind::BatteryDrain => (
            "Your battery is draining fast".to_owned(),
            format!(
                "About {} W on battery. Processes shows what is busy.",
                issue.total.unwrap_or(0)
            ),
        ),
        IssueKind::BatteryLow => (
            "Battery low".to_owned(),
            format!("{value}% left. Plug in soon."),
        ),
        IssueKind::BatteryCritical => (
            "Battery critically low".to_owned(),
            format!("{value}% left. Plug in now to avoid losing work."),
        ),
        IssueKind::BatteryService => (
            "macOS recommends servicing your battery".to_owned(),
            "See System Settings › Battery › Battery Health.".to_owned(),
        ),
        IssueKind::BatteryCycles => (
            "Your battery is near the end of its design life".to_owned(),
            format!("{value} of {} cycles used.", issue.total.unwrap_or(0)),
        ),
        IssueKind::BatteryHot => (
            "Your battery is hot".to_owned(),
            format!(
                "{value} °C. Let your Mac cool: unplug heavy work, keep its vents clear."
            ),
        ),
        IssueKind::LowDeviceBattery => (
            format!("{subject} battery is low"),
            format!("{value}% left."),
        ),
        IssueKind::FullTrash => (
            "The Trash is getting large".to_owned(),
            format!("{} in the Trash.", gb(issue.value)),
        ),
        IssueKind::UnusedApps => ("Apps you haven't opened in months".to_owned(), subject),
        IssueKind::ManyLoginItems => (
            "Many apps start at login".to_owned(),
            format!("{value} login items and agents. Maintenance › Login items lists them."),
        ),
        IssueKind::CleanupDue => (
            "Time for a cleanup".to_owned(),
            format!("No Cleanup scan in {value} days. A scan only reads; nothing moves until you approve it."),
        ),
    }
}

/// When each alert was last notified, and what the last checks saw, to decide what to notify.
#[derive(Default)]
struct Notifier {
    notified: HashMap<(IssueKind, Option<String>), Instant>,
    /// Kinds seen at the last full check (for alerts that must last).
    last_full: HashSet<IssueKind>,
    /// Session login items seen so far (label → name); `None` until the first full check.
    login_items: Option<HashMap<String, String>>,
    /// Whether the battery was full and plugged in at the last quick check.
    charged: Option<bool>,
}

impl Notifier {
    /// The issues worth notifying now, from `issues` (`full`: every kind was checked).
    fn due(&mut self, issues: &[Issue], full: bool, now: Instant) -> Vec<Issue> {
        let present: HashSet<IssueKind> = issues.iter().map(|issue| issue.kind).collect();
        // A cleared alert that resets can be notified again when it comes back.
        self.notified.retain(|(kind, _), _| {
            !kind.resets_when_cleared() || present.contains(kind) || (!full && !is_quick(*kind))
        });
        let mut due = Vec::new();
        for issue in issues {
            if issue.kind.needs_to_last() && !self.last_full.contains(&issue.kind) {
                continue;
            }
            let key = (issue.kind, issue.subject.clone());
            if self
                .notified
                .get(&key)
                .is_some_and(|when| now.duration_since(*when) < issue.kind.repeat())
            {
                continue;
            }
            self.notified.insert(key, now);
            due.push(issue.clone());
        }
        if full {
            self.last_full = present;
        }
        due
    }

    /// Login items that appeared since the last full check (none at the first).
    fn new_login_items(&mut self, now: Vec<(String, String)>) -> Vec<String> {
        let current: HashMap<String, String> = now.into_iter().collect();
        let added = self.login_items.as_ref().map_or_else(Vec::new, |seen| {
            let mut added: Vec<String> = current
                .iter()
                .filter(|(label, _)| !seen.contains_key(*label))
                .map(|(_, name)| name.clone())
                .collect();
            added.sort();
            added
        });
        self.login_items = Some(current);
        added
    }

    /// Whether the battery just became full while plugged in (never at the first check).
    fn just_charged(&mut self, charged: bool) -> bool {
        let before = self.charged.replace(charged);
        before == Some(false) && charged
    }
}

/// The kinds the quick checks cover.
const fn is_quick(kind: IssueKind) -> bool {
    matches!(kind, IssueKind::BatteryLow | IssueKind::BatteryCritical)
}

/// The battery's low or critical alert from its charge now.
fn quick_issues(percent: f64, discharging: bool) -> Vec<Issue> {
    assess(&Facts {
        battery: Some(BatteryFacts {
            percent: Some(percent),
            discharging,
            ..BatteryFacts::default()
        }),
        ..Facts::default()
    })
    .into_iter()
    .filter(|issue| is_quick(issue.kind))
    .collect()
}

/// Checks while Kleari runs: the battery's charge every minute, everything else every 15
/// minutes, and notifies what each group you turned on covers (each alert at most once a day,
/// reminders once a week). Nothing is read while every group is off.
pub fn watch(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let mut notifier = Notifier::default();
        let mut round: u32 = 0;
        loop {
            std::thread::sleep(QUICK_EVERY);
            round = round.wrapping_add(1);
            let history = app.state::<crate::history::History>();
            let preferences = crate::preferences::load(&history.db).preferences;
            let groups = [
                AlertGroup::Storage,
                AlertGroup::Performance,
                AlertGroup::Battery,
                AlertGroup::LoginItems,
                AlertGroup::Reminders,
            ];
            if !groups.iter().any(|&group| preferences.notifies(group)) {
                continue;
            }
            let send = |id: &str, (title, body): (String, String)| {
                crate::notifications::send(id, &title, &body, preferences.notify_sound);
            };
            let now = Instant::now();
            let full = round.is_multiple_of(FULL_EVERY);
            let issues = if full {
                let Some(home) = crate::storage::home_dir() else {
                    continue;
                };
                // SAFETY: getuid has no preconditions.
                let uid = unsafe { libc::getuid() };
                if preferences.notifies(AlertGroup::LoginItems) {
                    if let Some(items) = session_items() {
                        for name in notifier.new_login_items(items) {
                            send(
                                "issue-newLoginItem",
                                (
                                    "A new login item was added".to_owned(),
                                    format!("{name} now starts when you log in. Maintenance › Login items can stop it."),
                                ),
                            );
                        }
                    }
                }
                check(&home, uid, Some(&history.db))
            } else {
                battery_now().map_or_else(Vec::new, |(percent, discharging, _)| {
                    quick_issues(percent, discharging)
                })
            };
            if let Some((_, _, charged)) = battery_now() {
                if notifier.just_charged(charged) && preferences.notifies(AlertGroup::Battery) {
                    send(
                        "issue-batteryCharged",
                        (
                            "Battery fully charged".to_owned(),
                            "You can unplug your Mac.".to_owned(),
                        ),
                    );
                }
            }
            for issue in notifier.due(&issues, full, now) {
                if preferences.notifies(issue.kind.group()) {
                    send(
                        &format!("issue-{}", issue.kind.as_str()),
                        notification(&issue),
                    );
                }
            }
        }
    });
}

/// The issues now, for the Dashboard and the menu-bar popover.
#[tauri::command]
pub async fn get_issues(
    app: tauri::AppHandle,
) -> Result<Vec<Issue>, crate::commands::CommandError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let home = crate::cleanup_commands::home()?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let history = app.state::<crate::history::History>();
        Ok(check(&home, uid, Some(&history.db)))
    })
    .await
    .map_err(crate::cleanup_commands::blocking_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn kinds(facts: &Facts) -> Vec<IssueKind> {
        assess(facts).into_iter().map(|issue| issue.kind).collect()
    }

    fn on_battery(percent: f64) -> Facts {
        Facts {
            battery: Some(BatteryFacts {
                percent: Some(percent),
                discharging: true,
                ..BatteryFacts::default()
            }),
            ..Facts::default()
        }
    }

    #[test]
    fn disk_space_is_low_below_15_percent_and_critical_below_5() {
        let disk = |free| Facts {
            disk: Some((free * GB, 1000 * GB)),
            ..Facts::default()
        };
        assert!(kinds(&disk(400)).is_empty());
        assert_eq!(kinds(&disk(140)), [IssueKind::LowDiskSpace]);
        assert_eq!(kinds(&disk(40)), [IssueKind::CriticalDiskSpace]);
        // A small disk is also low under 15 GB free.
        let small = Facts {
            disk: Some((12 * GB, 60 * GB)),
            ..Facts::default()
        };
        assert_eq!(kinds(&small), [IssueKind::LowDiskSpace]);
    }

    #[test]
    fn memory_and_cpu() {
        let pressure = |level| Facts {
            memory_pressure: Some(level),
            ..Facts::default()
        };
        assert!(kinds(&pressure(1)).is_empty());
        assert_eq!(kinds(&pressure(2)), [IssueKind::HeavyMemory]);
        assert_eq!(kinds(&pressure(4)), [IssueKind::CriticalMemory]);
        let busy = Facts {
            load: Some((7.5, 8)),
            ..Facts::default()
        };
        assert_eq!(kinds(&busy), [IssueKind::BusyCpu]);
        let calm = Facts {
            load: Some((2.0, 8)),
            ..Facts::default()
        };
        assert!(kinds(&calm).is_empty());
    }

    #[test]
    fn the_battery_is_low_at_20_and_critical_at_10_only_on_battery() {
        assert!(kinds(&on_battery(45.0)).is_empty());
        assert_eq!(kinds(&on_battery(20.0)), [IssueKind::BatteryLow]);
        assert_eq!(kinds(&on_battery(9.0)), [IssueKind::BatteryCritical]);
        let mut plugged = on_battery(9.0);
        if let Some(battery) = plugged.battery.as_mut() {
            battery.discharging = false;
        }
        assert!(kinds(&plugged).is_empty());
    }

    #[test]
    fn battery_health_cycles_and_heat() {
        let facts = Facts {
            battery: Some(BatteryFacts {
                percent: Some(80.0),
                celsius: Some(46.2),
                cycles: Some(920),
                design_cycles: Some(1000),
                service_recommended: true,
                ..BatteryFacts::default()
            }),
            ..Facts::default()
        };
        assert_eq!(
            kinds(&facts),
            [
                IssueKind::BatteryService,
                IssueKind::BatteryCycles,
                IssueKind::BatteryHot
            ]
        );
    }

    #[test]
    fn login_items_and_the_weekly_reminder() {
        let facts = Facts {
            login_items: Some(16),
            days_since_scan: Some(7),
            ..Facts::default()
        };
        assert_eq!(
            kinds(&facts),
            [IssueKind::ManyLoginItems, IssueKind::CleanupDue]
        );
        let fine = Facts {
            login_items: Some(15),
            days_since_scan: Some(6),
            ..Facts::default()
        };
        assert!(kinds(&fine).is_empty());
    }

    #[test]
    fn each_alert_is_notified_once_a_day_and_again_after_it_clears_if_it_resets() {
        let mut notifier = Notifier::default();
        let start = Instant::now();
        let low = quick_issues(15.0, true);
        assert_eq!(notifier.due(&low, false, start).len(), 1);
        assert!(notifier.due(&low, false, start).is_empty());
        // Plugged in: it clears, so the next discharge notifies again.
        assert!(notifier.due(&[], false, start).is_empty());
        assert_eq!(notifier.due(&low, false, start).len(), 1);
    }

    #[test]
    fn heavy_memory_is_notified_only_once_it_lasts() {
        let mut notifier = Notifier::default();
        let now = Instant::now();
        let heavy = assess(&Facts {
            memory_pressure: Some(2),
            ..Facts::default()
        });
        assert!(notifier.due(&heavy, true, now).is_empty());
        assert_eq!(notifier.due(&heavy, true, now).len(), 1);
    }

    #[test]
    fn new_login_items_and_a_full_charge_are_noticed_after_the_first_check() {
        let mut notifier = Notifier::default();
        let item = |label: &str| (label.to_owned(), format!("{label} app"));
        assert!(notifier.new_login_items(vec![item("a")]).is_empty());
        assert_eq!(
            notifier.new_login_items(vec![item("a"), item("b")]),
            ["b app"]
        );
        assert!(!notifier.just_charged(true));
        assert!(!notifier.just_charged(false));
        assert!(notifier.just_charged(true));
        assert!(!notifier.just_charged(true));
    }

    #[test]
    fn every_kind_has_a_group_and_a_notification() {
        let all = [
            IssueKind::LowDiskSpace,
            IssueKind::CriticalDiskSpace,
            IssueKind::LowDriveSpace,
            IssueKind::HeavyMemory,
            IssueKind::CriticalMemory,
            IssueKind::BusyCpu,
            IssueKind::BatteryDrain,
            IssueKind::BatteryLow,
            IssueKind::BatteryCritical,
            IssueKind::BatteryService,
            IssueKind::BatteryCycles,
            IssueKind::BatteryHot,
            IssueKind::LowDeviceBattery,
            IssueKind::FullTrash,
            IssueKind::UnusedApps,
            IssueKind::ManyLoginItems,
            IssueKind::CleanupDue,
        ];
        for kind in all {
            let (title, _) = notification(&Issue::new(kind, Severity::Info));
            assert!(!title.is_empty(), "{kind:?}");
            let _ = kind.group();
        }
    }

    #[test]
    fn checking_reads_this_mac_without_changing_anything() {
        let home = std::env::temp_dir();
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let facts = read(&home, uid, None);
        assert!(facts
            .disk
            .is_some_and(|(free, total)| total > 0 && free <= total));
        let issues = assess(&facts);
        for kind in [
            IssueKind::LowDiskSpace,
            IssueKind::CriticalDiskSpace,
            IssueKind::BusyCpu,
            IssueKind::BatteryDrain,
            IssueKind::FullTrash,
            IssueKind::UnusedApps,
        ] {
            assert!(
                issues.iter().filter(|issue| issue.kind == kind).count() <= 1,
                "{kind:?}"
            );
        }
    }
}
