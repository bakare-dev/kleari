//! Lightweight readings for the System page's live charts, sampled about once a second while
//! the page is visible (every 5 s on the Dashboard): CPU, memory, swap, memory pressure, thermal state and network and
//! startup-disk throughput. It never enumerates processes (that stays in `SystemSampler`).
//! Nothing is stored; the frontend keeps only the points seen while the page is open.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, System,
    MINIMUM_CPU_UPDATE_INTERVAL,
};

use super::battery::read_battery;
use super::{
    map_battery, sysctl, BatteryReading, CpuReading, MemoryReading, Reading, UnavailableReason,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemActivity {
    pub sampled_at_ms: u64,
    pub cpu: Reading<CpuReading>,
    pub memory: Reading<MemoryReading>,
    pub swap: Reading<SwapReading>,
    pub memory_pressure: Reading<MemoryPressure>,
    pub thermal_state: Reading<ThermalState>,
    /// The battery's own temperature sensor, in °C: the one temperature macOS publishes.
    /// Not the chip's, which has no public API on Apple silicon.
    pub battery_temperature: Reading<f64>,
    /// The same IOKit power-source reading as `get_system_metrics`; cheap to read each second.
    pub battery: Reading<BatteryReading>,
    pub network: Reading<NetworkRates>,
    pub disk: Reading<DiskRates>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapReading {
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryPressure {
    Normal,
    Warning,
    Critical,
}

/// `NSProcessInfo.thermalState`: Apple's public thermal signal. Kleari shows this state
/// instead of a temperature, which has no public API on Apple silicon.
// Only the macOS reader constructs these; other targets (Linux CI) report "not reported".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRates {
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
    pub window_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskRates {
    pub read_bytes_per_second: f64,
    pub written_bytes_per_second: f64,
    pub window_ms: u64,
}

/// Cumulative byte counters at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counters {
    pub first: u64,
    pub second: u64,
}

/// Bytes per second between two counter snapshots. `None` when no time passed or a counter
/// went backwards (an interface reset or disk remount), so a bogus spike is never shown.
pub fn rates(previous: Counters, current: Counters, window_ms: u64) -> Option<(f64, f64)> {
    if window_ms == 0 || current.first < previous.first || current.second < previous.second {
        return None;
    }
    let seconds = window_ms as f64 / 1000.0;
    // Byte deltas over one or two seconds are far inside f64's exact integer range.
    Some((
        (current.first - previous.first) as f64 / seconds,
        (current.second - previous.second) as f64 / seconds,
    ))
}

pub fn memory_pressure_from_level(level: Option<u64>) -> Reading<MemoryPressure> {
    // kern.memorystatus_vm_pressure_level: 1 normal, 2 warning, 4 critical.
    match level {
        Some(1) => Reading::Available {
            value: MemoryPressure::Normal,
        },
        Some(2) => Reading::Available {
            value: MemoryPressure::Warning,
        },
        Some(4) => Reading::Available {
            value: MemoryPressure::Critical,
        },
        _ => Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        },
    }
}

/// Loopback and tunnel interfaces are skipped: a VPN's traffic also crosses the physical
/// interface, so counting its tunnel would count the same bytes twice.
pub fn counts_toward_network(interface: &str) -> bool {
    !["lo", "utun", "ipsec", "gif", "stf"]
        .iter()
        .any(|prefix| interface.starts_with(prefix))
}

struct Snapshot {
    at: Instant,
    counters: Counters,
}

pub struct ActivitySampler {
    /// `None` where sysinfo can't read this system (it supports macOS, and Linux for CI);
    /// the readings it gives are then unavailable, never zero.
    system: Option<System>,
    networks: Option<Networks>,
    disks: Option<Disks>,
    last_cpu_refresh: Instant,
    network: Option<Snapshot>,
    disk: Option<Snapshot>,
}

const NOT_REPORTED: UnavailableReason = UnavailableReason::NotReported;

impl ActivitySampler {
    pub fn new() -> Self {
        let mut system = System::new().ok();
        if let Some(system) = system.as_mut() {
            system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
        }
        Self {
            system,
            networks: Networks::new_with_refreshed_list().ok(),
            disks: Disks::new_with_refreshed_list_specifics(
                DiskRefreshKind::nothing().with_io_usage(),
            )
            .ok(),
            last_cpu_refresh: Instant::now(),
            network: None,
            disk: None,
        }
    }

    pub fn sample(&mut self) -> SystemActivity {
        let cpu = self.sample_cpu();
        let (memory, swap) = match self.system.as_mut() {
            None => (
                Reading::Unavailable {
                    reason: NOT_REPORTED,
                },
                Reading::Unavailable {
                    reason: NOT_REPORTED,
                },
            ),
            Some(system) => {
                system
                    .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
                let total = system.total_memory();
                let memory = if total == 0 {
                    Reading::Unavailable {
                        reason: NOT_REPORTED,
                    }
                } else {
                    Reading::Available {
                        value: MemoryReading {
                            total_bytes: total,
                            used_bytes: system.used_memory().min(total),
                            available_bytes: system.available_memory().min(total),
                        },
                    }
                };
                // A Mac that hasn't swapped yet genuinely reports 0 of 0, so zero is a real
                // reading.
                let swap_total = system.total_swap();
                let swap = Reading::Available {
                    value: SwapReading {
                        total_bytes: swap_total,
                        used_bytes: system.used_swap().min(swap_total),
                    },
                };
                (memory, swap)
            }
        };
        SystemActivity {
            sampled_at_ms: super::unix_time_ms(),
            cpu,
            memory,
            swap,
            memory_pressure: memory_pressure_from_level(sysctl::integer(
                "kern.memorystatus_vm_pressure_level",
            )),
            thermal_state: read_thermal_state(),
            battery_temperature: read_battery_temperature(),
            battery: map_battery(&read_battery()),
            network: self.sample_network(),
            disk: self.sample_disk(),
        }
    }

    fn sample_cpu(&mut self) -> Reading<CpuReading> {
        let Some(system) = self.system.as_mut() else {
            return Reading::Unavailable {
                reason: NOT_REPORTED,
            };
        };
        // The constructor takes the baseline sample; usage needs a spaced second one.
        let elapsed = self.last_cpu_refresh.elapsed();
        if elapsed < MINIMUM_CPU_UPDATE_INTERVAL {
            return Reading::Unavailable {
                reason: UnavailableReason::WarmingUp,
            };
        }
        system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
        self.last_cpu_refresh = Instant::now();
        // After a pause, a hidden window or a late first visit, the delta would be an average
        // over minutes; that refresh only becomes the new baseline.
        if !is_live_window(elapsed) {
            return Reading::Unavailable {
                reason: UnavailableReason::WarmingUp,
            };
        }
        let usage = system.global_cpu_usage();
        if !usage.is_finite() {
            return Reading::Unavailable {
                reason: UnavailableReason::NotReported,
            };
        }
        Reading::Available {
            value: CpuReading {
                usage_percent: f64::from(usage).clamp(0.0, 100.0),
                window_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            },
        }
    }

    fn sample_network(&mut self) -> Reading<NetworkRates> {
        let Some(networks) = self.networks.as_mut() else {
            return Reading::Unavailable {
                reason: NOT_REPORTED,
            };
        };
        networks.refresh(true);
        let counters = networks
            .iter()
            .filter(|(name, _)| counts_toward_network(&name.to_string_lossy()))
            .fold(
                Counters {
                    first: 0,
                    second: 0,
                },
                |sum, (_, data)| Counters {
                    first: sum.first.saturating_add(data.total_received()),
                    second: sum.second.saturating_add(data.total_transmitted()),
                },
            );
        let now = Instant::now();
        let previous = self.network.replace(Snapshot { at: now, counters });
        match previous.and_then(|previous| window_and_rates(&previous, now, counters)) {
            Some((window_ms, (received, transmitted))) => Reading::Available {
                value: NetworkRates {
                    received_bytes_per_second: received,
                    transmitted_bytes_per_second: transmitted,
                    window_ms,
                },
            },
            None => Reading::Unavailable {
                reason: UnavailableReason::WarmingUp,
            },
        }
    }

    /// Throughput of the physical disk behind the startup volume. Every APFS volume on that
    /// disk reports the same disk-wide counters, so only the root volume is read.
    fn sample_disk(&mut self) -> Reading<DiskRates> {
        let Some(disks) = self.disks.as_mut() else {
            return Reading::Unavailable {
                reason: NOT_REPORTED,
            };
        };
        disks.refresh_specifics(true, DiskRefreshKind::nothing().with_io_usage());
        let Some(root) = disks
            .iter()
            .find(|disk| disk.mount_point() == Path::new("/"))
        else {
            self.disk = None;
            return Reading::Unavailable {
                reason: UnavailableReason::NotFound,
            };
        };
        let usage = root.usage();
        let counters = Counters {
            first: usage.total_read_bytes,
            second: usage.total_written_bytes,
        };
        let now = Instant::now();
        let previous = self.disk.replace(Snapshot { at: now, counters });
        match previous.and_then(|previous| window_and_rates(&previous, now, counters)) {
            Some((window_ms, (read, written))) => Reading::Available {
                value: DiskRates {
                    read_bytes_per_second: read,
                    written_bytes_per_second: written,
                    window_ms,
                },
            },
            None => Reading::Unavailable {
                reason: UnavailableReason::WarmingUp,
            },
        }
    }
}

fn window_and_rates(
    previous: &Snapshot,
    now: Instant,
    counters: Counters,
) -> Option<(u64, (f64, f64))> {
    let window = now.duration_since(previous.at);
    if !is_live_window(window) {
        return None;
    }
    let window_ms = u64::try_from(window.as_millis()).unwrap_or(u64::MAX);
    rates(previous.counters, counters, window_ms).map(|rates| (window_ms, rates))
}

/// The longest window a reading is shown for as "now". Readings are polled every second on
/// the System page and every 5 s on the Dashboard, and macOS stretches an inactive window's
/// timers (App Nap), so this has room for both; an average over minutes (after a pause or a
/// hidden window) is still never shown.
pub const MAX_LIVE_WINDOW: Duration = Duration::from_secs(15);

/// Whether a sampling window is short enough to present as a current reading.
pub fn is_live_window(window: Duration) -> bool {
    window <= MAX_LIVE_WINDOW
}

impl Default for ActivitySampler {
    fn default() -> Self {
        Self::new()
    }
}

/// `AppleSmartBattery`'s `Temperature`, in hundredths of a degree Celsius.
pub fn read_battery_temperature() -> Reading<f64> {
    match super::registry::integers("AppleSmartBattery", &["Temperature"]) {
        None => Reading::Unavailable {
            reason: UnavailableReason::NotPresent,
        },
        Some(values) => battery_celsius(values.first().copied().flatten()),
    }
}

/// A plausible battery temperature from the sensor's hundredths of a degree, else "not reported".
pub fn battery_celsius(hundredths: Option<i64>) -> Reading<f64> {
    #[allow(clippy::cast_precision_loss)]
    match hundredths.map(|value| value as f64 / 100.0) {
        Some(celsius) if (-20.0..=90.0).contains(&celsius) => Reading::Available { value: celsius },
        _ => Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        },
    }
}

#[cfg(target_os = "macos")]
fn read_thermal_state() -> Reading<ThermalState> {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

    let state = NSProcessInfo::processInfo().thermalState();
    let value = if state == NSProcessInfoThermalState::Nominal {
        ThermalState::Nominal
    } else if state == NSProcessInfoThermalState::Fair {
        ThermalState::Fair
    } else if state == NSProcessInfoThermalState::Serious {
        ThermalState::Serious
    } else if state == NSProcessInfoThermalState::Critical {
        ThermalState::Critical
    } else {
        return Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        };
    };
    Reading::Available { value }
}

#[cfg(not(target_os = "macos"))]
fn read_thermal_state() -> Reading<ThermalState> {
    Reading::Unavailable {
        reason: UnavailableReason::NotReported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_and_app_nap_windows_still_read_as_now() {
        // The Dashboard polls every 5 s and App Nap stretches an inactive window's timers:
        // both used to come back as "measuring" forever.
        assert!(is_live_window(Duration::from_secs(1)));
        assert!(is_live_window(Duration::from_secs(5)));
        assert!(is_live_window(Duration::from_secs(12)));
        // An average over a pause or a hidden window is never shown as now.
        assert!(!is_live_window(Duration::from_secs(16)));
        assert!(!is_live_window(Duration::from_secs(120)));
    }

    #[test]
    fn the_battery_sensor_reads_in_hundredths_of_a_degree() {
        assert_eq!(
            battery_celsius(Some(3509)),
            Reading::Available { value: 35.09 }
        );
        assert!(matches!(
            battery_celsius(Some(65_535)),
            Reading::Unavailable { .. }
        ));
        assert!(matches!(battery_celsius(None), Reading::Unavailable { .. }));
    }
}
