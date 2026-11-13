//! The user's preferences, kept in the history database (`setting` table) so Rust applies them
//! itself: what a scan leaves out, how long activity is kept, which notifications are sent.
//! Every value is checked against the choices the Settings page offers; a stored value that
//! isn't one of them is replaced by its default, never used.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cleanup::{ScanMode, ScanOptions};
use crate::persistence::{Database, Unavailable};

/// The key the preferences are stored under; the version changes if their shape does.
const KEY: &str = "preferences.v1";
const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    /// The mode Cleanup starts with.
    pub default_scan: ScanMode,
    /// The options menu's starting values.
    pub scan_defaults: ScanOptions,
    /// Findings smaller than this are left out of Cleanup's review; 0 is off.
    pub min_size_mb: u32,
    /// Large files changed within this many days aren't suggested; 0 is off.
    pub skip_recent_days: u32,
    /// Quarantine holding more than this is mentioned (and notified, if on); 0 is off.
    pub quarantine_reminder_gb: u32,
    pub notify_scan: bool,
    pub notify_quarantine: bool,
    pub notify_sound: bool,
    /// Activity older than this is deleted (quarantine records are kept).
    pub retention_days: u32,
    /// Kleari sits in the menu bar and keeps running when its window is closed.
    #[serde(default = "on")]
    pub menu_bar: bool,
    /// Check GitHub for a newer Kleari at launch.
    #[serde(default = "on")]
    pub check_for_updates: bool,
    /// Also offer beta releases, not only stable ones (Settings › About). `None` until chosen:
    /// it then follows the installed version, so a beta keeps getting betas and a stable
    /// release only stable ones, as macOS's own betas do (`gets_betas`).
    #[serde(default)]
    pub beta_updates: Option<bool>,
    /// When Kleari scans on its own (ADR-015); off until chosen.
    #[serde(default)]
    pub scan_schedule: ScanSchedule,
    /// Notify issue alerts (low disk, a device's low battery, …) while Kleari runs (ADR-021).
    #[serde(default)]
    pub notify_issues: bool,
}

const fn on() -> bool {
    true
}

/// This build is a beta (its version has a pre-release part, such as `0.1.0-beta.1`).
const INSTALLED_IS_BETA: bool = {
    let version = env!("CARGO_PKG_VERSION").as_bytes();
    let mut index = 0;
    let mut beta = false;
    while index < version.len() {
        if version[index] == b'-' {
            beta = true;
        }
        index += 1;
    }
    beta
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Frequency {
    #[default]
    Off,
    Daily,
    Weekly,
}

/// A read-only scan Kleari runs on its own while it is running, then notifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanSchedule {
    pub frequency: Frequency,
    /// For a weekly scan: 0 is Monday, 6 is Sunday.
    pub weekday: u8,
    /// The local hour it runs at, 0–23.
    pub hour: u8,
    pub mode: ScanMode,
}

impl Default for ScanSchedule {
    fn default() -> Self {
        Self {
            frequency: Frequency::Off,
            weekday: 0,
            hour: 9,
            mode: ScanMode::Standard,
        }
    }
}

impl ScanSchedule {
    pub fn valid(&self) -> bool {
        self.weekday <= 6 && self.hour <= 23
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            default_scan: ScanMode::Standard,
            scan_defaults: ScanOptions::default(),
            min_size_mb: 0,
            skip_recent_days: 0,
            quarantine_reminder_gb: 0,
            notify_scan: false,
            notify_quarantine: false,
            notify_sound: false,
            retention_days: 90,
            menu_bar: true,
            check_for_updates: true,
            beta_updates: None,
            scan_schedule: ScanSchedule::default(),
            notify_issues: false,
        }
    }
}

impl Preferences {
    /// Only the values the Settings page offers: sizes in steps of 10 MB up to 500, up to 90
    /// days, up to 100 GB, and activity kept for 30 to 360 days in steps of 30, or a year.
    pub fn valid(&self) -> bool {
        let retention = self.retention_days == 365
            || (30..=360).contains(&self.retention_days) && self.retention_days.is_multiple_of(30);
        self.scan_defaults.valid()
            && self.scan_schedule.valid()
            && self.min_size_mb <= 500
            && self.min_size_mb.is_multiple_of(10)
            && self.skip_recent_days <= 90
            && self.quarantine_reminder_gb <= 100
            && retention
    }

    /// Whether update checks include betas: as chosen, or until then, whether this build is
    /// one.
    pub fn gets_betas(&self) -> bool {
        self.beta_updates.unwrap_or(INSTALLED_IS_BETA)
    }

    /// The oldest activity kept, given the time now.
    pub fn retention_start_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(u64::from(self.retention_days) * DAY_MS)
    }

    pub fn min_size_bytes(&self) -> u64 {
        u64::from(self.min_size_mb) * 1_000_000
    }

    pub fn reminder_bytes(&self) -> Option<u64> {
        (self.quarantine_reminder_gb > 0)
            .then(|| u64::from(self.quarantine_reminder_gb) * 1_000_000_000)
    }

    /// Large files changed at or after this time aren't suggested; `None` when off.
    pub fn recent_cutoff_ms(&self, now_ms: u64) -> Option<u64> {
        (self.skip_recent_days > 0)
            .then(|| now_ms.saturating_sub(u64::from(self.skip_recent_days) * DAY_MS))
    }
}

/// Whether `path` is inside a folder the user excluded, or contains one. Either way it is never
/// read or offered whole: measuring or moving it would reach into the excluded folder.
pub fn touches_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded
        .iter()
        .any(|folder| path.starts_with(folder) || folder.starts_with(path))
}

/// Whether `path` is inside an excluded folder (it may still contain one: a fixed folder
/// that is listed, not measured whole).
pub fn inside_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded.iter().any(|folder| path.starts_with(folder))
}

/// What was read: the preferences in effect, and whether the stored ones had to be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    pub preferences: Preferences,
    /// `saved`, `default` (nothing saved yet), `invalid` (saved but not usable, so the
    /// defaults apply) or `unavailable` (the history can't be read; defaults apply).
    pub source: &'static str,
}

/// The saved preferences, or the defaults with the reason.
pub fn load(db: &Database) -> Loaded {
    let stored = match db.settings() {
        Ok(rows) => rows
            .into_iter()
            .find(|(key, _)| key == KEY)
            .map(|(_, value)| value),
        Err(_) => {
            return Loaded {
                preferences: Preferences::default(),
                source: "unavailable",
            }
        }
    };
    let Some(text) = stored else {
        return Loaded {
            preferences: Preferences::default(),
            source: "default",
        };
    };
    match serde_json::from_str::<Preferences>(&text) {
        Ok(preferences) if preferences.valid() => Loaded {
            preferences,
            source: "saved",
        },
        _ => Loaded {
            preferences: Preferences::default(),
            source: "invalid",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    Invalid,
    Unavailable(Unavailable),
}

/// Saves `preferences` if every value is one the page offers.
pub fn save(db: &Database, preferences: &Preferences) -> Result<(), SaveError> {
    if !preferences.valid() {
        return Err(SaveError::Invalid);
    }
    let text = serde_json::to_string(preferences).map_err(|_| SaveError::Invalid)?;
    db.set_setting(KEY, &text).map_err(SaveError::Unavailable)
}
