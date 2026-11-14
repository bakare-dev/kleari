//! Hardware and OS details that change rarely: chip, cores, GPU, memory size, model, macOS
//! version, display, startup volume, boot time and battery health. Read on demand, never
//! stored. Everything comes from public sources: `sysctl`, the IORegistry, CoreGraphics and
//! AppKit; no shell execution or privileges.

use serde::Serialize;

use super::{names, registry, sysctl, Reading, UnavailableReason};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDetails {
    pub model: Reading<ModelInfo>,
    pub chip: Reading<String>,
    pub cpu_cores: Reading<CoreCounts>,
    pub gpu_cores: Reading<u64>,
    pub memory_bytes: Reading<u64>,
    pub os: Reading<OsInfo>,
    pub display: Reading<DisplayInfo>,
    pub startup_volume: Reading<VolumeInfo>,
    /// Milliseconds since the Unix epoch when the Mac last started.
    pub boot_time_ms: Reading<u64>,
    pub battery_health: Reading<BatteryHealth>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// `hw.model`, for example `Mac16,1`.
    pub identifier: String,
    /// The marketing name from the bundled table; `None` for identifiers it doesn't know.
    pub name: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreCounts {
    pub total: u64,
    /// Performance and efficiency cores on Apple silicon; `None` where macOS reports no
    /// clusters (Intel).
    pub performance: Option<u64>,
    pub efficiency: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    /// `kern.osproductversion`, for example `26.1`.
    pub version: String,
    pub name: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    /// macOS's localized name, for example "Built-in Liquid Retina XDR Display".
    pub name: String,
    pub built_in: bool,
    /// Diagonal in inches from the panel's reported physical size; `None` if not reported.
    pub diagonal_inches: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub name: String,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryHealth {
    pub cycle_count: Option<u64>,
    /// The cycles the battery is designed for (`DesignCycleCount9C`), when reported.
    pub design_cycle_count: Option<u64>,
    /// macOS's own "Maximum Capacity" (System Information); without it, full-charge capacity
    /// as a share of design capacity, capped at 100. `None` when neither is known.
    pub maximum_capacity_percent: Option<f64>,
    /// macOS says the battery should be serviced ("Service Recommended").
    pub service_recommended: bool,
}

/// Raw values before availability decisions. Display is read separately on the main thread.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawDetails {
    pub model: Option<String>,
    pub chip: Option<String>,
    pub physical_cores: Option<u64>,
    pub performance_cores: Option<u64>,
    pub efficiency_cores: Option<u64>,
    pub gpu_cores: Option<i64>,
    pub memory_bytes: Option<u64>,
    pub os_version: Option<String>,
    pub display: Option<RawDisplay>,
    pub startup_volume: Option<(String, u64)>,
    pub boot_time_s: Option<u64>,
    pub battery: RawBatteryHealth,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawDisplay {
    pub name: String,
    pub built_in: bool,
    pub width_mm: f64,
    pub height_mm: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum RawBatteryHealth {
    /// No `AppleSmartBattery` service: the Mac has no internal battery.
    #[default]
    NotPresent,
    Present {
        cycle_count: Option<i64>,
        design_cycle_count: Option<i64>,
        design_capacity: Option<i64>,
        nominal_capacity: Option<i64>,
        /// What macOS states (`system_profiler`), when it could be read.
        reported: Option<super::power_profile::ReportedHealth>,
    },
}

const fn not_reported<T>() -> Reading<T> {
    Reading::Unavailable {
        reason: UnavailableReason::NotReported,
    }
}

fn available<T>(value: Option<T>) -> Reading<T> {
    value.map_or_else(not_reported, |value| Reading::Available { value })
}

pub fn to_details(raw: RawDetails) -> SystemDetails {
    SystemDetails {
        model: available(raw.model.map(|identifier| ModelInfo {
            name: names::model_name(&identifier),
            identifier,
        })),
        chip: available(raw.chip),
        cpu_cores: available(raw.physical_cores.filter(|&total| total > 0).map(|total| {
            // Clusters are shown only when they add up to the total, so a partial report is
            // never presented as a breakdown.
            let split = match (raw.performance_cores, raw.efficiency_cores) {
                (Some(performance), Some(efficiency)) if performance + efficiency == total => {
                    (Some(performance), Some(efficiency))
                }
                _ => (None, None),
            };
            CoreCounts {
                total,
                performance: split.0,
                efficiency: split.1,
            }
        })),
        gpu_cores: available(
            raw.gpu_cores
                .and_then(|cores| u64::try_from(cores).ok())
                .filter(|&cores| cores > 0),
        ),
        memory_bytes: available(raw.memory_bytes.filter(|&bytes| bytes > 0)),
        os: available(raw.os_version.map(|version| OsInfo {
            name: names::macos_name(&version),
            version,
        })),
        display: available(raw.display.map(map_display)),
        startup_volume: available(
            raw.startup_volume
                .filter(|(name, total)| !name.is_empty() && *total > 0)
                .map(|(name, total_bytes)| VolumeInfo { name, total_bytes }),
        ),
        boot_time_ms: available(
            raw.boot_time_s
                .filter(|&seconds| seconds > 0)
                .and_then(|seconds| seconds.checked_mul(1000)),
        ),
        battery_health: map_battery_health(&raw.battery),
    }
}

fn map_display(display: RawDisplay) -> DisplayInfo {
    let RawDisplay {
        name,
        built_in,
        width_mm,
        height_mm,
    } = display;
    let valid = width_mm.is_finite() && height_mm.is_finite() && width_mm > 0.0 && height_mm > 0.0;
    DisplayInfo {
        name,
        built_in,
        // One decimal, as Apple quotes panel sizes (14.2″).
        diagonal_inches: valid.then(|| ((width_mm.hypot(height_mm) / 25.4) * 10.0).round() / 10.0),
    }
}

pub(crate) fn map_battery_health(battery: &RawBatteryHealth) -> Reading<BatteryHealth> {
    let RawBatteryHealth::Present {
        cycle_count,
        design_cycle_count,
        design_capacity,
        nominal_capacity,
        reported,
    } = battery
    else {
        return Reading::Unavailable {
            reason: UnavailableReason::NotPresent,
        };
    };
    let cycle_count = cycle_count.and_then(|count| u64::try_from(count).ok());
    // macOS's own figure first: the raw capacities can read several percent higher.
    let computed = match (design_capacity, nominal_capacity) {
        (Some(design), Some(nominal)) if *design > 0 && *nominal > 0 => {
            // Capacities are mAh values well inside f64's exact integer range.
            Some((*nominal as f64 * 100.0 / *design as f64).min(100.0))
        }
        _ => None,
    };
    let maximum_capacity_percent = reported
        .as_ref()
        .and_then(|health| health.maximum_capacity_percent)
        .or(computed);
    if cycle_count.is_none() && maximum_capacity_percent.is_none() {
        return not_reported();
    }
    Reading::Available {
        value: BatteryHealth {
            cycle_count,
            design_cycle_count: design_cycle_count
                .and_then(|count| u64::try_from(count).ok())
                .filter(|&count| count > 0),
            maximum_capacity_percent,
            service_recommended: reported
                .as_ref()
                .and_then(|health| health.service_recommended)
                .unwrap_or(false),
        },
    }
}

/// Reads everything except the display, which needs the main thread (see `display`).
pub fn read_raw() -> RawDetails {
    let (startup_volume, boot_time_s) = read_volume_and_boot();
    RawDetails {
        model: sysctl::string("hw.model"),
        chip: sysctl::string("machdep.cpu.brand_string"),
        physical_cores: sysctl::integer("hw.physicalcpu"),
        // Present on Apple silicon: perflevel0 is the performance cluster, perflevel1 the
        // efficiency cluster.
        performance_cores: (sysctl::integer("hw.nperflevels") == Some(2))
            .then(|| sysctl::integer("hw.perflevel0.physicalcpu"))
            .flatten(),
        efficiency_cores: (sysctl::integer("hw.nperflevels") == Some(2))
            .then(|| sysctl::integer("hw.perflevel1.physicalcpu"))
            .flatten(),
        gpu_cores: registry::integers("AGXAccelerator", &["gpu-core-count"])
            .and_then(|values| values.into_iter().next().flatten()),
        memory_bytes: sysctl::integer("hw.memsize"),
        os_version: sysctl::string("kern.osproductversion"),
        display: None,
        startup_volume,
        boot_time_s,
        battery: read_battery_health(),
    }
}

fn read_volume_and_boot() -> (Option<(String, u64)>, Option<u64>) {
    use std::path::Path;
    use sysinfo::{DiskRefreshKind, Disks, System};

    // Where sysinfo can't list disks, the volume is unknown (`None`), never made up.
    let disks =
        Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage()).ok();
    let volume = disks.as_ref().and_then(|disks| {
        disks
            .iter()
            .find(|disk| disk.mount_point() == Path::new("/"))
            .map(|disk| {
                (
                    disk.name().to_string_lossy().into_owned(),
                    disk.total_space(),
                )
            })
    });
    (volume, System::boot_time().ok())
}

pub(crate) fn read_battery_health() -> RawBatteryHealth {
    const CLASS: &str = "AppleSmartBattery";
    let Some(top) = registry::integers(
        CLASS,
        &[
            "CycleCount",
            "DesignCapacity",
            "NominalChargeCapacity",
            "DesignCycleCount9C",
        ],
    ) else {
        return RawBatteryHealth::NotPresent;
    };
    // Recent macOS versions keep the capacities only inside "BatteryData".
    let nested = registry::dictionary_integers(
        CLASS,
        "BatteryData",
        &["DesignCapacity", "NominalChargeCapacity"],
    )
    .unwrap_or_else(|| vec![None, None]);
    RawBatteryHealth::Present {
        cycle_count: top[0],
        design_cycle_count: top[3],
        design_capacity: top[1].or(nested[0]),
        nominal_capacity: top[2].or(nested[1]),
        reported: super::power_profile::read(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_monitor::power_profile::ReportedHealth;

    fn health(reported: Option<ReportedHealth>) -> BatteryHealth {
        let raw = RawBatteryHealth::Present {
            cycle_count: Some(312),
            design_cycle_count: Some(1000),
            // 99 % from the raw capacities, as a friend's Mac read while macOS said 96 %.
            design_capacity: Some(6_000),
            nominal_capacity: Some(5_940),
            reported,
        };
        match map_battery_health(&raw) {
            Reading::Available { value } => value,
            other => panic!("available: {other:?}"),
        }
    }

    #[test]
    fn the_maximum_capacity_is_the_one_macos_states() {
        let stated = health(Some(ReportedHealth {
            maximum_capacity_percent: Some(96.0),
            service_recommended: Some(true),
        }));
        assert_eq!(stated.maximum_capacity_percent, Some(96.0));
        assert!(stated.service_recommended);
        assert_eq!(stated.design_cycle_count, Some(1000));
        // Without macOS's figure, the raw capacities are the fallback.
        let computed = health(None);
        assert_eq!(computed.maximum_capacity_percent, Some(99.0));
        assert!(!computed.service_recommended);
    }
}
