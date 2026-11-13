//! The Processes page list: running processes grouped into rows, read-only. Apps (regular
//! macOS apps from `NSWorkspace`) are one row with their helper processes folded in; every
//! other process is its own row, unless it runs from inside an app bundle, which groups it
//! with that bundle.
//!
//! Pure: the OS adapter (`process_sampler`) supplies raw processes, the regular apps and
//! lookups for user names and thread counts, so every decision here is tested.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::processes::{app_bundle_from_exe, cpu_percent, display_name, CpuBaseline, RawProcess};

/// Rows sent: the busiest by CPU together with the largest by memory, so either sort is
/// complete for the processes that matter.
pub const ROW_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessKind {
    /// A regular app: it has a Dock icon and a menu bar.
    App,
    /// Run by you, without an app's menu bar (agents, tools, dev servers).
    Background,
    /// Run by macOS: owned by another account, or from macOS's own folders.
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    /// Stable while the row's app or process keeps running.
    pub id: String,
    pub name: String,
    pub kind: ProcessKind,
    /// The app's own process, or the row's main process.
    pub pid: u32,
    pub process_count: u32,
    /// Sum over members with a reading, as a share of one CPU; `None` before any reading.
    pub cpu_percent: Option<f64>,
    /// Members included in `cpu_percent`; fewer than `process_count` makes it a minimum.
    pub cpu_measured_count: u32,
    /// Resident memory summed over members (shared memory can be counted more than once).
    pub memory_bytes: u64,
    /// Account that runs the main process; `None` if it couldn't be looked up.
    pub user: Option<String>,
    /// When the main process started, in milliseconds since the Unix epoch.
    pub started_at_ms: Option<u64>,
    /// Threads in the main process; `None` when macOS doesn't let Kleari read it
    /// (processes of other accounts).
    pub threads: Option<u32>,
    /// The app bundle or executable, with the home folder shown as `~`.
    pub location: Option<String>,
    pub is_self: bool,
    /// Only regular apps of this account outside macOS's own folders, never Kleari
    /// itself, can be asked to quit.
    pub can_quit: bool,
    /// Set only when `can_quit`: the app process's start time in microseconds, which
    /// `quit_app` must be given back and checks again immediately before asking.
    pub quit_token: Option<u64>,
    /// The app's resolved bundle, kept by the backend with the token so `quit_app` can check
    /// it is still this app. Never sent to the frontend.
    #[serde(skip)]
    pub quit_bundle: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessList {
    pub sampled_at_ms: u64,
    /// The CPU sampling window; `None` on the first sample or after a long gap, so CPU
    /// values are withheld rather than averaged over a long period.
    pub window_ms: Option<u64>,
    pub logical_cpus: u32,
    /// All logical CPUs, 0–100, over the same window; `None` with `window_ms`.
    pub total_cpu_percent: Option<f64>,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    /// Every process seen, including unreadable ones and those not listed.
    pub total_count: u32,
    /// Processes with no memory value from macOS or `ps`, counted but left out of the rows.
    /// The cause isn't checked per process.
    pub unreadable_count: u32,
    pub rows: Vec<ProcessRow>,
}

/// A process the operating system reports as one of this account's regular apps.
#[derive(Debug, Clone, PartialEq, Eq)]
// Built from AppKit; on other targets (Linux CI) only tests build it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct RunningApp {
    pub pid: u32,
    pub name: String,
    pub bundle: Option<PathBuf>,
}

/// One process with what the list needs beyond [`RawProcess`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedProcess {
    pub raw: RawProcess,
    pub uid: Option<u32>,
    pub exe: Option<PathBuf>,
}

pub struct Context<'a> {
    pub own_pid: u32,
    pub own_uid: u32,
    pub home: Option<&'a Path>,
    pub logical_cpus: usize,
    pub user_name: &'a dyn Fn(u32) -> Option<String>,
    pub threads: &'a dyn Fn(u32) -> Option<u32>,
    /// A process's start time (microseconds) and owner, read by PID.
    pub identity: &'a dyn Fn(u32) -> Option<ProcessIdentity>,
    /// A bundle path with symlinks resolved (`/Applications/Safari.app` is a symlink into
    /// `/System/Cryptexes`); `None` if it can't be resolved.
    pub canonical: &'a dyn Fn(&Path) -> Option<PathBuf>,
}

/// What ties a PID to one particular process: a PID can be reused, a start time to the
/// microsecond together with the owner can't be confused with another process's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Read through proc_pidinfo on macOS; on other targets (Linux CI) only tests build it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct ProcessIdentity {
    pub start_us: u64,
    pub uid: u32,
}

/// Apps that ship with macOS (Finder, Terminal and others in `/System`) are never quit from
/// Kleari, even though macOS reports them as regular apps.
pub fn is_quittable_bundle(bundle: &Path) -> bool {
    !in_system_folder(Some(bundle))
}

/// Whether a regular app may be asked to quit: another app of this account, outside macOS's
/// own folders, identified to the microsecond. `bundle` must already be resolved
/// (`Context::canonical`), so a symlink can't hide a system app. Used for the list and again
/// by `quit_app`.
pub fn may_quit(
    own_pid: u32,
    own_uid: u32,
    pid: u32,
    bundle: Option<&Path>,
    identity: Option<ProcessIdentity>,
) -> bool {
    pid != own_pid
        && bundle.is_some_and(is_quittable_bundle)
        && identity.is_some_and(|identity| identity.uid == own_uid && identity.start_us > 0)
}

/// Folders macOS ships its own programs in.
const SYSTEM_FOLDERS: [&str; 5] = [
    "/System/",
    "/usr/libexec/",
    "/usr/sbin/",
    "/sbin/",
    "/Library/Apple/",
];

fn in_system_folder(exe: Option<&Path>) -> bool {
    exe.and_then(Path::to_str)
        .is_some_and(|path| SYSTEM_FOLDERS.iter().any(|folder| path.starts_with(folder)))
}

fn classify(process: &ListedProcess, own_uid: u32) -> ProcessKind {
    let own = process.uid == Some(own_uid);
    if own && process.raw.pid != 0 && !in_system_folder(process.exe.as_deref()) {
        ProcessKind::Background
    } else {
        ProcessKind::System
    }
}

pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) {
        if rest.as_os_str().is_empty() {
            return "~".to_owned();
        }
        return format!("~/{}", rest.to_string_lossy());
    }
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum GroupKey {
    Bundle(PathBuf),
    Process(u32, u64),
}

struct Group<'a> {
    key: GroupKey,
    members: Vec<&'a ListedProcess>,
}

/// The process that represents a group: the app's own process when it is a regular app,
/// otherwise the earliest started member.
fn main_member<'a>(group: &Group<'a>, app: Option<&RunningApp>) -> &'a ListedProcess {
    if let Some(found) = app.and_then(|app| {
        group
            .members
            .iter()
            .find(|member| member.raw.pid == app.pid)
    }) {
        return found;
    }
    group
        .members
        .iter()
        .min_by_key(|member| (member.raw.start_time, member.raw.pid))
        .copied()
        .unwrap_or(group.members[0])
}

fn row_of(
    group: &Group<'_>,
    apps_by_bundle: &HashMap<&Path, &RunningApp>,
    previous: Option<(&CpuBaseline, u64)>,
    context: &Context<'_>,
) -> ProcessRow {
    let app = match &group.key {
        GroupKey::Bundle(bundle) => apps_by_bundle.get(bundle.as_path()).copied(),
        GroupKey::Process(..) => None,
    };
    let main = main_member(group, app);
    let ceiling = context.logical_cpus.max(1) as f64 * 100.0;
    let mut cpu = None;
    let mut measured = 0_u32;
    let mut memory = 0_u64;
    for member in &group.members {
        memory = memory.saturating_add(member.raw.memory);
        if let Some(value) = previous.and_then(|(baseline, window)| {
            cpu_percent(&member.raw, baseline, window, context.logical_cpus)
        }) {
            cpu = Some((cpu.unwrap_or(0.0) + value).min(ceiling));
            measured = measured.saturating_add(1);
        }
    }
    let kind = if app.is_some() {
        ProcessKind::App
    } else {
        classify(main, context.own_uid)
    };
    let (id, name, location) = match &group.key {
        GroupKey::Bundle(bundle) => (
            format!("bundle:{}", bundle.to_string_lossy()),
            app.map(|app| app.name.clone()).unwrap_or_else(|| {
                bundle.file_stem().map_or_else(
                    || main.raw.name.clone(),
                    |stem| stem.to_string_lossy().into_owned(),
                )
            }),
            Some(display_path(bundle, context.home)),
        ),
        GroupKey::Process(pid, start) => (
            format!("pid:{pid}:{start}"),
            main.raw.name.clone(),
            main.exe
                .as_deref()
                .map(|exe| display_path(exe, context.home)),
        ),
    };
    let is_self = group
        .members
        .iter()
        .any(|member| member.raw.pid == context.own_pid);
    let quittable = (kind == ProcessKind::App && !is_self)
        .then(|| match &group.key {
            GroupKey::Bundle(bundle) => (context.canonical)(bundle),
            GroupKey::Process(..) => None,
        })
        .flatten()
        .and_then(|bundle| {
            let identity = (context.identity)(main.raw.pid)?;
            // The identity is read after the sample; it must be the sampled process (same
            // start second), or the PID now belongs to another process and nothing is offered.
            let sampled = identity.start_us / 1_000_000 == main.raw.start_time;
            (sampled
                && may_quit(
                    context.own_pid,
                    context.own_uid,
                    main.raw.pid,
                    Some(&bundle),
                    Some(identity),
                ))
            .then_some((identity.start_us, bundle))
        });
    let (quit_token, quit_bundle) =
        quittable.map_or((None, None), |(token, bundle)| (Some(token), Some(bundle)));
    ProcessRow {
        id,
        name: display_name(&name),
        kind,
        pid: main.raw.pid,
        process_count: u32::try_from(group.members.len()).unwrap_or(u32::MAX),
        cpu_percent: cpu,
        cpu_measured_count: measured,
        memory_bytes: memory,
        user: main.uid.and_then(|uid| (context.user_name)(uid)),
        started_at_ms: (main.raw.start_time > 0).then(|| main.raw.start_time.saturating_mul(1000)),
        threads: (context.threads)(main.raw.pid),
        location,
        is_self,
        can_quit: quit_token.is_some(),
        quit_token,
        quit_bundle,
    }
}

fn by_cpu(a: &ProcessRow, b: &ProcessRow) -> std::cmp::Ordering {
    let (a_cpu, b_cpu) = (a.cpu_percent.unwrap_or(-1.0), b.cpu_percent.unwrap_or(-1.0));
    b_cpu
        .total_cmp(&a_cpu)
        .then(b.memory_bytes.cmp(&a.memory_bytes))
        .then(a.id.cmp(&b.id))
}

/// Groups readable processes into rows and keeps the [`ROW_LIMIT`] busiest by CPU together
/// with the [`ROW_LIMIT`] largest by memory, ordered by CPU. Returns the rows and how many
/// processes had no memory value, which are counted but never listed.
pub fn build_rows(
    processes: &[ListedProcess],
    apps: &[RunningApp],
    previous: Option<(&CpuBaseline, u64)>,
    context: &Context<'_>,
) -> (Vec<ProcessRow>, u32) {
    let apps_by_pid: HashMap<u32, &RunningApp> = apps.iter().map(|app| (app.pid, app)).collect();
    let apps_by_bundle: HashMap<&Path, &RunningApp> = apps
        .iter()
        .filter_map(|app| app.bundle.as_deref().map(|bundle| (bundle, app)))
        .collect();
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut index: HashMap<GroupKey, usize> = HashMap::new();
    let mut unreadable = 0_u32;
    for process in processes {
        if process.raw.memory == 0 {
            unreadable = unreadable.saturating_add(1);
            continue;
        }
        // An app's own process joins its bundle even when its executable isn't laid out as
        // `Name.app/Contents/…` (iPhone and iPad apps on Apple silicon).
        let bundle = apps_by_pid
            .get(&process.raw.pid)
            .and_then(|app| app.bundle.clone())
            .or_else(|| process.exe.as_deref().and_then(app_bundle_from_exe));
        let key = bundle.map_or(
            GroupKey::Process(process.raw.pid, process.raw.start_time),
            GroupKey::Bundle,
        );
        let position = *index.entry(key.clone()).or_insert_with(|| {
            groups.push(Group {
                key,
                members: Vec::new(),
            });
            groups.len() - 1
        });
        groups[position].members.push(process);
    }

    let mut rows: Vec<ProcessRow> = groups
        .iter()
        .map(|group| row_of(group, &apps_by_bundle, previous, context))
        .collect();
    rows.sort_by(by_cpu);
    let mut by_memory: Vec<usize> = (0..rows.len()).collect();
    by_memory.sort_by(|&a, &b| {
        rows[b]
            .memory_bytes
            .cmp(&rows[a].memory_bytes)
            .then(rows[a].id.cmp(&rows[b].id))
    });
    let mut keep = vec![false; rows.len()];
    for position in (0..rows.len())
        .take(ROW_LIMIT)
        .chain(by_memory.into_iter().take(ROW_LIMIT))
    {
        keep[position] = true;
    }
    let mut kept = keep.iter();
    rows.retain(|_| kept.next().copied().unwrap_or(false));
    (rows, unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 501;

    fn process(
        pid: u32,
        name: &str,
        uid: u32,
        exe: &str,
        cpu_time_ms: u64,
        memory: u64,
    ) -> ListedProcess {
        ListedProcess {
            raw: RawProcess {
                pid,
                name: name.to_owned(),
                start_time: 1_700_000_000 + u64::from(pid),
                cpu_time_ms,
                memory,
                app: None,
            },
            uid: Some(uid),
            exe: (!exe.is_empty()).then(|| PathBuf::from(exe)),
        }
    }

    fn context<'a>(
        user_name: &'a dyn Fn(u32) -> Option<String>,
        threads: &'a dyn Fn(u32) -> Option<u32>,
    ) -> Context<'a> {
        Context {
            own_pid: 900,
            own_uid: ME,
            home: Some(Path::new("/Users/me")),
            logical_cpus: 4,
            user_name,
            threads,
            identity: &identities,
            canonical: &canonical,
        }
    }

    /// This account's processes (pid ≥ 100 in these fixtures) can be identified; launchd
    /// (root) can't.
    fn identities(pid: u32) -> Option<ProcessIdentity> {
        (pid >= 100).then(|| ProcessIdentity {
            // The sampled start second (see `process`), plus microseconds.
            start_us: (1_700_000_000 + u64::from(pid)) * 1_000_000 + 123,
            uid: ME,
        })
    }

    /// Paths resolve to themselves, except Safari, which is a symlink into the Cryptex.
    fn canonical(path: &Path) -> Option<PathBuf> {
        if path == Path::new("/Applications/Safari.app") {
            return Some(PathBuf::from(
                "/System/Cryptexes/App/System/Applications/Safari.app",
            ));
        }
        Some(path.to_path_buf())
    }

    fn names(uid: u32) -> Option<String> {
        match uid {
            501 => Some("me".to_owned()),
            0 => Some("root".to_owned()),
            _ => None,
        }
    }

    fn own_threads(pid: u32) -> Option<u32> {
        (pid != 1).then_some(12)
    }

    fn chrome() -> RunningApp {
        RunningApp {
            pid: 100,
            name: "Google Chrome".to_owned(),
            bundle: Some(PathBuf::from("/Applications/Google Chrome.app")),
        }
    }

    fn sample() -> Vec<ListedProcess> {
        vec![
            process(100, "Google Chrome", ME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", 1_000, 300),
            process(101, "Google Chrome Helper", ME, "/Applications/Google Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper", 2_000, 200),
            process(200, "node", ME, "/opt/homebrew/bin/node", 500, 100),
            process(300, "mdworker", ME, "/System/Library/Frameworks/CoreServices.framework/mdworker", 0, 50),
            process(1, "launchd", 0, "/sbin/launchd", 0, 40),
            process(400, "WindowServer", 88, "", 0, 0),
            process(900, "kleari", ME, "/Users/me/Code/kleari/target/debug/kleari", 0, 10),
        ]
    }

    fn rows(previous: Option<(&CpuBaseline, u64)>) -> (Vec<ProcessRow>, u32) {
        rows_with(&sample(), previous)
    }

    fn rows_with(
        processes: &[ListedProcess],
        previous: Option<(&CpuBaseline, u64)>,
    ) -> (Vec<ProcessRow>, u32) {
        let ctx = context(&names, &own_threads);
        build_rows(processes, &[chrome()], previous, &ctx)
    }

    fn find<'a>(rows: &'a [ProcessRow], name: &str) -> &'a ProcessRow {
        rows.iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    }

    #[test]
    fn apps_that_ship_with_macos_are_never_quittable() {
        let terminal = RunningApp {
            pid: 700,
            name: "Terminal".to_owned(),
            bundle: Some(PathBuf::from("/System/Applications/Utilities/Terminal.app")),
        };
        let mut processes = sample();
        processes.push(process(
            700,
            "Terminal",
            ME,
            "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
            0,
            90,
        ));
        let ctx = context(&names, &own_threads);
        let (rows, _) = build_rows(&processes, &[chrome(), terminal], None, &ctx);
        let row = find(&rows, "Terminal");
        assert_eq!(row.kind, ProcessKind::App);
        assert!(!row.can_quit);
        assert_eq!(row.quit_token, None);
    }

    #[test]
    fn an_app_that_cant_be_identified_or_belongs_to_another_account_isnt_quittable() {
        let none = |_: u32| None;
        let ctx = Context {
            identity: &none,
            ..context(&names, &own_threads)
        };
        let (rows, _) = build_rows(&sample(), &[chrome()], None, &ctx);
        assert!(!find(&rows, "Google Chrome").can_quit);

        // The sampled process, but owned by another account.
        let other =
            |pid: u32| identities(pid).map(|identity| ProcessIdentity { uid: 0, ..identity });
        let ctx = Context {
            identity: &other,
            ..context(&names, &own_threads)
        };
        let (rows, _) = build_rows(&sample(), &[chrome()], None, &ctx);
        assert!(!find(&rows, "Google Chrome").can_quit);
    }

    #[test]
    fn a_pid_reused_since_the_sample_gets_no_quit_token() {
        // The PID now has a process that started a minute after the sampled one.
        let reused = |pid: u32| {
            identities(pid).map(|identity| ProcessIdentity {
                start_us: identity.start_us + 60_000_000,
                ..identity
            })
        };
        let ctx = Context {
            identity: &reused,
            ..context(&names, &own_threads)
        };
        let (rows, _) = build_rows(&sample(), &[chrome()], None, &ctx);
        let chrome = find(&rows, "Google Chrome");
        assert!(!chrome.can_quit);
        assert_eq!(
            (chrome.quit_token, chrome.quit_bundle.as_deref()),
            (None, None)
        );
    }

    #[test]
    fn an_apple_app_behind_a_symlink_is_resolved_and_protected() {
        let safari = RunningApp {
            pid: 800,
            name: "Safari".to_owned(),
            bundle: Some(PathBuf::from("/Applications/Safari.app")),
        };
        let mut processes = sample();
        processes.push(process(
            800,
            "Safari",
            ME,
            "/Applications/Safari.app/Contents/MacOS/Safari",
            0,
            90,
        ));
        let ctx = context(&names, &own_threads);
        let (rows, _) = build_rows(&processes, &[chrome(), safari], None, &ctx);
        assert!(!find(&rows, "Safari").can_quit);

        // A bundle that can't be resolved is never offered.
        let unresolved = |_: &Path| None;
        let ctx = Context {
            canonical: &unresolved,
            ..context(&names, &own_threads)
        };
        let (rows, _) = build_rows(&sample(), &[chrome()], None, &ctx);
        assert!(!find(&rows, "Google Chrome").can_quit);
    }

    #[test]
    fn may_quit_requires_another_identified_app_of_this_account_outside_macos_folders() {
        let identity = Some(ProcessIdentity {
            start_us: 5,
            uid: ME,
        });
        let chrome = Some(Path::new("/Applications/Google Chrome.app"));
        assert!(may_quit(1, ME, 2, chrome, identity));
        assert!(!may_quit(2, ME, 2, chrome, identity), "Kleari itself");
        assert!(!may_quit(1, ME, 2, None, identity), "no bundle");
        assert!(!may_quit(
            1,
            ME,
            2,
            Some(Path::new("/System/Library/CoreServices/Finder.app")),
            identity
        ));
        assert!(!may_quit(1, ME, 2, chrome, None), "unidentified");
        assert!(
            !may_quit(
                1,
                ME,
                2,
                chrome,
                Some(ProcessIdentity {
                    start_us: 5,
                    uid: 0
                })
            ),
            "other account"
        );
        assert!(
            !may_quit(
                1,
                ME,
                2,
                chrome,
                Some(ProcessIdentity {
                    start_us: 0,
                    uid: ME
                })
            ),
            "no start time"
        );
    }

    #[test]
    fn processes_are_background_or_system_by_owner_and_folder() {
        let (rows, _) = rows(None);
        assert_eq!(find(&rows, "node").kind, ProcessKind::Background);
        assert_eq!(find(&rows, "mdworker").kind, ProcessKind::System);
        let launchd = find(&rows, "launchd");
        assert_eq!(launchd.kind, ProcessKind::System);
        assert_eq!(launchd.user.as_deref(), Some("root"));
        assert_eq!(launchd.threads, None);
        // Nothing but a regular app can be quit.
        assert!(rows
            .iter()
            .filter(|row| row.kind != ProcessKind::App)
            .all(|row| !row.can_quit));
    }

    #[test]
    fn kleari_itself_is_marked_and_never_quittable() {
        let (rows, _) = rows(None);
        let own = find(&rows, "kleari");
        assert!(own.is_self && !own.can_quit);
        assert_eq!(
            own.location.as_deref(),
            Some("~/Code/kleari/target/debug/kleari")
        );

        // Even when Kleari is a regular app, it can't be asked to quit from here.
        let mut processes = sample();
        processes.push(process(
            901,
            "Kleari",
            ME,
            "/Applications/Kleari.app/Contents/MacOS/kleari",
            0,
            10,
        ));
        let ctx = Context {
            own_pid: 901,
            ..context(&names, &own_threads)
        };
        let app = RunningApp {
            pid: 901,
            name: "Kleari".to_owned(),
            bundle: Some(PathBuf::from("/Applications/Kleari.app")),
        };
        let (rows, _) = build_rows(&processes, &[app], None, &ctx);
        let kleari = find(&rows, "Kleari");
        assert_eq!(kleari.kind, ProcessKind::App);
        assert!(kleari.is_self && !kleari.can_quit);
    }

    #[test]
    fn a_bundle_that_isnt_a_regular_app_is_grouped_but_not_quittable() {
        let mut processes = sample();
        processes.push(process(
            500,
            "Dropbox Helper",
            ME,
            "/Applications/Dropbox.app/Contents/MacOS/Dropbox Helper",
            0,
            70,
        ));
        let (rows, _) = rows_with(&processes, None);
        let dropbox = find(&rows, "Dropbox");
        assert_eq!(dropbox.kind, ProcessKind::Background);
        assert!(!dropbox.can_quit);
    }
}
