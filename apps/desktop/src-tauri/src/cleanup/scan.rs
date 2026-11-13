//! The Cleanup page's read-only scan. Three modes, each a fixed list of steps:
//! - Standard: app caches, logs and temporary files, package caches.
//! - Deep: Standard plus large and old files in the folders you granted (inspect only).
//! - Developer: Xcode data, simulators, `node_modules` in your projects, Docker (read-only).
//!
//! Every location is fixed in Rust or was granted by the user; nothing comes from the
//! frontend but the mode and its options. Folders are opened from `/` without following a
//! symlink anywhere, children relative to their open parent, measured through handles on the
//! same volume (`developer::measure`). No file is opened except a simulator's small
//! `device.plist` for its name; nothing is written.

use std::ffi::{CString, OsStr};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::intake::LOCKFILES;
use super::policy::{self, Kind};
use super::{assess_now, new_id, Candidate, Source, StoredFacts};
use crate::developer::docker::{self, DockerState};
use crate::developer::projects;
use crate::developer::{catalog, measure_dir, open_absolute, Links, Measured};
use crate::quarantine::Identity;
use crate::storage::fsdir::{display_name, DirHandle, FileStat};
use crate::system_monitor::open_paths::OpenPaths;
use crate::system_monitor::running_apps::RunningBundle;
use crate::system_monitor::{display_path, non_local_mounts};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanMode {
    Standard,
    Deep,
    Dev,
}

impl ScanMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Deep => "deep",
            Self::Dev => "dev",
        }
    }
}

/// The options menu of each mode. Unknown fields and values outside the menu are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanOptions {
    pub logs: bool,
    pub temp: bool,
    pub log_age_days: u32,
    pub large_min_mb: u32,
    pub old_days: u32,
    pub hidden: bool,
    pub xcode: bool,
    pub sim: bool,
    pub node: bool,
    pub docker: bool,
}

impl Default for ScanOptions {
    /// The menu's defaults, as the Cleanup page shows them before anything is changed.
    fn default() -> Self {
        Self {
            logs: true,
            temp: true,
            log_age_days: 30,
            large_min_mb: 1_000,
            old_days: 180,
            hidden: false,
            xcode: true,
            sim: true,
            node: true,
            docker: true,
        }
    }
}

impl ScanOptions {
    /// Only the values the menu offers.
    pub fn valid(&self) -> bool {
        [7, 30, 90].contains(&self.log_age_days)
            && [500, 1_000, 5_000].contains(&self.large_min_mb)
            && [90, 180, 365].contains(&self.old_days)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepId {
    AppCaches,
    LogsAndTemp,
    PackageCaches,
    LargeAndOld,
    Xcode,
    Simulators,
    NodeModules,
    Docker,
}

/// The steps a mode runs with these options, in order.
pub fn steps(mode: ScanMode, options: &ScanOptions) -> Vec<StepId> {
    let mut steps = Vec::new();
    match mode {
        ScanMode::Standard | ScanMode::Deep => {
            steps.push(StepId::AppCaches);
            if options.logs || options.temp {
                steps.push(StepId::LogsAndTemp);
            }
            steps.push(StepId::PackageCaches);
            if mode == ScanMode::Deep {
                steps.push(StepId::LargeAndOld);
            }
        }
        ScanMode::Dev => {
            for (on, step) in [
                (options.xcode, StepId::Xcode),
                (options.sim, StepId::Simulators),
                (options.node, StepId::NodeModules),
                (options.docker, StepId::Docker),
            ] {
                if on {
                    steps.push(step);
                }
            }
        }
    }
    steps
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub id: StepId,
    pub items: u32,
    pub bytes: u64,
    /// Did the step run to the end (not stopped by a cancel)?
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Index of the step in progress.
    pub step: u32,
    /// Items it found so far.
    pub items: u32,
    /// Their size so far.
    pub bytes: u64,
}

/// How often a step reports what it has found so far.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

/// Where and how to scan. `large_roots` are the granted folders searched for large and old
/// files; `project_roots` are searched for `node_modules`.
pub struct Context<'a> {
    pub home: &'a Path,
    pub temp: Option<&'a Path>,
    pub running: &'a [RunningBundle],
    /// What is open now; `None` (couldn't be read) counts every project as in use.
    pub open: Option<&'a OpenPaths>,
    pub now_ms: u64,
    pub options: ScanOptions,
    /// The quarantine folder's volume; items elsewhere are inspect only.
    pub store_dev: Option<u64>,
    pub large_roots: &'a [PathBuf],
    pub project_roots: &'a [PathBuf],
    pub cancel: &'a AtomicBool,
    /// Which findings to keep (the user's exclusions and minimum size); applied at the end
    /// of each step, so the step's totals count only what is kept.
    pub keep: &'a dyn Fn(&Candidate) -> bool,
    /// Folders the user excluded in Settings: never listed, entered or measured.
    pub excluded: &'a [PathBuf],
}

pub struct Outcome {
    pub cancelled: bool,
    pub steps: Vec<StepResult>,
    pub items: Vec<Candidate>,
    /// Folders or items macOS refused to open: some need Full Disk Access.
    pub denied: u64,
    pub docker: Option<DockerState>,
    /// Granted or added folders that couldn't be searched, shown as `~/…`.
    pub skipped_roots: Vec<String>,
}

/// Caches and logs smaller than this (an empty folder's metadata) aren't listed.
const MIN_BYTES: u64 = 1_000_000;
/// Temporary files must be this many days untouched.
pub const TEMP_IDLE_DAYS: u64 = 7;
/// Old files are listed only from this size.
const OLD_MIN_BYTES: u64 = 100_000_000;
/// Large and old files kept, each.
const MAX_FILES: usize = 200;
/// Folders deep below a granted folder for large and old files.
const MAX_FILE_DEPTH: usize = 16;
/// Crash reports kept, oldest first.
const MAX_CRASH_REPORTS: usize = 200;
const DAY_S: u64 = 86_400;

/// An open folder, its volume and its entries with their `lstat`.
type Listing = (DirHandle, u64, Vec<(CString, FileStat)>);

/// Gathers candidates for one scan.
struct Collector<'a> {
    ctx: &'a Context<'a>,
    items: Vec<Candidate>,
    links: Links,
    denied: u64,
    /// The step in progress and where its items start, for progress reports.
    step: u32,
    step_start: usize,
    last_report: std::time::Instant,
    report: &'a mut dyn FnMut(Progress),
}

fn denied_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES))
}

fn seconds_to_ms(seconds: i64) -> Option<u64> {
    u64::try_from(seconds)
        .ok()
        .filter(|&seconds| seconds > 0)
        .and_then(|seconds| seconds.checked_mul(1000))
}

impl<'a> Collector<'a> {
    fn new(ctx: &'a Context<'a>, report: &'a mut dyn FnMut(Progress)) -> Self {
        Self {
            ctx,
            items: Vec::new(),
            links: Links::default(),
            denied: 0,
            step: 0,
            step_start: 0,
            last_report: std::time::Instant::now(),
            report,
        }
    }

    /// What the current step has found so far.
    fn progress(&self) -> Progress {
        let found = &self.items[self.step_start..];
        Progress {
            step: self.step,
            items: u32::try_from(found.len()).unwrap_or(u32::MAX),
            bytes: found
                .iter()
                .map(|item| item.bytes)
                .fold(0, u64::saturating_add),
        }
    }

    fn start_step(&mut self, step: u32) {
        self.step = step;
        self.step_start = self.items.len();
        self.last_report = std::time::Instant::now();
        let progress = self.progress();
        (self.report)(progress);
    }

    /// Reports the current step's findings, at most every `PROGRESS_INTERVAL`.
    fn report_found(&mut self) {
        if self.last_report.elapsed() >= PROGRESS_INTERVAL {
            self.last_report = std::time::Instant::now();
            let progress = self.progress();
            (self.report)(progress);
        }
    }

    fn cancelled(&self) -> bool {
        self.ctx.cancel.load(Ordering::Relaxed)
    }

    /// Whether `path` is in a folder the user excluded, so it isn't read at all.
    fn excluded(&self, path: &Path) -> bool {
        crate::preferences::inside_excluded(path, self.ctx.excluded)
    }

    /// Whether a would-be finding at `path` is in an excluded folder or contains one; either
    /// way it is never measured or offered (measuring it would read the excluded folder).
    fn touches_excluded(&self, path: &Path) -> bool {
        crate::preferences::touches_excluded(path, self.ctx.excluded)
    }

    /// Drops the current step's findings the user's preferences leave out.
    fn keep_step(&mut self) {
        let (start, keep) = (self.step_start, self.ctx.keep);
        let mut index = 0;
        self.items.retain(|item| {
            let kept = index < start || keep(item);
            index += 1;
            kept
        });
    }

    fn idle_days(&self, seconds: i64) -> u64 {
        let now_s = self.ctx.now_ms / 1000;
        u64::try_from(seconds).map_or(0, |then| now_s.saturating_sub(then) / DAY_S)
    }

    /// Adds an item: classified by the policy, with its identity from `stat`. Returns it so a
    /// caller can give it a clearer name or its project.
    fn add(
        &mut self,
        kind: Kind,
        path: PathBuf,
        stat: &FileStat,
        measured: Option<&Measured>,
        facts: StoredFacts,
    ) -> &mut Candidate {
        let same_volume = self.ctx.store_dev.is_none_or(|dev| dev == stat.dev);
        let file_name = super::display_name_of(&path);
        // A project, and so whether it is in use, is known only for build folders, reassessed
        // once it is set.
        let assessment = assess_now(
            &kind,
            &file_name,
            facts,
            self.ctx.running,
            false,
            same_volume,
        );
        let (bytes, files, lower_bound) = measured
            .map_or((stat.blocks.saturating_mul(512), 1, false), |measured| {
                (measured.bytes, measured.files, measured.lower_bound)
            });
        let newest = measured.map_or(stat.modified_s, |measured| {
            measured.newest_modified_s.max(stat.modified_s)
        });
        let index = self.items.len();
        self.items.push(Candidate {
            id: new_id(self.ctx.now_ms),
            name: file_name,
            location: super::location_of(&path, Some(self.ctx.home)),
            identity: Identity::of(stat),
            kind,
            path,
            bytes,
            files,
            lower_bound,
            modified_ms: seconds_to_ms(newest),
            assessment,
            facts,
            source: Source::Scan,
            project: None,
        });
        self.report_found();
        &mut self.items[index]
    }

    /// The entries of the fixed folder `folder` (not symlinks), with the folder open. `None`
    /// if it doesn't exist or can't be opened safely; a refusal by macOS is counted.
    fn list(&mut self, folder: &Path) -> Option<Listing> {
        if self.excluded(folder) {
            return None;
        }
        let mounts = non_local_mounts()?;
        if mounts.iter().any(|mount| folder.starts_with(mount)) {
            return None;
        }
        let Some((mut dir, dev)) = open_absolute(folder) else {
            if std::fs::symlink_metadata(folder).is_ok()
                && DirHandle::open_root(folder)
                    .as_ref()
                    .is_err_and(denied_error)
            {
                self.denied += 1;
            }
            return None;
        };
        let names: Vec<CString> = std::iter::from_fn(|| dir.next_name())
            .filter_map(Result::ok)
            .collect();
        let mut entries = Vec::with_capacity(names.len());
        for name in names {
            // An excluded folder inside this one isn't even looked at, nor is an entry that
            // contains one (each entry may be measured whole).
            if self.touches_excluded(&folder.join(OsStr::from_bytes(name.to_bytes()))) {
                continue;
            }
            match dir.stat_child(&name) {
                Ok(stat) if !stat.is_symlink() => entries.push((name, stat)),
                Ok(_) => {}
                Err(error) if denied_error(&error) => self.denied += 1,
                Err(_) => {}
            }
        }
        Some((dir, dev, entries))
    }

    /// Measures the child folder `name` of `dir` through a handle (identity checked).
    fn measure(
        &mut self,
        dir: &DirHandle,
        name: &CString,
        stat: &FileStat,
        dev: u64,
    ) -> Option<Measured> {
        if !stat.is_dir() || stat.dev != dev {
            return None;
        }
        match dir.open_child(name) {
            Ok((child, opened)) if opened.dev == stat.dev && opened.ino == stat.ino => {
                Some(measure_dir(child, dev, &mut self.links, self.ctx.cancel))
            }
            Ok(_) => None,
            Err(error) => {
                if denied_error(&error) {
                    self.denied += 1;
                }
                None
            }
        }
    }

    fn app_caches(&mut self, packages: bool) {
        let caches = self.ctx.home.join("Library/Caches");
        let Some((dir, dev, entries)) = self.list(&caches) else {
            return;
        };
        for (name, stat) in entries {
            if self.cancelled() {
                return;
            }
            let shown = display_name(OsStr::from_bytes(name.to_bytes()));
            let is_package = policy::CACHES_PACKAGES.contains(&shown.as_str());
            if is_package != packages || !stat.is_dir() {
                continue;
            }
            let Some(measured) = self.measure(&dir, &name, &stat, dev) else {
                continue;
            };
            if measured.bytes < MIN_BYTES {
                continue;
            }
            let kind = if packages {
                Kind::CachesPackage
            } else {
                Kind::AppCache
            };
            self.add(
                kind,
                caches.join(OsStr::from_bytes(name.to_bytes())),
                &stat,
                Some(&measured),
                StoredFacts::default(),
            );
        }
    }

    /// Developer-catalog caches outside `~/Library/Caches` (those inside are listed by name).
    fn catalog_caches(&mut self) {
        let caches = self.ctx.home.join("Library/Caches");
        for cache in catalog::CACHES {
            if cache.id.starts_with("xcode") || cache.id.starts_with("simulator") {
                continue;
            }
            for &location in cache.paths {
                if self.cancelled() {
                    return;
                }
                let Some(path) = catalog::resolve(location, Some(self.ctx.home)) else {
                    continue;
                };
                if path.parent() == Some(caches.as_path()) {
                    continue;
                }
                if let Some((stat, measured)) = self.measure_path(&path) {
                    if measured.bytes >= MIN_BYTES {
                        self.add(
                            Kind::Catalog(cache.id),
                            path,
                            &stat,
                            Some(&measured),
                            StoredFacts::default(),
                        );
                    }
                }
            }
        }
    }

    /// Opens and measures one fixed folder by its absolute path.
    fn measure_path(&mut self, path: &Path) -> Option<(FileStat, Measured)> {
        if self.touches_excluded(path) {
            return None;
        }
        let mounts = non_local_mounts()?;
        if mounts.iter().any(|mount| path.starts_with(mount)) {
            return None;
        }
        let (parent, name) = crate::quarantine::split(path)?;
        let (dir, dev) = open_absolute(parent)?;
        let stat = dir.stat_child(&name).ok()?;
        let measured = self.measure(&dir, &name, &stat, dev)?;
        Some((stat, measured))
    }

    fn logs_and_temp(&mut self) {
        let options = self.ctx.options;
        if options.logs {
            let logs = self.ctx.home.join("Library/Logs");
            self.aged_children(
                &logs,
                u64::from(options.log_age_days),
                Kind::Log,
                &["DiagnosticReports"],
            );
            self.crash_reports(
                &logs.join("DiagnosticReports"),
                u64::from(options.log_age_days),
            );
        }
        if options.temp {
            if let Some(temp) = self.ctx.temp {
                self.aged_children(temp, TEMP_IDLE_DAYS, Kind::Temp, &[]);
            }
        }
    }

    /// Children of `folder` in which nothing changed for `min_days`.
    fn aged_children(&mut self, folder: &Path, min_days: u64, kind: Kind, skip: &[&str]) {
        let Some((dir, dev, entries)) = self.list(folder) else {
            return;
        };
        for (name, stat) in entries {
            if self.cancelled() {
                return;
            }
            let shown = display_name(OsStr::from_bytes(name.to_bytes()));
            if skip.contains(&shown.as_str()) || shown.starts_with("com.apple.") || stat.dev != dev
            {
                continue;
            }
            let measured = if stat.is_dir() {
                match self.measure(&dir, &name, &stat, dev) {
                    Some(measured) => Some(measured),
                    None => continue,
                }
            } else if stat.is_file() {
                None
            } else {
                continue;
            };
            let newest = measured.as_ref().map_or(stat.modified_s, |measured| {
                measured.newest_modified_s.max(stat.modified_s)
            });
            let idle = self.idle_days(newest);
            let bytes = measured
                .as_ref()
                .map_or(stat.blocks.saturating_mul(512), |measured| measured.bytes);
            if idle < min_days
                || bytes == 0
                || measured
                    .as_ref()
                    .is_some_and(|measured| measured.lower_bound)
            {
                continue;
            }
            let facts = StoredFacts {
                idle_days: idle,
                ..StoredFacts::default()
            };
            self.add(
                kind.clone(),
                folder.join(OsStr::from_bytes(name.to_bytes())),
                &stat,
                measured.as_ref(),
                facts,
            );
        }
    }

    fn crash_reports(&mut self, folder: &Path, min_days: u64) {
        let Some((_, dev, mut entries)) = self.list(folder) else {
            return;
        };
        entries.retain(|(_, stat)| stat.is_file() && stat.dev == dev);
        entries.sort_by_key(|(_, stat)| stat.modified_s);
        for (name, stat) in entries.into_iter().take(MAX_CRASH_REPORTS) {
            let idle = self.idle_days(stat.modified_s);
            if idle < min_days {
                continue;
            }
            let facts = StoredFacts {
                idle_days: idle,
                ..StoredFacts::default()
            };
            self.add(
                Kind::CrashReport,
                folder.join(OsStr::from_bytes(name.to_bytes())),
                &stat,
                None,
                facts,
            );
        }
    }

    fn xcode(&mut self) {
        let xcode = self.ctx.home.join("Library/Developer/Xcode");
        self.children_as(&xcode.join("DerivedData"), Kind::DerivedData);
        self.children_as(&xcode.join("Archives"), Kind::Archive);
        let Some((_, _, entries)) = self.list(&xcode) else {
            return;
        };
        for (name, stat) in entries {
            let shown = display_name(OsStr::from_bytes(name.to_bytes()));
            if stat.is_dir() && shown.ends_with(" DeviceSupport") {
                self.children_as(
                    &xcode.join(OsStr::from_bytes(name.to_bytes())),
                    Kind::DeviceSupport,
                );
            }
        }
    }

    /// Each folder directly in `folder` as an item of `kind`.
    fn children_as(&mut self, folder: &Path, kind: Kind) {
        let Some((dir, dev, entries)) = self.list(folder) else {
            return;
        };
        for (name, stat) in entries {
            if self.cancelled() {
                return;
            }
            let Some(measured) = self.measure(&dir, &name, &stat, dev) else {
                continue;
            };
            if measured.bytes < MIN_BYTES {
                continue;
            }
            self.add(
                kind.clone(),
                folder.join(OsStr::from_bytes(name.to_bytes())),
                &stat,
                Some(&measured),
                StoredFacts::default(),
            );
        }
    }

    fn simulators(&mut self) {
        let simulator = self.ctx.home.join("Library/Developer/CoreSimulator");
        let caches = simulator.join("Caches");
        if let Some((stat, measured)) = self.measure_path(&caches) {
            if measured.bytes >= MIN_BYTES {
                self.add(
                    Kind::SimulatorCache,
                    caches,
                    &stat,
                    Some(&measured),
                    StoredFacts::default(),
                )
                .name = "Simulator caches".to_owned();
            }
        }
        let devices = simulator.join("Devices");
        let Some((dir, dev, entries)) = self.list(&devices) else {
            return;
        };
        for (name, stat) in entries {
            if self.cancelled() {
                return;
            }
            let device_name = device_name(&dir, &name, dev);
            let Some(measured) = self.measure(&dir, &name, &stat, dev) else {
                continue;
            };
            if measured.bytes < MIN_BYTES {
                continue;
            }
            let item = self.add(
                Kind::SimulatorDevice,
                devices.join(OsStr::from_bytes(name.to_bytes())),
                &stat,
                Some(&measured),
                StoredFacts::default(),
            );
            if let Some(device_name) = device_name {
                item.name = device_name;
            }
        }
    }

    fn node_modules(&mut self, skipped: &mut Vec<String>) {
        let home = self.ctx.home;
        // Excluded folders are never searched, nor are roots inside them.
        let skip: Vec<PathBuf> = [home.join("Library"), home.join(".Trash")]
            .into_iter()
            .chain(self.ctx.excluded.iter().cloned())
            .collect();
        let roots: Vec<PathBuf> = self
            .ctx
            .project_roots
            .iter()
            .filter(|root| !self.excluded(root))
            .cloned()
            .collect();
        let mut found: Vec<(PathBuf, FileStat, Measured, bool)> = Vec::new();
        let links = &mut self.links;
        let cancel = self.ctx.cancel;
        // The search can take a while; what it found so far is reported as it goes.
        let report = &mut self.report;
        let step = self.step;
        let mut last_report = std::time::Instant::now();
        let mut found_bytes = 0_u64;
        let excluded = self.ctx.excluded;
        let discovery = projects::discover(
            &roots,
            &skip,
            projects::Limits::default(),
            cancel,
            non_local_mounts,
            |project, dir, dev| {
                // A node_modules that contains an excluded folder isn't measured.
                if crate::preferences::touches_excluded(
                    &project.path.join("node_modules"),
                    excluded,
                ) {
                    return;
                }
                if !project
                    .outputs
                    .iter()
                    .any(|output| output == "node_modules")
                {
                    return;
                }
                let Ok(stat) = dir.stat_child(c"node_modules") else {
                    return;
                };
                if !stat.is_dir() || stat.dev != dev {
                    return;
                }
                let Ok((child, opened)) = dir.open_child(c"node_modules") else {
                    return;
                };
                if opened.ino != stat.ino || opened.dev != stat.dev {
                    return;
                }
                let lockfile = LOCKFILES
                    .iter()
                    .any(|lock| dir.stat_child(lock).is_ok_and(|lock| lock.is_file()));
                let measured = measure_dir(child, dev, links, cancel);
                if measured.bytes > 0 {
                    found_bytes = found_bytes.saturating_add(measured.bytes);
                    found.push((project.path, stat, measured, lockfile));
                    if last_report.elapsed() >= PROGRESS_INTERVAL {
                        last_report = std::time::Instant::now();
                        report(Progress {
                            step,
                            items: u32::try_from(found.len()).unwrap_or(u32::MAX),
                            bytes: found_bytes,
                        });
                    }
                }
            },
        );
        skipped.extend(
            discovery
                .skipped_roots
                .iter()
                .map(|root| display_path(root, Some(home))),
        );
        for (project, stat, measured, lockfile) in found {
            let project_name = super::display_name_of(&project);
            // In a workspace the lockfile is at the workspace root, above the package.
            let lockfile =
                lockfile || super::intake::has_lockfile(&project, home, self.ctx.excluded);
            let facts = StoredFacts {
                lockfile,
                ..StoredFacts::default()
            };
            let kind = Kind::BuildFolder {
                name: "node_modules".to_owned(),
            };
            let (running, open) = (self.ctx.running, self.ctx.open);
            let same_volume = self.ctx.store_dev.is_none_or(|dev| dev == stat.dev);
            let item = self.add(
                kind,
                project.join("node_modules"),
                &stat,
                Some(&measured),
                facts,
            );
            item.name = format!("{project_name}/node_modules");
            item.project = Some(project);
            item.reassess(running, open, same_volume);
        }
    }

    /// Large and old files in the granted folders, inspect only.
    fn large_and_old(&mut self, skipped: &mut Vec<String>) {
        let options = self.ctx.options;
        let large_min = u64::from(options.large_min_mb).saturating_mul(1_000_000);
        let mut large: Vec<(PathBuf, FileStat)> = Vec::new();
        let mut old: Vec<(PathBuf, FileStat, u64)> = Vec::new();
        for root in self.ctx.large_roots {
            if self.cancelled() {
                break;
            }
            if !self.walk_files(root, &mut |path, stat, idle| {
                let bytes = stat.blocks.saturating_mul(512);
                if bytes >= large_min {
                    keep_largest(&mut large, (path, *stat), |(_, stat)| stat.blocks);
                } else if bytes >= OLD_MIN_BYTES && idle >= u64::from(options.old_days) {
                    keep_largest(&mut old, (path, *stat, idle), |(_, stat, _)| stat.blocks);
                }
            }) {
                skipped.push(display_path(root, Some(self.ctx.home)));
            }
        }
        for (path, stat) in large {
            let facts = StoredFacts {
                larger_than: large_min,
                ..StoredFacts::default()
            };
            self.add(Kind::LargeFile, path, &stat, None, facts);
        }
        for (path, stat, idle) in old {
            let facts = StoredFacts {
                idle_days: idle,
                ..StoredFacts::default()
            };
            self.add(Kind::OldFile, path, &stat, None, facts);
        }
    }

    /// Visits every regular file below `root` on its volume, depth-first through handles.
    /// `false` if the root couldn't be opened.
    fn walk_files(&mut self, root: &Path, visit: &mut dyn FnMut(PathBuf, &FileStat, u64)) -> bool {
        if self.excluded(root) {
            // Not searched by choice, so not reported as skipped either.
            return true;
        }
        let Some(mounts) = non_local_mounts() else {
            return false;
        };
        if mounts.iter().any(|mount| root.starts_with(mount)) {
            return false;
        }
        let Some((dir, dev)) = open_absolute(root) else {
            return false;
        };
        let hidden = self.ctx.options.hidden;
        let mut stack: Vec<(DirHandle, PathBuf)> = vec![(dir, root.to_path_buf())];
        while let Some((mut dir, path)) = stack.pop() {
            if self.cancelled() {
                break;
            }
            let names: Vec<CString> = std::iter::from_fn(|| dir.next_name())
                .filter_map(Result::ok)
                .collect();
            for name in names {
                let bytes = name.to_bytes();
                if hidden_skipped(bytes, hidden) {
                    continue;
                }
                let child = path.join(OsStr::from_bytes(bytes));
                if self.excluded(&child) {
                    continue;
                }
                let Ok(stat) = dir.stat_child(&name) else {
                    continue;
                };
                if stat.is_symlink() || stat.dev != dev {
                    continue;
                }
                if stat.is_file() {
                    let idle = self.idle_days(stat.accessed_s.max(stat.modified_s));
                    visit(child, &stat, idle);
                } else if stat.is_dir() && stack.len() < MAX_FILE_DEPTH && !skipped_folder(bytes) {
                    if let Ok((opened_dir, opened)) = dir.open_child(&name) {
                        if opened.dev == stat.dev && opened.ino == stat.ino {
                            stack.push((opened_dir, child));
                        }
                    }
                }
            }
        }
        true
    }
}

/// Folders a large-file search doesn't enter: app bundles and libraries (one item each to
/// macOS), and places with their own pages.
/// Dot-folders tools make that aren't private: build output, dependencies and caches. Their
/// name starts with a dot only to keep them out of the way, so the large and old files in
/// them are searched even with hidden files off.
const TOOL_DOT_FOLDERS: &[&str] = &[
    ".cache",
    ".expo",
    ".angular",
    ".output",
    ".vercel",
    ".netlify",
    ".serverless",
    ".terraform",
    ".yarn",
    ".pnpm-store",
];

/// Whether the large and old search skips `name` as hidden. With hidden files off, only
/// private dotfiles are skipped (`.env`, `.ssh`, `.git`, …), never a tool's build or cache
/// folder such as `.next`.
pub(super) fn hidden_skipped(name: &[u8], hidden: bool) -> bool {
    if hidden || !name.starts_with(b".") {
        return false;
    }
    let Ok(name) = std::str::from_utf8(name) else {
        return true;
    };
    !(policy::is_build_folder(name) || TOOL_DOT_FOLDERS.contains(&name))
}

fn skipped_folder(name: &[u8]) -> bool {
    const SUFFIXES: &[&[u8]] = &[
        b".app",
        b".photoslibrary",
        b".musiclibrary",
        b".fcpbundle",
        b".xcarchive",
        b".bundle",
        b".framework",
    ];
    name == b"node_modules"
        || name == b"Library"
        || name == b".Trash"
        || name == b".git"
        || SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn keep_largest<T>(list: &mut Vec<T>, entry: T, size: impl Fn(&T) -> u64) {
    let position = list.partition_point(|kept| size(kept) >= size(&entry));
    if position < MAX_FILES {
        list.insert(position, entry);
        list.truncate(MAX_FILES);
    }
}

/// A simulator's name from its `device.plist` (no-follow, regular file, at most 64 KB).
fn device_name(devices: &DirHandle, device: &CString, dev: u64) -> Option<String> {
    let (dir, opened) = devices.open_child(device).ok()?;
    if opened.dev != dev {
        return None;
    }
    let file = dir.open_file_child(c"device.plist").ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes).ok()?;
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    let name = value.as_dictionary()?.get("name")?.as_string()?.trim();
    (!name.is_empty() && name.len() <= 80 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

/// Runs the mode's steps, reporting progress. Stops early on cancel, keeping what was found.
pub fn run(mode: ScanMode, ctx: &Context<'_>, mut progress: impl FnMut(Progress)) -> Outcome {
    let mut collector = Collector::new(ctx, &mut progress);
    let mut results = Vec::new();
    let mut docker = None;
    let mut skipped = Vec::new();
    for (index, step) in steps(mode, &ctx.options).into_iter().enumerate() {
        if collector.cancelled() {
            break;
        }
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        collector.start_step(index);
        match step {
            StepId::AppCaches => collector.app_caches(false),
            StepId::LogsAndTemp => collector.logs_and_temp(),
            StepId::PackageCaches => {
                collector.app_caches(true);
                collector.catalog_caches();
            }
            StepId::LargeAndOld => collector.large_and_old(&mut skipped),
            StepId::Xcode => collector.xcode(),
            StepId::Simulators => collector.simulators(),
            StepId::NodeModules => collector.node_modules(&mut skipped),
            StepId::Docker => docker = Some(docker::read(Some(ctx.home), ctx.cancel)),
        }
        collector.keep_step();
        let done = collector.progress();
        (collector.report)(done);
        results.push(StepResult {
            id: step,
            items: done.items,
            bytes: done.bytes,
            finished: !collector.cancelled(),
        });
    }
    Outcome {
        cancelled: collector.cancelled(),
        steps: results,
        items: collector.items,
        denied: collector.denied,
        docker,
        skipped_roots: skipped,
    }
}
