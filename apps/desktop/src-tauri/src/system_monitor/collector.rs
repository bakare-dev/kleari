//! OS adapter: reads raw values through `sysinfo` without shell execution or privileges.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Pid, ProcessRefreshKind,
    ProcessesToUpdate, System, UpdateKind, MINIMUM_CPU_UPDATE_INTERVAL,
};

use super::battery::read_battery;
use super::processes::{self, CpuBaseline, ProcessSummary, RawProcess};
use super::{unix_time_ms, RawCpu, RawDisk, RawSample, Reading, UnavailableReason};

/// Derived app names per process identity `(pid, start_time)`. Executable paths are never
/// kept: only the name derived from them.
type AppNames = HashMap<(u32, u64), Option<String>>;

/// Keeps the previous CPU samples so usage can be computed as a delta between two refreshes.
pub struct SystemSampler {
    /// `None` where sysinfo can't read this system (it supports macOS, and Linux for CI); its
    /// readings are then unavailable (no CPU, memory total 0, no disk, no processes).
    system: Option<System>,
    disks: Option<Disks>,
    last_cpu_refresh: Instant,
    last_cpu: Option<RawCpu>,
    process_baseline: CpuBaseline,
    last_processes: Reading<ProcessSummary>,
    app_names: AppNames,
}

impl SystemSampler {
    /// Takes the initial CPU and process samples. CPU readings become available once
    /// `MINIMUM_CPU_UPDATE_INTERVAL` has elapsed; the memory ranking is available at once.
    pub fn new() -> Self {
        let mut app_names = AppNames::new();
        let mut system = System::new().ok();
        let (process_baseline, last_processes) = match system.as_mut() {
            Some(system) => {
                system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
                let current = read_processes(system, &mut app_names);
                (
                    processes::baseline_of(&current),
                    processes::summarize(&current, None, system.cpus().len()),
                )
            }
            None => (
                CpuBaseline::new(),
                Reading::Unavailable {
                    reason: UnavailableReason::NotReported,
                },
            ),
        };
        Self {
            system,
            disks: Disks::new().ok(),
            last_cpu_refresh: Instant::now(),
            last_cpu: None,
            process_baseline,
            last_processes,
            app_names,
        }
    }

    pub fn sample(&mut self) -> RawSample {
        self.refresh_cpu();
        let (memory_total, memory_used, memory_available) = match self.system.as_mut() {
            Some(system) => {
                system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
                (
                    system.total_memory(),
                    system.used_memory(),
                    system.available_memory(),
                )
            }
            // A total of 0 is reported as unavailable, never as zero memory.
            None => (0, 0, 0),
        };
        let startup_disk = self.disks.as_mut().and_then(|disks| {
            disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            disks
                .iter()
                .find(|disk| disk.mount_point() == Path::new("/"))
                .map(|disk| RawDisk {
                    total: disk.total_space(),
                    available: disk.available_space(),
                })
        });

        RawSample {
            sampled_at_ms: unix_time_ms(),
            cpu: self.last_cpu,
            memory_total,
            memory_used,
            memory_available,
            startup_disk,
            battery: read_battery(),
            processes: self.last_processes.clone(),
        }
    }

    /// Refreshes CPU usage only when enough time has passed for a meaningful delta;
    /// otherwise the previous reading, which is still accurate for its window, is kept.
    fn refresh_cpu(&mut self) {
        let Some(system) = self.system.as_mut() else {
            return;
        };
        let elapsed = self.last_cpu_refresh.elapsed();
        if elapsed < MINIMUM_CPU_UPDATE_INTERVAL {
            return;
        }
        system.refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
        self.last_cpu_refresh = Instant::now();
        let window_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        self.last_cpu = Some(RawCpu {
            usage_percent: system.global_cpu_usage(),
            window_ms,
        });

        // Processes share the CPU window so per-process and total CPU cover the same span.
        let current = read_processes(system, &mut self.app_names);
        self.last_processes = processes::summarize(
            &current,
            Some((&self.process_baseline, window_ms)),
            system.cpus().len(),
        );
        self.process_baseline = processes::baseline_of(&current);
    }
}

fn read_processes(system: &mut System, app_names: &mut AppNames) -> Vec<RawProcess> {
    // The long-lived instance never requests executable paths, so none are cached here.
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    let unseen: Vec<Pid> = system
        .processes()
        .values()
        .filter(|process| !app_names.contains_key(&(process.pid().as_u32(), process.start_time())))
        .map(sysinfo::Process::pid)
        .collect();
    if !unseen.is_empty() {
        app_names.extend(resolve_app_names(&unseen));
    }
    let live: HashSet<(u32, u64)> = system
        .processes()
        .values()
        .map(|process| (process.pid().as_u32(), process.start_time()))
        .collect();
    app_names.retain(|identity, _| live.contains(identity));

    system
        .processes()
        .values()
        .map(|process| {
            let identity = (process.pid().as_u32(), process.start_time());
            RawProcess {
                pid: identity.0,
                name: process.name().to_string_lossy().into_owned(),
                start_time: identity.1,
                cpu_time_ms: process.accumulated_cpu_time(),
                memory: process.memory(),
                app: app_names.get(&identity).cloned().flatten(),
            }
        })
        .collect()
}

/// Derives app names for newly seen processes from their executable paths using a
/// short-lived `System`, so the paths are dropped on return instead of staying in the
/// sampler's cache. A process's executable does not change, so each is resolved once.
fn resolve_app_names(pids: &[Pid]) -> AppNames {
    // Where sysinfo can't read this system, no names are found; they are looked up again
    // with the next sample.
    let Ok(mut lookup) = System::new() else {
        return AppNames::new();
    };
    lookup.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        false,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    lookup
        .processes()
        .values()
        .map(|process| {
            (
                (process.pid().as_u32(), process.start_time()),
                process.exe().and_then(processes::app_name_from_exe),
            )
        })
        .collect()
}

impl Default for SystemSampler {
    fn default() -> Self {
        Self::new()
    }
}
