//! Read-only top-process summary. The name, PID, CPU and memory of each process are read,
//! plus the executable path, which is used only here to derive the containing app's name
//! and is never sent to the frontend, stored or logged. No command line or user is read.
//!
//! Per-process CPU is computed here from cumulative CPU time across our own sample window
//! rather than taken from `sysinfo`'s `cpu_usage`, which keeps its previous value when a
//! process did no work since the last refresh and would leave idle processes ranked high.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Reading, UnavailableReason};

pub const TOP_COUNT: usize = 5;
const MAX_NAME_CHARS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessReading {
    pub pid: u32,
    pub name: String,
    /// Share of one logical CPU over the sample window, as in Activity Monitor, so it can
    /// exceed 100. `None` until a process has two samples.
    pub cpu_percent: Option<f64>,
    pub memory_bytes: u64,
}

/// Processes grouped by the app bundle they run from, or by name when outside any bundle.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppReading {
    pub name: String,
    pub process_count: u32,
    /// Sum over members that have a CPU reading; `None` when no member has one yet.
    pub cpu_percent: Option<f64>,
    /// Members included in `cpu_percent`. Fewer than `process_count` means the sum is a
    /// lower bound because some members were first seen in this sample.
    pub cpu_measured_count: u32,
    /// Sum of members' resident memory, which can count shared memory more than once.
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSummary {
    pub top_by_cpu: Vec<ProcessReading>,
    pub top_by_memory: Vec<ProcessReading>,
    pub top_apps_by_cpu: Vec<AppReading>,
    pub top_apps_by_memory: Vec<AppReading>,
    pub readable_count: u32,
    /// Processes with no readable memory value (reported as zero), excluded from ranking.
    /// Usually ones macOS withholds from unprivileged apps; the cause is not checked per
    /// process.
    pub unreadable_count: u32,
    /// `None` until a second sample exists, so CPU ranking is not yet available.
    pub cpu_window_ms: Option<u64>,
}

/// Raw per-process values from the OS adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    pub pid: u32,
    pub name: String,
    /// Process start time; together with the PID it detects PID reuse between samples.
    pub start_time: u64,
    /// Cumulative CPU time in milliseconds.
    pub cpu_time_ms: u64,
    pub memory: u64,
    /// Name of the outermost `.app` bundle containing the executable, if any.
    pub app: Option<String>,
}

/// Returns the outermost app bundle name in an executable path, so helpers such as
/// `…/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/…` group under
/// "Visual Studio Code". A component counts as a bundle only when it is `Name.app`
/// immediately followed by `Contents` (the macOS bundle layout), so an ordinary directory
/// such as `/Users/x/my.app/bin/tool` is not treated as an app. Other layouts without
/// `Contents` (for example iPhone and iPad apps running on Apple silicon) are deliberately
/// not recognized and fall back to grouping by process name. The path itself is not kept.
pub fn app_name_from_exe(path: &Path) -> Option<String> {
    let bundle = app_bundle_from_exe(path)?;
    let name = bundle.file_name()?.to_str()?.strip_suffix(".app")?;
    Some(name.to_owned())
}

/// The outermost app bundle containing an executable, by the same rule as
/// [`app_name_from_exe`]: a `Name.app` component immediately followed by `Contents`.
pub fn app_bundle_from_exe(path: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<_>>()?;
    let index = parts.windows(2).position(|pair| {
        pair[0]
            .strip_suffix(".app")
            .is_some_and(|name| pair[1] == "Contents" && !name.trim().is_empty())
    })?;
    Some(parts[..=index].iter().collect())
}

/// Group identity: app bundles and bare process names never merge with each other.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum GroupKey {
    App(String),
    Process(String),
}

/// Cumulative CPU time per PID from the previous sample: `pid -> (start_time, cpu_time_ms)`.
pub type CpuBaseline = HashMap<u32, (u64, u64)>;

pub fn baseline_of(processes: &[RawProcess]) -> CpuBaseline {
    processes
        .iter()
        .map(|process| (process.pid, (process.start_time, process.cpu_time_ms)))
        .collect()
}

/// Ranks readable processes. `previous` is the prior baseline and the elapsed window in
/// milliseconds; without it, only the memory ranking is available.
pub fn summarize(
    current: &[RawProcess],
    previous: Option<(&CpuBaseline, u64)>,
    logical_cpus: usize,
) -> Reading<ProcessSummary> {
    let readable_raw: Vec<&RawProcess> = current
        .iter()
        .filter(|process| process.memory > 0)
        .collect();
    let readable: Vec<ProcessReading> = readable_raw
        .iter()
        .map(|process| ProcessReading {
            pid: process.pid,
            name: display_name(&process.name),
            cpu_percent: previous.and_then(|(baseline, window_ms)| {
                cpu_percent(process, baseline, window_ms, logical_cpus)
            }),
            memory_bytes: process.memory,
        })
        .collect();
    if readable.is_empty() {
        return Reading::Unavailable {
            reason: UnavailableReason::NotReported,
        };
    }

    let mut top_by_cpu: Vec<ProcessReading> = readable
        .iter()
        .filter(|process| process.cpu_percent.is_some())
        .cloned()
        .collect();
    top_by_cpu.sort_by(|a, b| {
        let (a_cpu, b_cpu) = (a.cpu_percent.unwrap_or(0.0), b.cpu_percent.unwrap_or(0.0));
        b_cpu
            .total_cmp(&a_cpu)
            .then(b.memory_bytes.cmp(&a.memory_bytes))
            .then(a.pid.cmp(&b.pid))
    });
    top_by_cpu.truncate(TOP_COUNT);

    let mut top_by_memory = readable.clone();
    top_by_memory.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes).then(a.pid.cmp(&b.pid)));
    top_by_memory.truncate(TOP_COUNT);

    let apps = group_by_app(&readable_raw, &readable, logical_cpus);
    let mut top_apps_by_cpu: Vec<AppReading> = apps
        .iter()
        .filter(|app| app.cpu_percent.is_some())
        .cloned()
        .collect();
    top_apps_by_cpu.sort_by(|a, b| {
        let (a_cpu, b_cpu) = (a.cpu_percent.unwrap_or(0.0), b.cpu_percent.unwrap_or(0.0));
        b_cpu
            .total_cmp(&a_cpu)
            .then(b.memory_bytes.cmp(&a.memory_bytes))
            .then(a.name.cmp(&b.name))
    });
    top_apps_by_cpu.truncate(TOP_COUNT);
    let mut top_apps_by_memory = apps;
    top_apps_by_memory.sort_by(|a, b| {
        b.memory_bytes
            .cmp(&a.memory_bytes)
            .then(a.name.cmp(&b.name))
    });
    top_apps_by_memory.truncate(TOP_COUNT);

    Reading::Available {
        value: ProcessSummary {
            top_by_cpu,
            top_by_memory,
            top_apps_by_cpu,
            top_apps_by_memory,
            readable_count: count(readable.len()),
            unreadable_count: count(current.len() - readable.len()),
            cpu_window_ms: previous.map(|(_, window_ms)| window_ms),
        },
    }
}

/// `raw` and `readings` are parallel: `readings[i]` was derived from `raw[i]`.
fn group_by_app(
    raw: &[&RawProcess],
    readings: &[ProcessReading],
    logical_cpus: usize,
) -> Vec<AppReading> {
    let ceiling = logical_cpus.max(1) as f64 * 100.0;
    let mut groups: HashMap<GroupKey, AppReading> = HashMap::new();
    for (process, reading) in raw.iter().zip(readings) {
        let key = match &process.app {
            Some(app) => GroupKey::App(display_name(app)),
            None => GroupKey::Process(reading.name.clone()),
        };
        let name = match &key {
            GroupKey::App(name) | GroupKey::Process(name) => name.clone(),
        };
        let group = groups.entry(key).or_insert_with(|| AppReading {
            name,
            process_count: 0,
            cpu_percent: None,
            cpu_measured_count: 0,
            memory_bytes: 0,
        });
        group.process_count = group.process_count.saturating_add(1);
        group.memory_bytes = group.memory_bytes.saturating_add(reading.memory_bytes);
        if let Some(cpu) = reading.cpu_percent {
            group.cpu_percent = Some((group.cpu_percent.unwrap_or(0.0) + cpu).min(ceiling));
            group.cpu_measured_count = group.cpu_measured_count.saturating_add(1);
        }
    }
    groups.into_values().collect()
}

pub(super) fn cpu_percent(
    process: &RawProcess,
    baseline: &CpuBaseline,
    window_ms: u64,
    logical_cpus: usize,
) -> Option<f64> {
    if window_ms == 0 {
        return None;
    }
    let &(start_time, previous_ms) = baseline.get(&process.pid)?;
    if start_time != process.start_time {
        return None;
    }
    let delta_ms = process.cpu_time_ms.checked_sub(previous_ms)?;
    // Millisecond counters over a seconds-long window are far below f64's exact range.
    let percent = delta_ms as f64 * 100.0 / window_ms as f64;
    let ceiling = logical_cpus.max(1) as f64 * 100.0;
    Some(percent.min(ceiling))
}

pub(super) fn display_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.chars().count() <= MAX_NAME_CHARS {
        return trimmed.to_owned();
    }
    let mut short: String = trimmed.chars().take(MAX_NAME_CHARS - 1).collect();
    short.push('…');
    short
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
