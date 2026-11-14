//! Read-only system metric readings and their IPC contract.
//!
//! The collector gathers raw OS values; [`to_metrics`] maps them into the DTO sent to the
//! frontend. The mapping is where "unavailable" is decided: the OS adapter reports `0` when
//! a source fails, so a zero total is never presented as a genuine reading.

mod activity;
mod battery;
mod collector;
mod details;
mod display;
mod names;
pub mod open_paths;
mod power_profile;
mod process_list;
mod process_sampler;
mod processes;
#[cfg(target_os = "macos")]
mod ps;
pub(crate) mod registry;
pub mod running_apps;
mod startup;
mod sysctl;

pub use activity::{ActivitySampler, SystemActivity};

/// A numeric sysctl value, for the issue checks.
pub fn sysctl_integer(name: &str) -> Option<u64> {
    sysctl::integer(name)
}
pub use battery::RawBattery;
pub use collector::SystemSampler;
pub use details::SystemDetails;
/// Shows a path with the home folder as `~`; also used by the Developer page.
pub(crate) use process_list::display_path;
pub use process_list::ProcessList;
pub use process_sampler::ProcessListSampler;
pub use processes::ProcessSummary;
/// Also used by the Storage analysis to keep app indexing off network volumes.
pub(crate) use startup::non_local_mounts;
pub use startup::{StartupItems, StartupScope};

use serde::Serialize;

/// Reads hardware and OS details. Runs on a worker thread; only the display is read on the
/// main thread, as AppKit requires.
pub fn read_details<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> SystemDetails {
    let mut raw = details::read_raw();
    raw.display = display::read(app);
    details::to_details(raw)
}

/// The internal battery now (charge, and charging or not), for the issue checks.
pub fn read_battery_now() -> Reading<BatteryReading> {
    map_battery(&battery::read_battery())
}

/// The battery's health as macOS states it (runs `system_profiler`; about 80 ms).
pub fn read_battery_health() -> Reading<details::BatteryHealth> {
    details::map_battery_health(&details::read_battery_health())
}

/// The battery sensor's temperature in °C.
pub fn read_battery_temperature() -> Reading<f64> {
    activity::read_battery_temperature()
}

/// Lists launch agents and daemons from the standard folders.
pub fn read_startup_items() -> StartupItems {
    startup::read()
}

pub(crate) fn unix_time_ms() -> u64 {
    // A clock before 1970 is a host misconfiguration; report the epoch rather than panic.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Identifies what produced the readings, shown to the user as the source: `sysinfo` for
/// CPU, memory, disk and processes; macOS IOKit power sources for the battery.
pub const METRICS_SOURCE: &str = "sysinfo and IOKit";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetrics {
    /// Milliseconds since the Unix epoch when the sample was taken.
    pub sampled_at_ms: u64,
    pub source: &'static str,
    pub cpu: Reading<CpuReading>,
    pub memory: Reading<MemoryReading>,
    pub disk: Reading<DiskReading>,
    pub battery: Reading<BatteryReading>,
    pub processes: Reading<ProcessSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Reading<T> {
    Available { value: T },
    Unavailable { reason: UnavailableReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    /// A first sample exists but a second, spaced sample is still required.
    WarmingUp,
    /// The operating system did not report a usable value.
    NotReported,
    /// The startup volume could not be identified.
    NotFound,
    /// This Mac has no internal battery; a genuine absence, not a failed reading.
    NotPresent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuReading {
    /// Average usage across all logical CPUs over `window_ms`, in the range 0–100.
    pub usage_percent: f64,
    pub window_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryReading {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskReading {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerSource {
    Battery,
    Ac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChargeState {
    Charging,
    Charged,
    /// Connected to power but not charging, for example while charging is held.
    NotCharging,
    Discharging,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryReading {
    pub charge_percent: f64,
    pub power_source: PowerSource,
    pub state: ChargeState,
    /// Minutes until full while charging or until empty while discharging; `None` while
    /// macOS is still calculating or when no estimate applies.
    pub minutes_remaining: Option<u64>,
}

/// Raw values as reported by the OS adapter, before availability decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSample {
    pub sampled_at_ms: u64,
    pub cpu: Option<RawCpu>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub startup_disk: Option<RawDisk>,
    pub battery: RawBattery,
    /// Already summarized by the collector, which owns the per-process CPU baseline.
    pub processes: Reading<ProcessSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawCpu {
    pub usage_percent: f32,
    pub window_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawDisk {
    pub total: u64,
    pub available: u64,
}

pub fn to_metrics(raw: &RawSample) -> SystemMetrics {
    SystemMetrics {
        sampled_at_ms: raw.sampled_at_ms,
        source: METRICS_SOURCE,
        cpu: map_cpu(raw.cpu),
        memory: map_memory(raw.memory_total, raw.memory_used, raw.memory_available),
        disk: map_disk(raw.startup_disk),
        battery: map_battery(&raw.battery),
        processes: raw.processes.clone(),
    }
}

/// Upper bound for a plausible estimate (48 h); larger values are treated as no estimate.
const MAX_PLAUSIBLE_MINUTES: i64 = 48 * 60;

pub(crate) fn map_battery(battery: &RawBattery) -> Reading<BatteryReading> {
    let fields = match battery {
        RawBattery::NotPresent => {
            return Reading::Unavailable {
                reason: UnavailableReason::NotPresent,
            }
        }
        RawBattery::Unreadable => {
            return Reading::Unavailable {
                reason: UnavailableReason::NotReported,
            }
        }
        RawBattery::Present(fields) => fields,
    };
    if fields.is_present == Some(false) {
        return Reading::Unavailable {
            reason: UnavailableReason::NotPresent,
        };
    }
    let not_reported = Reading::Unavailable {
        reason: UnavailableReason::NotReported,
    };
    // IOPSKeys.h marks these keys as required; a missing one is a malformed reading, not
    // `false`.
    let (Some(true), Some(current), Some(max), Some(on_ac_power), Some(is_charging)) = (
        fields.is_present,
        fields.current_capacity,
        fields.max_capacity,
        fields.on_ac_power,
        fields.is_charging,
    ) else {
        return not_reported;
    };
    if max <= 0 || !(0..=max).contains(&current) {
        return not_reported;
    }

    let state = if is_charging {
        ChargeState::Charging
    } else if !on_ac_power {
        ChargeState::Discharging
    } else {
        // Only consulted on AC power: macOS omits "Is Charged" while running on battery.
        match fields.is_charged {
            Some(true) => ChargeState::Charged,
            Some(false) => ChargeState::NotCharging,
            None => return not_reported,
        }
    };
    let estimate = match state {
        ChargeState::Charging => fields.time_to_full,
        ChargeState::Discharging => fields.time_to_empty,
        ChargeState::Charged | ChargeState::NotCharging => None,
    };
    // -1 means "calculating"; 0 is never shown as a real estimate.
    let minutes_remaining = estimate
        .filter(|minutes| (1..=MAX_PLAUSIBLE_MINUTES).contains(minutes))
        .and_then(|minutes| u64::try_from(minutes).ok());

    // Capacities are small integers (macOS reports percent, max 100), so f64 is exact.
    let charge_percent = current as f64 * 100.0 / max as f64;
    Reading::Available {
        value: BatteryReading {
            charge_percent,
            power_source: if on_ac_power {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            state,
            minutes_remaining,
        },
    }
}

fn map_cpu(cpu: Option<RawCpu>) -> Reading<CpuReading> {
    let Some(cpu) = cpu else {
        return Reading::Unavailable {
            reason: UnavailableReason::WarmingUp,
        };
    };
    if !cpu.usage_percent.is_finite() {
        return Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        };
    }
    Reading::Available {
        value: CpuReading {
            usage_percent: f64::from(cpu.usage_percent).clamp(0.0, 100.0),
            window_ms: cpu.window_ms,
        },
    }
}

fn map_memory(total: u64, used: u64, available: u64) -> Reading<MemoryReading> {
    if total == 0 {
        return Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        };
    }
    Reading::Available {
        value: MemoryReading {
            total_bytes: total,
            used_bytes: used.min(total),
            available_bytes: available.min(total),
        },
    }
}

fn map_disk(disk: Option<RawDisk>) -> Reading<DiskReading> {
    let Some(disk) = disk else {
        return Reading::Unavailable {
            reason: UnavailableReason::NotFound,
        };
    };
    if disk.total == 0 {
        return Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        };
    }
    let available = disk.available.min(disk.total);
    Reading::Available {
        value: DiskReading {
            total_bytes: disk.total,
            used_bytes: disk.total - available,
            available_bytes: available,
        },
    }
}
