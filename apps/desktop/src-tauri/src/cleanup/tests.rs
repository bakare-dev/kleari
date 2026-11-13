use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use super::execute::{self, ApproveOutcome, Checks, MoveState, PlannedState};
use super::policy::{self, Eligibility, Facts, Kind, Risk, WhyNot};
use super::scan::{self, Context, ScanMode, ScanOptions};
use super::scope::{self, Access, ProtectedFolder};
use super::*;
use crate::history::History;
use crate::persistence::{Database, ItemState, Unavailable};
use crate::quarantine::Store;
use crate::system_monitor::open_paths::OpenPaths;
use crate::system_monitor::running_apps::RunningBundle;

/// Nothing open in any app.
static NOTHING_OPEN: OpenPaths = OpenPaths::NOTHING;

const DAY: Duration = Duration::from_secs(86_400);

struct Home {
    root: PathBuf,
}

impl Home {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("kleari-cleanup-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home")).expect("home");
        fs::create_dir_all(root.join("data")).expect("data");
        Self {
            root: fs::canonicalize(&root).expect("canonical"),
        }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn write(&self, relative: &str, bytes: usize) -> PathBuf {
        let path = self.home().join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(&path, vec![7_u8; bytes]).expect("file");
        path
    }

    fn history(&self) -> History {
        History {
            db: Database::in_memory(),
            store: Store::new(self.root.join("data")),
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Sets the modification time of `path` (a file or folder).
fn age(path: &Path, by: Duration) {
    let when = SystemTime::now() - by;
    fs::File::open(path)
        .expect("open")
        .set_modified(when)
        .expect("mtime");
}

fn options() -> ScanOptions {
    ScanOptions {
        logs: true,
        temp: false,
        log_age_days: 30,
        large_min_mb: 500,
        old_days: 180,
        hidden: false,
        xcode: true,
        sim: true,
        node: true,
        docker: false,
    }
}

fn now_ms() -> u64 {
    crate::history::unix_time_ms()
}

/// Every path below `root` with its size and modification time, to prove a scan wrote nothing.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in fs::read_dir(&folder).expect("read").flatten() {
            let metadata = entry.metadata().expect("metadata");
            entries.push((
                entry.path(),
                metadata.len(),
                metadata.modified().expect("mtime"),
            ));
            if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    entries.sort();
    entries
}

fn run_scan(
    home: &Home,
    history: &History,
    mode: ScanMode,
    running: &[RunningBundle],
) -> scan::Outcome {
    run_scan_keeping(home, history, mode, running, &|_| true, &[])
}

fn run_scan_keeping(
    home: &Home,
    history: &History,
    mode: ScanMode,
    running: &[RunningBundle],
    keep: &dyn Fn(&Candidate) -> bool,
    excluded: &[PathBuf],
) -> scan::Outcome {
    run_scan_with_open(
        home,
        history,
        mode,
        running,
        keep,
        excluded,
        Some(&NOTHING_OPEN),
    )
}

fn run_scan_with_open(
    home: &Home,
    history: &History,
    mode: ScanMode,
    running: &[RunningBundle],
    keep: &dyn Fn(&Candidate) -> bool,
    excluded: &[PathBuf],
    open: Option<&OpenPaths>,
) -> scan::Outcome {
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let roots = vec![home_path.clone()];
    let ctx = Context {
        home: &home_path,
        temp: None,
        running,
        open,
        now_ms: now_ms(),
        options: options(),
        store_dev: history.store.volume(),
        large_roots: &[],
        project_roots: &roots,
        cancel: &cancel,
        keep,
        excluded,
    };
    scan::run(mode, &ctx, |_| {})
}

#[test]
fn preferences_leave_findings_out_and_the_step_totals_count_only_what_is_kept() {
    let home = Home::new("preferences");
    home.write("Library/Caches/com.example.Big/blob", 30_000_000);
    home.write("Library/Caches/com.example.Small/blob", 2_000_000);
    home.write("Library/Caches/com.example.Skipped/blob", 30_000_000);
    let history = home.history();
    let preferences = crate::preferences::Preferences {
        min_size_mb: 10,
        ..crate::preferences::Preferences::default()
    };
    let excluded = [home.home().join("Library/Caches/com.example.Skipped")];
    use std::os::unix::fs::PermissionsExt;
    // The excluded folder is unreadable: opening it would be counted as refused.
    let skipped = excluded[0].clone();
    fs::set_permissions(&skipped, fs::Permissions::from_mode(0o000)).expect("lock");
    let keep = |item: &Candidate| kept_by_preferences(item, &preferences, &excluded, now_ms());
    let outcome = run_scan_keeping(&home, &history, ScanMode::Standard, &[], &keep, &excluded);
    // Without the exclusion the same scan does try to read it.
    let probe = run_scan_keeping(&home, &history, ScanMode::Standard, &[], &|_| true, &[]);
    fs::set_permissions(&skipped, fs::Permissions::from_mode(0o755)).expect("unlock");
    assert!(probe.denied > 0, "the locked folder is a valid probe");
    let names: Vec<&str> = outcome
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["com.example.Big"]);
    let caches = outcome
        .steps
        .iter()
        .find(|step| step.id == scan::StepId::AppCaches)
        .expect("app caches step");
    assert_eq!(caches.items, 1);
    // Never opened, so nothing was refused.
    assert_eq!(outcome.denied, 0);
}

#[test]
fn only_recently_changed_large_files_are_left_out_and_preferences_are_checked() {
    use crate::preferences::Preferences;
    let now = 100 * 86_400_000;
    let preferences = Preferences {
        skip_recent_days: 7,
        ..Preferences::default()
    };
    let home = Home::new("recent");
    let file = home.write("Downloads/big.dmg", 10);
    let cache = home.write("Library/Caches/com.example.App/blob", 10);
    let item = |kind: Kind, path: PathBuf, modified_days: u64| Candidate {
        id: new_id(now),
        kind,
        identity: crate::quarantine::Identity::of(&stat_path(&path).expect("stat")),
        name: display_name_of(&path),
        location: String::new(),
        path,
        bytes: 1,
        files: 1,
        lower_bound: false,
        modified_ms: Some(modified_days * 86_400_000),
        assessment: policy::assess(&Kind::LargeFile, &facts("x")),
        facts: StoredFacts::default(),
        source: Source::Scan,
        project: None,
    };
    let keep = |candidate: &Candidate| kept_by_preferences(candidate, &preferences, &[], now);
    // Large and old files say what kind of file they are; other findings don't.
    let video = home.write("Movies/IMG_5349.MOV", 10);
    assert_eq!(
        item(Kind::LargeFile, video, 1).finding().file_category,
        Some(crate::storage::category::FileCategory::Video)
    );
    assert_eq!(
        item(Kind::LargeFile, file.clone(), 1)
            .finding()
            .file_category,
        Some(crate::storage::category::FileCategory::DiskImage)
    );
    assert_eq!(
        item(Kind::AppCache, cache.clone(), 1)
            .finding()
            .file_category,
        None
    );
    assert!(!keep(&item(Kind::LargeFile, file.clone(), 98)));
    assert!(keep(&item(Kind::LargeFile, file, 80)));
    // A cache changed yesterday is still listed: the rule is for large files only.
    let folder = cache.parent().expect("folder").to_path_buf();
    assert!(keep(&item(Kind::AppCache, folder, 99)));

    assert!(Preferences::default().valid());
    let off_menu = |change: fn(&mut Preferences)| {
        let mut preferences = Preferences::default();
        change(&mut preferences);
        preferences.valid()
    };
    assert!(off_menu(|p| p.retention_days = 365));
    assert!(off_menu(|p| p.retention_days = 360));
    assert!(!off_menu(|p| p.retention_days = 45));
    assert!(!off_menu(|p| p.retention_days = 0));
    assert!(!off_menu(|p| p.min_size_mb = 15));
    assert!(!off_menu(|p| p.min_size_mb = 510));
    assert!(!off_menu(|p| p.skip_recent_days = 91));
    assert!(!off_menu(|p| p.quarantine_reminder_gb = 101));
    assert!(!off_menu(|p| p.scan_defaults.large_min_mb = 2_000));
    assert!(!off_menu(|p| p.scan_schedule.weekday = 7));
    assert!(!off_menu(|p| p.scan_schedule.hour = 24));
}

#[test]
fn preferences_saved_before_the_schedule_existed_load_with_it_off() {
    use crate::preferences::{load, Frequency, Preferences};
    let db = Database::in_memory();
    // An older shape, without scanSchedule.
    let mut stored = serde_json::to_value(Preferences::default()).expect("serialised");
    if let Some(fields) = stored.as_object_mut() {
        fields.remove("scanSchedule");
    }
    db.set_setting("preferences.v1", &stored.to_string())
        .expect("stored");
    let loaded = load(&db);
    assert_eq!(loaded.source, "saved");
    assert_eq!(loaded.preferences.scan_schedule.frequency, Frequency::Off);
}

#[test]
fn saved_preferences_round_trip_and_unusable_ones_fall_back_to_the_defaults() {
    use crate::preferences::{load, save, Preferences, SaveError};
    let db = Database::in_memory();
    assert_eq!(load(&db).source, "default");
    let chosen = Preferences {
        default_scan: ScanMode::Deep,
        notify_scan: true,
        retention_days: 30,
        ..Preferences::default()
    };
    save(&db, &chosen).expect("saved");
    assert_eq!(load(&db).preferences, chosen);
    assert_eq!(load(&db).source, "saved");
    let bad = Preferences {
        retention_days: 7,
        ..chosen
    };
    assert_eq!(save(&db, &bad), Err(SaveError::Invalid));
    assert_eq!(load(&db).preferences, chosen);
    // Edited by hand to something the page doesn't offer: the defaults apply.
    db.set_setting("preferences.v1", "{\"defaultScan\":\"deep\"}")
        .expect("edited");
    assert_eq!(load(&db).source, "invalid");
    assert_eq!(load(&db).preferences, Preferences::default());
}

fn checks<'a>(
    home: &'a Path,
    history: &History,
    cancel: &'a AtomicBool,
    running: &'a [RunningBundle],
) -> Checks<'a> {
    Checks {
        home,
        temp: None,
        running,
        open: Some(&NOTHING_OPEN),
        store_dev: history.store.volume(),
        added: &[],
        cancel,
        keep: &|_| true,
        excluded: &[],
    }
}

fn by_name<'a>(items: &'a [Candidate], name: &str) -> &'a Candidate {
    items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("{name} found"))
}

fn facts(name: &str) -> Facts<'_> {
    Facts {
        name,
        same_volume: true,
        ..Facts::default()
    }
}

#[test]
fn the_policy_separates_risk_from_eligibility() {
    let app = policy::assess(&Kind::AppCache, &facts("com.example.App"));
    assert_eq!(
        (app.risk, app.eligibility),
        (Risk::Low, Eligibility::Eligible)
    );

    let apple = policy::assess(&Kind::AppCache, &facts("com.apple.Safari"));
    assert_eq!(
        (apple.eligibility, apple.why_not),
        (Eligibility::InspectOnly, Some(WhyNot::AppleCache))
    );

    let running = policy::assess(
        &Kind::AppCache,
        &Facts {
            running_app: Some("Example"),
            ..facts("com.example.App")
        },
    );
    assert_eq!(
        (running.risk, running.eligibility),
        (Risk::Low, Eligibility::NotNow)
    );

    let derived = policy::assess(
        &Kind::DerivedData,
        &Facts {
            xcode_running: true,
            ..facts("App-abc")
        },
    );
    assert_eq!(derived.why_not, Some(WhyNot::XcodeRunning));

    let node = Kind::BuildFolder {
        name: "node_modules".to_owned(),
    };
    assert_eq!(
        policy::assess(&node, &facts("node_modules")).eligibility,
        Eligibility::NeedsCheck
    );
    let locked = Facts {
        lockfile: true,
        ..facts("node_modules")
    };
    assert_eq!(
        policy::assess(&node, &locked).eligibility,
        Eligibility::Eligible
    );
    let active = Facts {
        project_active: true,
        ..locked.clone()
    };
    assert_eq!(
        policy::assess(&node, &active).why_not,
        Some(WhyNot::ActiveProject)
    );

    // Medium risk can still be eligible; eligibility is its own decision.
    let archive = policy::assess(&Kind::Archive, &facts("2026-08-12"));
    assert_eq!(
        (archive.risk, archive.eligibility),
        (Risk::Medium, Eligibility::Eligible)
    );

    for inspect in [
        Kind::LargeFile,
        Kind::OldFile,
        Kind::SimulatorDevice,
        Kind::Catalog("simulatorRuntimes"),
    ] {
        assert_eq!(
            policy::assess(&inspect, &facts("x")).eligibility,
            Eligibility::InspectOnly,
            "{inspect:?}"
        );
    }

    let elsewhere = Facts {
        same_volume: false,
        ..facts("com.example.App")
    };
    let other = policy::assess(&Kind::AppCache, &elsewhere);
    assert_eq!(
        (other.eligibility, other.why_not),
        (Eligibility::InspectOnly, Some(WhyNot::OtherVolume))
    );
}

#[test]
fn items_are_in_bounds_only_at_the_fixed_places_their_kind_allows() {
    let home = Path::new("/Users/someone");
    let none = |_: &str| Vec::new();
    let check = |kind: &Kind, path: &str| {
        policy::within_bounds(
            kind,
            Path::new(path),
            home,
            Some(Path::new("/private/var/folders/x/T")),
            none,
        )
    };
    assert!(check(
        &Kind::AppCache,
        "/Users/someone/Library/Caches/com.example"
    ));
    assert!(!check(
        &Kind::AppCache,
        "/Users/someone/Library/Caches/com.example/inner"
    ));
    assert!(!check(
        &Kind::AppCache,
        "/Users/someone/Library/Caches/com.apple.Safari"
    ));
    assert!(!check(
        &Kind::AppCache,
        "/Users/other/Library/Caches/com.example"
    ));
    assert!(!check(
        &Kind::AppCache,
        "/Users/someone/Library/Caches/../Keychains"
    ));
    assert!(!check(&Kind::AppCache, "Library/Caches/com.example"));
    assert!(check(&Kind::Log, "/Users/someone/Library/Logs/App"));
    assert!(!check(
        &Kind::Log,
        "/Users/someone/Library/Logs/DiagnosticReports"
    ));
    assert!(check(
        &Kind::CrashReport,
        "/Users/someone/Library/Logs/DiagnosticReports/a.ips"
    ));
    assert!(check(&Kind::Temp, "/private/var/folders/x/T/installer"));
    assert!(!check(
        &Kind::Temp,
        "/private/var/folders/x/T/deep/installer"
    ));
    assert!(check(
        &Kind::DeviceSupport,
        "/Users/someone/Library/Developer/Xcode/iOS DeviceSupport/17.5"
    ));
    assert!(!check(
        &Kind::DeviceSupport,
        "/Users/someone/Library/Developer/Xcode/Other/17.5"
    ));

    let node = Kind::BuildFolder {
        name: "node_modules".to_owned(),
    };
    assert!(check(&node, "/Users/someone/Code/web/node_modules"));
    assert!(!check(&node, "/Users/someone/Code/web/src"));
    assert!(!check(&node, "/Users/someone/Code/.git/node_modules"));
    assert!(!check(
        &node,
        "/Users/someone/Library/Application Support/x/node_modules"
    ));
    // Never moved, so never in bounds.
    assert!(!check(&Kind::LargeFile, "/Users/someone/Downloads/big.dmg"));
    assert!(!check(
        &Kind::SimulatorDevice,
        "/Users/someone/Library/Developer/CoreSimulator/Devices/ABC"
    ));

    let catalog = |id: &str| {
        if id == "npm" {
            vec![PathBuf::from("/Users/someone/.npm/_cacache")]
        } else {
            Vec::new()
        }
    };
    assert!(policy::within_bounds(
        &Kind::Catalog("npm"),
        Path::new("/Users/someone/.npm/_cacache"),
        home,
        None,
        catalog
    ));
    assert!(!policy::within_bounds(
        &Kind::Catalog("npm"),
        Path::new("/Users/someone/.npm"),
        home,
        None,
        catalog
    ));
}

#[test]
fn a_stored_original_is_restorable_only_at_a_place_its_category_comes_from() {
    let home = Path::new("/Users/someone");
    let temp = Some(Path::new("/private/var/folders/x/T"));
    let allowed =
        |category: &str, path: &str| policy::restorable(category, Path::new(path), &[], home, temp);
    let under = |root: &str, category: &str, path: &str| {
        policy::restorable(
            category,
            Path::new(path),
            &[PathBuf::from(root)],
            home,
            temp,
        )
    };
    // A build folder outside the home folder, inside a folder chosen in the panel.
    assert!(!allowed("node", "/opt/work/app/node_modules"));
    assert!(under("/opt/work", "node", "/opt/work/app/node_modules"));
    assert!(under("/opt/work", "build", "/opt/work/app/target"));
    assert!(!under("/opt/work", "node", "/opt/other/app/node_modules"));
    assert!(!under("/opt/work", "node", "/opt/work"));
    assert!(!under("/opt/work", "cache", "/opt/work/app/node_modules"));
    assert!(!under("/opt/work", "build", "/opt/work/.git/target"));
    assert!(!under("/opt/work", "build", "/opt/work/app/notes"));
    // A recorded root can't widen the home folder's protections, or be the disk's root.
    assert!(!under("/", "node", "/srv/app/node_modules"));
    assert!(!under("/opt/../srv", "node", "/srv/app/node_modules"));
    assert!(!under(
        "/Users/someone",
        "node",
        "/Users/someone/Library/app/node_modules"
    ));
    assert!(allowed(
        "cache",
        "/Users/someone/Library/Caches/com.example.App"
    ));
    assert!(allowed(
        "xcode",
        "/Users/someone/Library/Developer/Xcode/DerivedData/App-abc"
    ));
    assert!(allowed("node", "/Users/someone/code/app/node_modules"));
    assert!(allowed("build", "/Users/someone/code/app/target"));
    assert!(allowed("logs", "/Users/someone/Library/Logs/App"));

    // The path fits another category, not the recorded one.
    assert!(!allowed("cache", "/Users/someone/code/app/node_modules"));
    assert!(!allowed("build", "/Users/someone/code/app/node_modules"));
    assert!(!allowed("node", "/Users/someone/code/app/target"));
    // Places nothing is ever moved from.
    assert!(!allowed(
        "cache",
        "/Users/someone/Library/Mobile Documents/App"
    ));
    assert!(!allowed("build", "/Users/someone/.ssh/build"));
    assert!(!allowed(
        "cache",
        "/Users/someone/Library/Caches/com.apple.Safari"
    ));
    assert!(!allowed("cache", "/Library/Caches/com.example.App"));
    assert!(!allowed("cache", "/Users/someone/Library/Caches/../Mail/x"));
    // Categories that are never moved.
    assert!(!allowed("large", "/Users/someone/Downloads/big.dmg"));
    assert!(!allowed(
        "sim",
        "/Users/someone/Library/Developer/CoreSimulator/Devices/ABC"
    ));
}

#[test]
fn hidden_files_off_skips_dotfiles_but_not_build_folders() {
    for private in [".env", ".ssh", ".git", ".DS_Store", ".zsh_history"] {
        assert!(scan::hidden_skipped(private.as_bytes(), false), "{private}");
        assert!(!scan::hidden_skipped(private.as_bytes(), true), "{private}");
    }
    for tool in [
        ".next",
        ".nuxt",
        ".turbo",
        ".venv",
        ".gradle",
        ".cache",
        ".terraform",
    ] {
        assert!(!scan::hidden_skipped(tool.as_bytes(), false), "{tool}");
    }
    assert!(!scan::hidden_skipped(b"src", false));
}

#[test]
fn scan_options_accept_only_the_menu_values() {
    assert!(options().valid());
    assert!(!ScanOptions {
        log_age_days: 1,
        ..options()
    }
    .valid());
    assert!(!ScanOptions {
        large_min_mb: 1,
        ..options()
    }
    .valid());
    assert!(!ScanOptions {
        old_days: 2,
        ..options()
    }
    .valid());
    let json = r#"{"logs":true,"temp":true,"logAgeDays":30,"largeMinMb":500,"oldDays":180,"hidden":false,"xcode":true,"sim":true,"node":true,"docker":true,"path":"/"}"#;
    assert!(
        serde_json::from_str::<ScanOptions>(json).is_err(),
        "unknown fields are rejected"
    );
}

#[test]
fn a_standard_scan_classifies_caches_and_logs_and_writes_nothing() {
    let home = Home::new("standard");
    home.write("Library/Caches/com.example.App/blob", 2_000_000);
    home.write("Library/Caches/com.example.Running/blob", 2_000_000);
    home.write("Library/Caches/com.apple.Something/blob", 2_000_000);
    home.write("Library/Caches/Homebrew/downloads/pkg.tar", 2_000_000);
    home.write("Library/Caches/tiny/blob", 1_000);
    let old_log = home.write("Library/Logs/OldApp/app.log", 4_000);
    age(&old_log, 60 * DAY);
    age(old_log.parent().expect("folder"), 60 * DAY);
    home.write("Library/Logs/NewApp/app.log", 4_000);
    let report = home.write("Library/Logs/DiagnosticReports/crash.ips", 4_000);
    age(&report, 60 * DAY);
    let history = home.history();
    let before = snapshot(&home.home());

    let running = [RunningBundle {
        id: "com.example.Running".to_owned(),
        name: "Running".to_owned(),
    }];
    let outcome = run_scan(&home, &history, ScanMode::Standard, &running);
    assert!(!outcome.cancelled);
    assert_eq!(snapshot(&home.home()), before, "the scan wrote nothing");

    let items = &outcome.items;
    assert_eq!(
        by_name(items, "com.example.App").assessment.eligibility,
        Eligibility::Eligible
    );
    assert_eq!(
        by_name(items, "com.example.Running").assessment.eligibility,
        Eligibility::NotNow
    );
    assert_eq!(
        by_name(items, "com.apple.Something").assessment.eligibility,
        Eligibility::InspectOnly
    );
    assert_eq!(by_name(items, "Homebrew").category(), policy::Category::Pkg);
    assert_eq!(by_name(items, "OldApp").category(), policy::Category::Logs);
    assert_eq!(by_name(items, "crash.ips").kind, Kind::CrashReport);
    assert!(items
        .iter()
        .all(|item| item.name != "tiny" && item.name != "NewApp"));
    // Paths never reach the frontend.
    let finding =
        serde_json::to_string(&by_name(items, "com.example.App").finding()).expect("json");
    assert!(!finding.contains(&home.root.to_string_lossy().into_owned()));
    assert!(finding.contains("~/Library/Caches"));
}

#[test]
fn a_developer_scan_finds_xcode_data_and_checks_node_modules_lockfiles() {
    let home = Home::new("dev");
    home.write(
        "Library/Developer/Xcode/DerivedData/App-abc/Build/x",
        2_000_000,
    );
    home.write(
        "Library/Developer/Xcode/iOS DeviceSupport/17.5/symbols",
        2_000_000,
    );
    home.write(
        "Library/Developer/CoreSimulator/Devices/ABC-123/data/x",
        2_000_000,
    );
    home.write("Code/locked/package.json", 10);
    home.write("Code/locked/package-lock.json", 10);
    home.write("Code/locked/node_modules/pkg/index.js", 20_000);
    home.write("Code/loose/package.json", 10);
    home.write("Code/loose/node_modules/pkg/index.js", 20_000);
    // A workspace: the lockfile is at the repository root, above the package.
    home.write("Code/mono/package.json", 10);
    home.write("Code/mono/pnpm-lock.yaml", 10);
    fs::write(
        home.home().join("Code/mono/pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n  - '!packages/legacy'\n",
    )
    .expect("workspace");
    // Below the workspace root but not one of its packages: its lockfile isn't this one's.
    home.write("Code/mono/tools/script/package.json", 10);
    home.write("Code/mono/tools/script/node_modules/pkg/index.js", 20_000);
    fs::create_dir_all(home.home().join("Code/mono/.git")).expect("git");
    home.write("Code/mono/packages/ui/package.json", 10);
    home.write("Code/mono/packages/ui/node_modules/pkg/index.js", 20_000);
    // Another repository below a workspace doesn't borrow its lockfile.
    home.write("Code/mono/vendor-repo/package.json", 10);
    fs::create_dir_all(home.home().join("Code/mono/vendor-repo/.git")).expect("git");
    home.write("Code/mono/vendor-repo/node_modules/pkg/index.js", 20_000);
    let history = home.history();
    let before = snapshot(&home.home());

    let outcome = run_scan(&home, &history, ScanMode::Dev, &[]);
    assert_eq!(snapshot(&home.home()), before, "the scan wrote nothing");
    let items = &outcome.items;
    assert_eq!(
        by_name(items, "App-abc").assessment.eligibility,
        Eligibility::Eligible
    );
    assert_eq!(by_name(items, "17.5").kind, Kind::DeviceSupport);
    assert_eq!(
        by_name(items, "ABC-123").assessment.eligibility,
        Eligibility::InspectOnly
    );
    assert_eq!(
        by_name(items, "locked/node_modules").assessment.eligibility,
        Eligibility::Eligible
    );
    assert_eq!(
        by_name(items, "loose/node_modules").assessment.eligibility,
        Eligibility::NeedsCheck
    );
    assert_eq!(
        by_name(items, "ui/node_modules").assessment.eligibility,
        Eligibility::Eligible
    );
    assert_eq!(
        by_name(items, "script/node_modules").assessment.eligibility,
        Eligibility::NeedsCheck
    );
    assert_eq!(
        by_name(items, "vendor-repo/node_modules")
            .assessment
            .eligibility,
        Eligibility::NeedsCheck
    );
}

#[test]
fn a_project_open_in_some_app_is_held_back_until_nothing_in_it_is_open() {
    let home = Home::new("in-use");
    home.write("Code/web/package.json", 10);
    home.write("Code/web/package-lock.json", 10);
    home.write("Code/web/node_modules/pkg/index.js", 20_000);
    home.write("Code/old/package.json", 10);
    home.write("Code/old/package-lock.json", 10);
    home.write("Code/old/node_modules/pkg/index.js", 20_000);
    let history = home.history();
    let home_path = home.home();
    // An editor has a file in web open; nothing in old is open, whatever its Git history.
    let open = OpenPaths::from_paths(vec![home_path.join("Code/web/src/App.tsx")]);

    let outcome = run_scan_with_open(
        &home,
        &history,
        ScanMode::Dev,
        &[],
        &|_| true,
        &[],
        Some(&open),
    );
    let web = by_name(&outcome.items, "web/node_modules");
    assert_eq!(web.assessment.eligibility, Eligibility::NotNow);
    assert_eq!(web.assessment.why_not, Some(WhyNot::ActiveProject));
    assert_eq!(
        by_name(&outcome.items, "old/node_modules")
            .assessment
            .eligibility,
        Eligibility::Eligible
    );

    // Without a reading of what is open, nothing in a project is suggested (fail closed).
    let unread = run_scan_with_open(&home, &history, ScanMode::Dev, &[], &|_| true, &[], None);
    assert_eq!(
        by_name(&unread.items, "old/node_modules")
            .assessment
            .eligibility,
        Eligibility::NotNow
    );

    // Approval reads what is open again: old, opened since the scan, isn't planned.
    let state = CleanupState::default();
    let old = by_name(&outcome.items, "old/node_modules").clone();
    state.set_scan(
        ScanInfo {
            mode: ScanMode::Dev,
            started_at_ms: now_ms(),
            cancelled: false,
        },
        outcome.items.clone(),
    );
    let cancel = AtomicBool::new(false);
    let opened_since = OpenPaths::from_paths(vec![home_path.join("Code/old")]);
    let now_open = Checks {
        open: Some(&opened_since),
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview =
        execute::create_preview(&state, std::slice::from_ref(&old.id), &now_open, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
    let unread = Checks {
        open: None,
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview = execute::create_preview(&state, std::slice::from_ref(&old.id), &unread, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
    let closed = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&state, std::slice::from_ref(&old.id), &closed, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Planned);
}

fn scanned(home: &Home, history: &History, state: &CleanupState) -> Vec<Candidate> {
    let outcome = run_scan(home, history, ScanMode::Standard, &[]);
    let items = outcome.items.clone();
    state.set_scan(
        ScanInfo {
            mode: ScanMode::Standard,
            started_at_ms: now_ms(),
            cancelled: false,
        },
        outcome.items,
    );
    items
}

#[test]
fn an_approved_preview_moves_items_into_quarantine_and_they_can_be_restored() {
    let home = Home::new("approve");
    let cache = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let item = by_name(&items, "com.example.App");
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let checks = checks(&home_path, &history, &cancel, &[]);

    let preview =
        execute::create_preview(&state, std::slice::from_ref(&item.id), &checks, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Planned);
    // Nothing moves until the preview is approved.
    assert!(cache.exists());

    let mut seen = Vec::new();
    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "standard",
        now_ms,
        |progress| seen.push(progress.state),
    );
    let ApproveOutcome::Finished { items: moved, .. } = outcome else {
        panic!("finished: {outcome:?}");
    };
    assert_eq!(moved[0].state, MoveState::Moved);
    assert_eq!(seen, [MoveState::Checking, MoveState::Moved]);
    assert!(!cache.exists());
    assert!(state
        .review()
        .findings
        .iter()
        .all(|finding| finding.id != item.id));

    let held = history.db.held_items().expect("held");
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].state, ItemState::Held);
    let results = crate::quarantine::restore_items(
        &history.db,
        &history.store,
        &crate::quarantine::Scope {
            home: &home.home(),
            temp: None,
            added: &[],
            excluded: &[],
        },
        &[held[0].id.clone()],
        now_ms(),
    )
    .expect("restore");
    assert_eq!(format!("{:?}", results[0].outcome), "Restored");
    assert_eq!(fs::metadata(&cache).expect("back").len(), 2_000_000);
}

#[test]
fn a_removed_apps_leftover_moves_through_preview_into_quarantine_and_restores() {
    let home = Home::new("app-leftover");
    let data = home.write(
        "Library/Application Support/com.example.Gone/data",
        2_000_000,
    );
    let prefs = home.write("Library/Preferences/com.example.Gone.plist", 1_000);
    let history = home.history();
    let home_path = home.home();
    let bundle = crate::apps::Bundle {
        id: "a1".to_owned(),
        path: home_path.join("Applications/Gone.app"),
        stat: stat_path(&data).expect("stat"),
        name: "Gone".to_owned(),
        bundle_id: Some("com.example.Gone".to_owned()),
        apple: false,
    };
    let cancel = AtomicBool::new(false);
    let items = crate::apps::leftovers_for_removed(&bundle, &home_path, now_ms(), &cancel);
    assert_eq!(items.len(), 2);
    let intake = super::intake::Intake {
        home: &home_path,
        running: &[],
        open: Some(&NOTHING_OPEN),
        store_dev: history.store.volume(),
        now_ms: now_ms(),
        excluded: &[],
    };
    let candidates = intake.app_items(items);
    assert!(candidates
        .iter()
        .all(|item| item.category() == policy::Category::Apps
            && item.assessment.eligibility == Eligibility::Eligible
            && item.assessment.risk == Risk::Low));
    let state = CleanupState::default();
    let ids = state.add_sent(candidates);

    // While the app is open again, its data isn't planned.
    let running = [RunningBundle {
        id: "com.example.Gone".to_owned(),
        name: "Gone".to_owned(),
    }];
    let open_again = checks(&home_path, &history, &cancel, &running);
    let preview = execute::create_preview(&state, &ids, &open_again, now_ms());
    assert!(preview
        .items
        .iter()
        .all(|item| item.state == PlannedState::NotEligible));

    let checks = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&state, &ids, &checks, now_ms());
    assert!(preview
        .items
        .iter()
        .all(|item| item.state == PlannedState::Planned));
    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "sent",
        now_ms,
        |_| {},
    );
    let ApproveOutcome::Finished { items: moved, .. } = outcome else {
        panic!("finished: {outcome:?}");
    };
    assert!(moved.iter().all(|item| item.state == MoveState::Moved));
    assert!(!data.exists() && !prefs.exists());

    let held = history.db.held_items().expect("held");
    let held_ids: Vec<String> = held.iter().map(|item| item.id.clone()).collect();
    let results = crate::quarantine::restore_items(
        &history.db,
        &history.store,
        &crate::quarantine::Scope {
            home: &home_path,
            temp: None,
            added: &[],
            excluded: &[],
        },
        &held_ids,
        now_ms(),
    )
    .expect("restore");
    assert!(results
        .iter()
        .all(|result| format!("{:?}", result.outcome) == "Restored"));
    assert_eq!(fs::metadata(&data).expect("back").len(), 2_000_000);
    assert!(prefs.exists());
}

#[test]
fn something_you_chose_moves_only_while_nothing_in_it_is_open_and_restores() {
    let home = Home::new("chosen");
    let video = home.write("Movies/holiday/clip.mov", 3_000_000);
    let history = home.history();
    let home_path = home.home();
    let state = crate::storage::explorer::ExplorerState::default();
    let cancel = AtomicBool::new(false);
    let root = state
        .list(&home_path, None, &[], &cancel, &mut |_| {})
        .expect("home");
    let movies = root
        .entries
        .iter()
        .find(|entry| entry.name == "Movies")
        .expect("Movies");
    let movies = state
        .list(&home_path, Some(&movies.id), &[], &cancel, &mut |_| {})
        .expect("Movies listed");
    let holiday = movies
        .entries
        .iter()
        .find(|entry| entry.name == "holiday")
        .expect("holiday");
    assert!(holiday.selectable);
    let chosen = state.chosen(std::slice::from_ref(&holiday.id));

    let open = OpenPaths::from_paths(vec![video.clone()]);
    let intake = |open: Option<&'static OpenPaths>| super::intake::Intake {
        home: &home_path,
        running: &[],
        open,
        store_dev: history.store.volume(),
        now_ms: now_ms(),
        excluded: &[],
    };
    let leaked: &'static OpenPaths = Box::leak(Box::new(open));
    let held = intake(Some(leaked)).chosen_items(chosen.clone());
    assert_eq!(held[0].assessment.why_not, Some(WhyNot::OpenInApp));
    let candidates = intake(Some(&NOTHING_OPEN)).chosen_items(chosen);
    assert_eq!(candidates[0].category(), policy::Category::Chosen);
    assert_eq!(candidates[0].assessment.eligibility, Eligibility::Eligible);

    let review = CleanupState::default();
    let ids = review.add_sent(candidates);
    // Opened in an app since it was sent: approval reads that again and leaves it.
    let now_open = Checks {
        open: Some(leaked),
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview = execute::create_preview(&review, &ids, &now_open, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
    let checks = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&review, &ids, &checks, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Planned);
    let outcome = execute::approve(
        &review,
        &preview.id,
        &checks,
        &history,
        "sent",
        now_ms,
        |_| {},
    );
    assert!(
        matches!(outcome, ApproveOutcome::Finished { .. }),
        "{outcome:?}"
    );
    assert!(!video.exists());
    let held = history.db.held_items().expect("held");
    let results = crate::quarantine::restore_items(
        &history.db,
        &history.store,
        &crate::quarantine::Scope {
            home: &home_path,
            temp: None,
            added: &[],
            excluded: &[],
        },
        &[held[0].id.clone()],
        now_ms(),
    )
    .expect("restore");
    assert_eq!(format!("{:?}", results[0].outcome), "Restored");
    assert_eq!(fs::metadata(&video).expect("back").len(), 3_000_000);
}

#[test]
fn a_folder_containing_an_excluded_folder_is_never_measured_or_offered() {
    let home = Home::new("excluded-inside");
    home.write("Library/Caches/com.example.Google/Private/blob", 2_000_000);
    home.write("Library/Caches/com.example.Google/Public/blob", 2_000_000);
    home.write("Library/Caches/com.example.Other/blob", 2_000_000);
    let history = home.history();
    let excluded = [home
        .home()
        .join("Library/Caches/com.example.Google/Private")];
    let preferences = crate::preferences::Preferences::default();
    let keep = |item: &Candidate| kept_by_preferences(item, &preferences, &excluded, now_ms());
    let outcome = run_scan_keeping(&home, &history, ScanMode::Standard, &[], &keep, &excluded);
    let names: Vec<&str> = outcome
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    // Moving com.example.Google would move the excluded folder inside it.
    assert_eq!(names, ["com.example.Other"]);
    // And preview or approval refuses it if it was found before the exclusion.
    let parent = home.home().join("Library/Caches/com.example.Google");
    assert!(crate::preferences::touches_excluded(&parent, &excluded));
    assert!(!crate::preferences::touches_excluded(
        &home.home().join("Library/Caches/com.example.Other"),
        &excluded
    ));
}

#[test]
fn preview_judges_preferences_on_the_item_as_it_is_now() {
    let home = Home::new("fresh-facts");
    let blob = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let folder = blob.parent().expect("folder").to_path_buf();
    age(&blob, 30 * DAY);
    age(&folder, 30 * DAY);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let item = by_name(&items, "com.example.App").clone();
    let home_path = home.home();
    let cancel = AtomicBool::new(false);

    // Smaller than the minimum now, though it wasn't at the scan.
    let at_least_1mb = |candidate: &Candidate| candidate.bytes >= 1_000_000;
    let min_size = Checks {
        keep: &at_least_1mb,
        ..checks(&home_path, &history, &cancel, &[])
    };
    fs::OpenOptions::new()
        .write(true)
        .open(&blob)
        .expect("open")
        .set_len(10)
        .expect("shrink");
    let preview =
        execute::create_preview(&state, std::slice::from_ref(&item.id), &min_size, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);

    // Changed since the scan, though the scan saw it untouched for 30 days.
    let cutoff = now_ms() - 86_400_000;
    let not_recent = |candidate: &Candidate| candidate.modified_ms.is_some_and(|ms| ms < cutoff);
    let recent = Checks {
        keep: &not_recent,
        ..checks(&home_path, &history, &cancel, &[])
    };
    age(&folder, Duration::ZERO);
    let preview =
        execute::create_preview(&state, std::slice::from_ref(&item.id), &recent, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
}

#[test]
fn unreadable_exclusions_are_an_error_never_an_empty_list() {
    let home = Home::new("exclusions-unreadable");
    let broken = History {
        db: Database::failed(Unavailable::Corrupt),
        store: Store::new(home.root.join("data")),
    };
    assert!(broken.exclusions().is_err());
    let working = home.history();
    assert_eq!(
        working.exclusions().expect("readable"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn show_in_finder_refuses_an_item_excluded_after_the_scan() {
    let home = Home::new("reveal-excluded");
    home.write("Library/Caches/com.example.App/Inner/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let item = by_name(&items, "com.example.App").clone();
    assert!(revealable(&item, &[]));
    // Excluded itself, inside an excluded folder, or holding one: refused (the exclusion is
    // checked before the path is read).
    assert!(!revealable(&item, std::slice::from_ref(&item.path)));
    assert!(!revealable(&item, &[home.home().join("Library/Caches")]));
    assert!(!revealable(&item, &[item.path.join("Inner")]));
    // An unrelated exclusion doesn't matter.
    assert!(revealable(&item, &[home.home().join("Downloads")]));
}

#[test]
fn the_lockfile_search_never_looks_into_excluded_folders() {
    use super::intake::has_lockfile;
    let home = Home::new("lockfile-excluded");
    home.write("Code/web/package-lock.json", 10);
    home.write("Code/web/node_modules/pkg", 10);
    home.write("Code/mono/pnpm-lock.yaml", 10);
    home.write("Code/mono/pnpm-workspace.yaml", 30);
    fs::write(
        home.home().join("Code/mono/pnpm-workspace.yaml"),
        "packages:\n  - 'apps/*'\n",
    )
    .expect("workspace");
    home.write("Code/mono/apps/site/node_modules/pkg", 10);
    let root = home.home();
    let web = root.join("Code/web");
    let site = root.join("Code/mono/apps/site");
    assert!(has_lockfile(&web, &root, &[]));
    assert!(has_lockfile(&site, &root, &[]));
    // The project excluded: not opened, so no lockfile is assumed.
    assert!(!has_lockfile(&web, &root, std::slice::from_ref(&web)));
    // The lockfile itself, or the workspace manifest above, excluded: not read.
    assert!(!has_lockfile(&web, &root, &[web.join("package-lock.json")]));
    assert!(!has_lockfile(
        &site,
        &root,
        &[root.join("Code/mono/pnpm-workspace.yaml")]
    ));
    // The workspace root excluded: the search stops before it.
    assert!(!has_lockfile(&site, &root, &[root.join("Code/mono")]));
}

#[test]
fn an_item_excluded_after_the_scan_is_refused_without_being_measured_again() {
    let home = Home::new("excluded-unmeasured");
    let blob = home.write("Library/Caches/com.example.App/Inner/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let item = by_name(&items, "com.example.App").clone();
    // It shrinks after the scan; a preview that measured it again would report the new size.
    fs::OpenOptions::new()
        .write(true)
        .open(&blob)
        .expect("open")
        .set_len(10)
        .expect("shrink");
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let excluded = [blob.parent().expect("inner").to_path_buf()];
    let now_excluded = Checks {
        excluded: &excluded,
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview = execute::create_preview(
        &state,
        std::slice::from_ref(&item.id),
        &now_excluded,
        now_ms(),
    );
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
    assert_eq!(preview.items[0].bytes, item.bytes);
}

#[test]
fn the_preferences_are_checked_again_immediately_before_each_move() {
    let home = Home::new("per-move");
    let first = home.write("Library/Caches/com.example.One/blob", 2_000_000);
    let second = home.write("Library/Caches/com.example.Two/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let ids = vec![
        by_name(&items, "com.example.One").id.clone(),
        by_name(&items, "com.example.Two").id.clone(),
    ];
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let preview = execute::create_preview(
        &state,
        &ids,
        &checks(&home_path, &history, &cancel, &[]),
        now_ms(),
    );
    // Kept through the approval's checks of both items and the first item's move, then the
    // second item changes (as a recent write would) just before its turn.
    let calls = std::cell::Cell::new(0);
    let changes_late = |_: &Candidate| {
        calls.set(calls.get() + 1);
        calls.get() <= 3
    };
    let late = Checks {
        keep: &changes_late,
        ..checks(&home_path, &history, &cancel, &[])
    };
    let outcome = execute::approve(
        &state,
        &preview.id,
        &late,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    let ApproveOutcome::Finished { items: moved, .. } = outcome else {
        panic!("finished: {outcome:?}");
    };
    assert_eq!(moved[0].state, MoveState::Moved);
    assert_eq!(
        moved[1].state,
        MoveState::Failed {
            reason: execute::MoveFailure::Changed
        }
    );
    assert!(!first.exists());
    assert!(second.exists());
}

#[test]
fn a_folder_excluded_after_the_scan_is_never_planned_or_moved() {
    let home = Home::new("excluded-later");
    let cache = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let item = by_name(&items, "com.example.App").clone();
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let folder = cache.parent().expect("folder").to_path_buf();
    let excluded = |candidate: &Candidate| !candidate.path.starts_with(&folder);

    // Excluded before the preview: left out of it.
    let now_excluded = Checks {
        keep: &excluded,
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview = execute::create_preview(
        &state,
        std::slice::from_ref(&item.id),
        &now_excluded,
        now_ms(),
    );
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);

    // Excluded after the preview was made: approval finds it changed and moves nothing.
    let allowed = checks(&home_path, &history, &cancel, &[]);
    let preview =
        execute::create_preview(&state, std::slice::from_ref(&item.id), &allowed, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Planned);
    let outcome = execute::approve(
        &state,
        &preview.id,
        &now_excluded,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    assert!(matches!(outcome, ApproveOutcome::Stale { .. }));
    assert!(cache.exists());
}

#[test]
fn a_change_after_the_preview_blocks_approval_and_nothing_moves() {
    let home = Home::new("stale");
    let first = home.write("Library/Caches/com.example.One/blob", 2_000_000);
    let second = home.write("Library/Caches/com.example.Two/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let ids = vec![
        by_name(&items, "com.example.One").id.clone(),
        by_name(&items, "com.example.Two").id.clone(),
    ];
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let checks = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&state, &ids, &checks, now_ms());

    // One item grows after the preview.
    fs::write(second.with_file_name("more"), vec![1_u8; 500_000]).expect("grow");
    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    let ApproveOutcome::Stale { changed } = outcome else {
        panic!("stale: {outcome:?}");
    };
    assert_eq!(changed, [ids[1].clone()]);
    assert!(first.exists() && second.exists(), "nothing moved");
    assert!(history.db.held_items().expect("held").is_empty());
    // The stale preview can't be approved again; a rebuilt one can.
    assert_eq!(
        execute::approve(
            &state,
            &preview.id,
            &checks,
            &history,
            "standard",
            now_ms,
            |_| {}
        ),
        ApproveOutcome::NoPreview
    );
    let rebuilt = execute::create_preview(&state, &ids, &checks, now_ms());
    assert!(rebuilt
        .items
        .iter()
        .all(|item| item.state == PlannedState::Planned));
}

#[test]
fn a_replaced_item_is_left_out_of_the_preview_and_needs_a_new_scan() {
    let home = Home::new("replaced");
    let cache = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let id = by_name(&items, "com.example.App").id.clone();
    // The replacement is made before the old folder goes, so it has a new inode even on
    // ext4, which reuses a freed one at once.
    let fresh = home.write("Library/Caches/com.example.App.new/other", 2_000_000);
    let folder = cache.parent().expect("folder");
    fs::remove_dir_all(folder).expect("remove");
    fs::rename(fresh.parent().expect("new folder"), folder).expect("swap in");
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let checks = checks(&home_path, &history, &cancel, &[]);

    let preview = execute::create_preview(&state, std::slice::from_ref(&id), &checks, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Changed);
    let finding = state
        .review()
        .findings
        .into_iter()
        .find(|finding| finding.id == id)
        .expect("still listed");
    assert_eq!(finding.eligibility, Eligibility::Rescan);
    // The preview binds nothing, so approving moves nothing.
    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    assert!(matches!(outcome, ApproveOutcome::Finished { ref items, .. } if items.is_empty()));
    assert!(home
        .home()
        .join("Library/Caches/com.example.App/other")
        .exists());
}

#[test]
fn nothing_moves_without_a_working_history() {
    let home = Home::new("nohistory");
    let cache = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = History {
        db: Database::failed(Unavailable::Corrupt),
        store: Store::new(home.root.join("data")),
    };
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let id = by_name(&items, "com.example.App").id.clone();
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let checks = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&state, std::slice::from_ref(&id), &checks, now_ms());

    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    assert_eq!(
        outcome,
        ApproveOutcome::HistoryUnavailable {
            reason: Unavailable::Corrupt
        }
    );
    assert!(cache.exists());
}

#[test]
fn an_app_opened_after_the_scan_keeps_its_cache_out_of_the_preview() {
    let home = Home::new("opened");
    let cache = home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let id = by_name(&items, "com.example.App").id.clone();
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let running = [RunningBundle {
        id: "com.example.App".to_owned(),
        name: "Example".to_owned(),
    }];
    let checks = checks(&home_path, &history, &cancel, &running);

    let preview = execute::create_preview(&state, std::slice::from_ref(&id), &checks, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::NotEligible);
    assert!(cache.exists());
}

#[test]
fn inspect_only_and_out_of_bounds_items_are_never_planned() {
    let home = Home::new("bounds");
    let file = home.write("Downloads/big.dmg", 10_000);
    let elsewhere = home.write("Documents/notes/keep.txt", 10_000);
    let history = home.history();
    let state = CleanupState::default();
    let stat_of = |path: &Path| stat_path(path).expect("stat");
    let make = |kind: Kind, path: PathBuf, stat: FileStat, assessment: Assessment| Candidate {
        id: new_id(now_ms()),
        kind,
        identity: crate::quarantine::Identity::of(&stat),
        name: display_name_of(&path),
        location: String::new(),
        path,
        bytes: 1,
        files: 1,
        lower_bound: false,
        modified_ms: None,
        assessment,
        facts: StoredFacts::default(),
        source: Source::Scan,
        project: None,
    };
    let large = make(
        Kind::LargeFile,
        file.clone(),
        stat_of(&file),
        policy::assess(&Kind::LargeFile, &facts("big.dmg")),
    );
    // A forged candidate: claims to be an app cache but points at a user folder.
    let notes = elsewhere.parent().expect("folder").to_path_buf();
    let forged = make(
        Kind::AppCache,
        notes.clone(),
        stat_of(&notes),
        policy::assess(&Kind::AppCache, &facts("com.example.App")),
    );
    let ids = vec![large.id.clone(), forged.id.clone()];
    state.set_scan(
        ScanInfo {
            mode: ScanMode::Deep,
            started_at_ms: 0,
            cancelled: false,
        },
        vec![large, forged],
    );
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let checks = checks(&home_path, &history, &cancel, &[]);
    let preview = execute::create_preview(&state, &ids, &checks, now_ms());
    assert!(preview
        .items
        .iter()
        .all(|item| item.state == PlannedState::NotEligible));
    let _ = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "deep",
        now_ms,
        |_| {},
    );
    assert!(file.exists() && elsewhere.exists());
}

#[test]
fn a_build_folder_from_an_added_folder_outside_home_records_it_and_can_be_restored() {
    let home = Home::new("added-root");
    let work = home.root.join("work");
    let modules = work.join("app/node_modules");
    fs::create_dir_all(&modules).expect("folders");
    fs::write(modules.join("blob"), vec![7_u8; 1_000]).expect("file");
    let history = home.history();
    let state = CleanupState::default();
    let kind = Kind::BuildFolder {
        name: "node_modules".to_owned(),
    };
    let facts = StoredFacts {
        lockfile: true,
        ..StoredFacts::default()
    };
    let stat = stat_path(&modules).expect("stat");
    let item = Candidate {
        id: new_id(now_ms()),
        assessment: policy::assess(
            &kind,
            &Facts {
                lockfile: true,
                ..self::facts("node_modules")
            },
        ),
        kind,
        identity: crate::quarantine::Identity::of(&stat),
        name: "node_modules".to_owned(),
        location: String::new(),
        path: modules.clone(),
        bytes: 1_000,
        files: 1,
        lower_bound: false,
        modified_ms: None,
        facts,
        source: Source::Scan,
        project: None,
    };
    state.set_scan(
        ScanInfo {
            mode: ScanMode::Standard,
            started_at_ms: 0,
            cancelled: false,
        },
        vec![item.clone()],
    );
    let home_path = home.home();
    let cancel = AtomicBool::new(false);
    let added = [work.clone()];
    let checks = Checks {
        added: &added,
        ..checks(&home_path, &history, &cancel, &[])
    };
    let preview =
        execute::create_preview(&state, std::slice::from_ref(&item.id), &checks, now_ms());
    assert_eq!(preview.items[0].state, PlannedState::Planned);
    let outcome = execute::approve(
        &state,
        &preview.id,
        &checks,
        &history,
        "standard",
        now_ms,
        |_| {},
    );
    assert!(matches!(outcome, ApproveOutcome::Finished { .. }));
    assert!(!modules.exists());

    // The added folder is recorded with the item. After a restart nothing has been chosen in
    // the panel yet: the record alone doesn't allow the restore, it names the folder to ask
    // for.
    let held = history.db.held_items().expect("held");
    assert_eq!(held[0].root.as_deref(), Some(work.as_path()));
    let after_restart = crate::quarantine::Scope {
        home: &home_path,
        temp: None,
        added: &[],
        excluded: &[],
    };
    assert!(!after_restart.allows(&held[0]));
    assert_eq!(after_restart.needed_folder(&held[0]), Some(work.as_path()));
    let results = crate::quarantine::restore_items(
        &history.db,
        &history.store,
        &after_restart,
        std::slice::from_ref(&held[0].id),
        now_ms(),
    )
    .expect("restore");
    assert_eq!(format!("{:?}", results[0].outcome), "NeedsFolder");
    assert!(!modules.exists());

    // Once the user chooses the folder again, it goes back.
    let scope = crate::quarantine::Scope {
        home: &home_path,
        temp: None,
        added: &added,
        excluded: &[],
    };
    assert!(scope.allows(&held[0]));
    let results = crate::quarantine::restore_items(
        &history.db,
        &history.store,
        &scope,
        std::slice::from_ref(&held[0].id),
        now_ms(),
    )
    .expect("restore");
    assert_eq!(format!("{:?}", results[0].outcome), "Restored");
    assert!(modules.join("blob").exists());
}

#[test]
fn protected_folders_are_read_only_after_access_is_requested() {
    let home = Home::new("scope");
    home.write("Downloads/file", 10);
    let db = Database::in_memory();
    let folders = scope::folders(&home.home(), &db, &[], false);
    let downloads = folders
        .iter()
        .find(|folder| folder.id == scope::FolderId::Downloads)
        .expect("downloads");
    assert_eq!(
        (downloads.access, downloads.path.as_str()),
        (Access::NotRequested, "~/Downloads")
    );
    let desktop = folders
        .iter()
        .find(|folder| folder.id == scope::FolderId::Desktop)
        .expect("desktop");
    assert_eq!(desktop.access, Access::Missing);
    assert!(scope::large_roots(&home.home(), &db, &[], false).is_empty());

    assert_eq!(
        scope::request_access(&home.home(), &db, ProtectedFolder::Downloads, 1),
        Access::Granted
    );
    assert_eq!(
        scope::access_of(&db, ProtectedFolder::Downloads, false),
        Access::Granted
    );
    assert_eq!(
        scope::large_roots(&home.home(), &db, &[], false),
        [home.home().join("Downloads")]
    );
}

#[test]
fn full_disk_access_covers_every_protected_folder_without_asking() {
    let home = Home::new("fda");
    home.write("Downloads/file", 10);
    home.write("Documents/file", 10);
    let db = Database::in_memory();
    let folders = scope::folders(&home.home(), &db, &[], true);
    let access = |id| {
        folders
            .iter()
            .find(|folder| folder.id == id)
            .map(|folder| folder.access)
    };
    assert_eq!(access(scope::FolderId::Downloads), Some(Access::ByFullDisk));
    assert_eq!(access(scope::FolderId::Documents), Some(Access::ByFullDisk));
    // The frontend sees it as `fullDiskAccess`.
    assert_eq!(
        serde_json::to_value(Access::ByFullDisk).expect("json"),
        serde_json::json!("fullDiskAccess")
    );
    // A folder that doesn't exist is still missing, whatever the access.
    assert_eq!(access(scope::FolderId::Movies), Some(Access::Missing));
    assert_eq!(
        scope::large_roots(&home.home(), &db, &[], true),
        [home.home().join("Downloads"), home.home().join("Documents")]
    );
}

#[test]
fn sent_items_join_the_review_and_replace_the_scans_copy() {
    let home = Home::new("sent");
    home.write("Library/Caches/com.example.App/blob", 2_000_000);
    let history = home.history();
    let state = CleanupState::default();
    let items = scanned(&home, &history, &state);
    let mut sent = by_name(&items, "com.example.App").clone();
    sent.id = new_id(now_ms());
    sent.source = Source::Developer;
    let ids = state.add_sent(vec![sent.clone()]);
    assert_eq!(ids, [sent.id.clone()]);
    let review = state.review();
    let same: Vec<&Finding> = review
        .findings
        .iter()
        .filter(|finding| finding.name == "com.example.App")
        .collect();
    assert_eq!(same.len(), 1);
    assert_eq!(same[0].source, Source::Developer);
}

#[test]
fn workspace_patterns_decide_which_packages_a_root_lockfile_covers() {
    use super::intake::{pnpm_packages, workspace_member};
    let yaml = "# comment\npackages:\n  - 'packages/*'\n  - \"apps/**\"\n  - '!packages/old-*'\ncatalog:\n  - not-a-package\n";
    let patterns = pnpm_packages(yaml);
    assert_eq!(patterns, ["packages/*", "apps/**", "!packages/old-*"]);
    assert!(workspace_member(&patterns, "packages/ui"));
    assert!(workspace_member(&patterns, "apps/web/site"));
    assert!(!workspace_member(&patterns, "packages/old-ui"));
    assert!(!workspace_member(&patterns, "packages/ui/nested"));
    assert!(!workspace_member(&patterns, "tools/script"));
    assert!(!workspace_member(&[], "packages/ui"));
    assert!(workspace_member(
        &["./libs/*-kit/".to_owned()],
        "libs/ui-kit"
    ));

    // A hostile manifest: tens of thousands of `**` segments collapse to one, so matching
    // stays instant and the stack untouched.
    let hostile = format!("{}x", "**/".repeat(60_000));
    let started = std::time::Instant::now();
    assert!(workspace_member(&[hostile], "a/b/c/x"));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    // Beyond the limits the whole workspace includes nothing (fail closed), so an exclusion
    // past the limit can never be dropped while its include still counts.
    let long = vec!["*"; 40].join("/");
    let deep = vec!["a"; 40].join("/");
    assert!(!workspace_member(&[long], &deep));
    let mut many = vec!["packages/**".to_owned()];
    many.extend(vec!["nope/*".to_owned(); 300]);
    many.push("!packages/private/**".to_owned());
    assert!(!workspace_member(&many, "packages/private/app"));
    assert!(!workspace_member(&many, "packages/ui"));
    // An over-long exclusion fails the workspace closed too.
    let overlong_exclusion = vec![
        "packages/**".to_owned(),
        format!("!packages/{}", vec!["x"; 40].join("/")),
    ];
    assert!(!workspace_member(&overlong_exclusion, "packages/ui"));
}

#[test]
fn build_folders_in_added_folders_keep_the_home_folders_protections() {
    let home = Path::new("/Users/someone");
    let added = |root: &str| vec![PathBuf::from(root)];
    let check = |path: &str, root: &str| {
        policy::build_folder_in_added(Path::new(path), "node_modules", home, &added(root))
    };
    // An ordinary added folder, inside or outside the home folder.
    assert!(check(
        "/Users/someone/Work/web/node_modules",
        "/Users/someone/Work"
    ));
    assert!(check("/Volumes/Work/web/node_modules", "/Volumes/Work"));
    // The disk itself, or a path that isn't plain, is never an added folder: approval and
    // restore share this rule.
    assert!(!check("/srv/web/node_modules", "/"));
    assert!(!check("/srv/web/node_modules", "/opt/../srv"));
    // Adding a protected folder doesn't make what is inside it eligible.
    assert!(!check(
        "/Users/someone/Library/Application Support/App/web/node_modules",
        "/Users/someone/Library/Application Support"
    ));
    assert!(!check(
        "/Users/someone/Library/Containers/App/web/node_modules",
        "/Users/someone/Library/Containers"
    ));
    assert!(!check(
        "/Users/someone/Library/Other/node_modules",
        "/Users/someone/Library"
    ));
    assert!(!check(
        "/Users/someone/.Trash/web/node_modules",
        "/Users/someone/.Trash"
    ));
    assert!(!check(
        "/Users/someone/.ssh/x/node_modules",
        "/Users/someone/.ssh"
    ));
}
