use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use super::*;
use crate::cleanup::policy::{self, Kind};

const DAY: Duration = Duration::from_secs(86_400);

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "kleari-apps-{name}-{}-{}",
            std::process::id(),
            new_id(0)
        ));
        fs::create_dir_all(&root).expect("home");
        // The real path, as the scan opens it without following symlinks (/tmp, /var).
        Self(fs::canonicalize(&root).expect("real path"))
    }

    fn write(&self, relative: &str, bytes: usize) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(&path, vec![1_u8; bytes]).expect("write");
        path
    }

    fn app(&self, name: &str, bundle_id: &str) {
        self.app_in("Applications", name, bundle_id);
    }

    /// An app bundle in `folder` (relative to the home folder).
    fn app_in(&self, folder: &str, name: &str, bundle_id: &str) {
        self.write(&format!("{folder}/{name}.app/Contents/Info.plist"), 0);
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleShortVersionString</key><string>2.1</string>
</dict></plist>"#
        );
        fs::write(
            self.0
                .join(format!("{folder}/{name}.app/Contents/Info.plist")),
            plist,
        )
        .expect("plist");
        self.write(
            &format!("{folder}/{name}.app/Contents/MacOS/{name}"),
            40_000,
        );
    }
}

fn names(result: &AppsScan) -> Vec<&str> {
    let mut names: Vec<&str> = result.apps.iter().map(|app| app.name.as_str()).collect();
    names.sort_unstable();
    names
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Sets the modification time of `path` and everything inside to `ago` before now.
fn age(path: &Path, ago: Duration) {
    let when = SystemTime::now() - ago;
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read") {
            age(&entry.expect("entry").path(), ago);
        }
    }
    let file = fs::File::open(path).expect("open");
    file.set_modified(when).expect("set time");
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis(),
    )
    .expect("fits")
}

fn run(home: &Home, running: &[RunningBundle]) -> (AppsScan, Vec<Bundle>, Vec<ReviewItem>) {
    run_open(home, running, &OpenPaths::NOTHING)
}

fn run_open(
    home: &Home,
    running: &[RunningBundle],
    open: &OpenPaths,
) -> (AppsScan, Vec<Bundle>, Vec<ReviewItem>) {
    let cancel = AtomicBool::new(false);
    scan(
        &[home.0.join("Applications")],
        &[],
        Some(&home.0),
        running,
        Some(open),
        now_ms(),
        &cancel,
    )
}

fn orphan_names(result: &AppsScan) -> Vec<&str> {
    let mut names: Vec<&str> = result
        .orphans
        .iter()
        .map(|left| left.name.as_str())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn a_leftover_something_has_open_or_runs_from_is_not_offered() {
    let home = Home::new("open");
    let held = home.write("Library/Application Support/com.gone.Held/db.sqlite", 500);
    age(held.parent().expect("folder"), 60 * DAY);
    let helper = home.write(
        "Library/Application Support/com.gone.Helper/bin/helper",
        500,
    );
    age(
        &home.0.join("Library/Application Support/com.gone.Helper"),
        60 * DAY,
    );
    let idle = home.write("Library/Application Support/com.gone.Idle/x", 500);
    age(idle.parent().expect("folder"), 60 * DAY);
    // A file open inside one, and a program running from inside another.
    let open = OpenPaths::from_paths(vec![held, helper]);
    let (result, _, review) = run_open(&home, &[], &open);
    assert_eq!(orphan_names(&result), ["com.gone.Idle"]);
    assert_eq!(review.len(), 1);
}

#[test]
fn a_launch_agent_is_offered_only_when_its_program_is_gone() {
    let home = Home::new("agents");
    let agent = |label: &str, program: &Path| {
        let path = home.0.join(format!("Library/LaunchAgents/{label}.plist"));
        fs::create_dir_all(path.parent().expect("folder")).expect("folders");
        fs::write(
            &path,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array><string>{}</string></array>
</dict></plist>"#,
                program.display()
            ),
        )
        .expect("agent");
        age(&path, 60 * DAY);
    };
    // A command-line tool still installed, without an app: its agent is in use.
    let tool = home.write("bin/tool", 100);
    agent("com.cli.Tool", &tool);
    agent("com.gone.Agent", &home.0.join("bin/removed"));
    let unreadable = home.write("Library/LaunchAgents/com.gone.Broken.plist", 10);
    age(&unreadable, 60 * DAY);
    let (result, _, _) = run(&home, &[]);
    assert_eq!(orphan_names(&result), ["com.gone.Agent.plist"]);
}

fn snapshot(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read") {
            let entry = entry.expect("entry");
            let meta = entry.metadata().expect("meta");
            if meta.is_dir() {
                stack.push(entry.path());
            }
            out.push((entry.path(), meta.len()));
        }
    }
    out.sort();
    out
}

#[test]
fn installed_apps_list_their_leftovers_without_sending_them() {
    let home = Home::new("installed");
    home.app("Fake", "com.example.Fake");
    home.write(
        "Library/Application Support/com.example.Fake/data.db",
        30_000,
    );
    home.write("Library/Application Support/Fake/state.json", 2_000);
    home.write("Library/Preferences/com.example.Fake.plist", 500);
    home.write(
        "Library/Preferences/ByHost/com.example.Fake.0A1B2C3D-0000-1111-2222-333344445555.plist",
        500,
    );
    home.write("Library/Caches/com.example.Fake/blob", 10_000);
    // Another app's data that starts the same isn't Fake's.
    home.write("Library/Application Support/com.example.FakeOther/x", 500);
    let before = snapshot(&home.0);

    let (result, bundles, review) = run(&home, &[]);
    assert_eq!(snapshot(&home.0), before, "the scan wrote nothing");
    assert_eq!(result.status, ScanStatus::Completed);
    let app = result
        .apps
        .iter()
        .find(|app| app.name == "Fake")
        .expect("Fake listed");
    assert_eq!(app.bundle_id.as_deref(), Some("com.example.Fake"));
    assert_eq!(app.version.as_deref(), Some("2.1"));
    assert!(!app.apple && !app.running);
    assert!(app.bytes >= 40_000);
    let mut names: Vec<&str> = app
        .leftovers
        .iter()
        .map(|left| left.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "Fake",
            "com.example.Fake",
            "com.example.Fake",
            "com.example.Fake.0A1B2C3D-0000-1111-2222-333344445555.plist",
            "com.example.Fake.plist",
        ]
    );
    // An installed app's leftovers go to the review only once it is removed.
    assert!(app.leftovers.iter().all(|left| left.review_id.is_none()));
    assert!(review
        .iter()
        .all(|item| !matches!(&item.kind, Kind::AppLeftover { .. })));
    assert!(bundles.iter().any(|bundle| bundle.name == "Fake"));
}

#[test]
fn leftovers_of_apps_that_are_gone_are_offered_only_when_unused_for_a_month() {
    let home = Home::new("orphans");
    home.app("Kept", "com.vendor.Kept");
    let gone = home.write(
        "Library/Application Support/com.gone.Editor/settings.json",
        5_000,
    );
    age(gone.parent().expect("folder"), 60 * DAY);
    let prefs = home.write("Library/Preferences/com.gone.Editor.plist", 500);
    age(&prefs, 60 * DAY);
    // Changed last week: something still uses it.
    home.write("Library/Application Support/com.recent.Tool/state", 500);
    // The same vendor as an installed app: a shared helper, not a leftover.
    let shared = home.write("Library/Application Support/com.vendor.Updater/x", 500);
    age(shared.parent().expect("folder"), 60 * DAY);
    // Apple's, and names that aren't bundle IDs, are never offered.
    let apple = home.write("Library/Preferences/com.apple.finder.plist", 500);
    age(&apple, 60 * DAY);
    let plain = home.write("Library/Application Support/Some Folder/x", 500);
    age(plain.parent().expect("folder"), 60 * DAY);
    // A running app's data isn't offered even though it isn't installed in a scanned folder.
    let running_data = home.write("Library/Application Support/com.elsewhere.Running/x", 500);
    age(running_data.parent().expect("folder"), 60 * DAY);
    let running = [RunningBundle {
        id: "com.elsewhere.Running".to_owned(),
        name: "Running".to_owned(),
    }];

    let (result, _, review) = run(&home, &running);
    let mut owners: Vec<(&str, &str)> = result
        .orphans
        .iter()
        .map(|left| (left.owner.as_str(), left.name.as_str()))
        .collect();
    owners.sort_unstable();
    assert_eq!(
        owners,
        [
            ("com.gone.editor", "com.gone.Editor"),
            ("com.gone.editor", "com.gone.Editor.plist"),
        ]
    );
    // Each one can be sent, and is in bounds for the policy that moves it.
    for orphan in &result.orphans {
        let id = orphan.review_id.as_deref().expect("sendable");
        let item = review.iter().find(|item| item.id == id).expect("kept");
        assert!(!item.owner_removed);
        assert!(policy::within_bounds(
            &item.kind,
            &item.path,
            &home.0,
            None,
            |_| Vec::new()
        ));
    }
}

#[test]
fn installers_in_downloads_and_on_the_desktop_are_offered() {
    let home = Home::new("installers");
    home.app("Fake", "com.example.Fake");
    home.write("Downloads/Fake-2.1.dmg", 20_000);
    home.write("Desktop/Tool.pkg", 10_000);
    home.write("Downloads/notes.pdf", 10_000);
    home.write("Downloads/deeper/Other.dmg", 10_000);

    let (result, _, review) = run(&home, &[]);
    let mut names: Vec<(&str, Option<&str>)> = result
        .installers
        .iter()
        .map(|installer| (installer.name.as_str(), installer.installed_app.as_deref()))
        .collect();
    names.sort_unstable();
    assert_eq!(names, [("Fake-2.1.dmg", Some("Fake")), ("Tool.pkg", None)]);
    for installer in &result.installers {
        let item = review
            .iter()
            .find(|item| item.id == installer.review_id)
            .expect("kept");
        assert_eq!(item.kind, Kind::Installer);
        assert!(policy::within_bounds(
            &item.kind,
            &item.path,
            &home.0,
            None,
            |_| Vec::new()
        ));
    }
}

#[test]
fn a_removed_apps_leftovers_are_read_again_and_marked_as_its_own() {
    let home = Home::new("removed");
    home.app("Fake", "com.example.Fake");
    home.write("Library/Containers/com.example.Fake/Data/x", 3_000);
    home.write(
        "Library/Group Containers/ABCDE12345.com.example.Fake/x",
        3_000,
    );
    home.write("Library/Preferences/com.example.Fake.plist", 500);
    let (_, bundles, _) = run(&home, &[]);
    let bundle = bundles
        .iter()
        .find(|bundle| bundle.name == "Fake")
        .expect("found");
    let cancel = AtomicBool::new(false);
    let items = leftovers_for_removed(bundle, &bundles, &[], &home.0, now_ms(), &cancel);
    let mut names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "ABCDE12345.com.example.Fake",
            "com.example.Fake",
            "com.example.Fake.plist"
        ]
    );
    for item in &items {
        assert!(item.owner_removed);
        assert!(policy::within_bounds(
            &item.kind,
            &item.path,
            &home.0,
            None,
            |_| Vec::new()
        ));
    }
}

#[test]
fn a_name_another_app_shares_doesnt_make_its_data_a_leftover() {
    let home = Home::new("shared-name");
    home.app("Code", "com.example.code");
    home.app_in("Applications/Tools", "Code", "com.other.code");
    home.write("Library/Application Support/Code/state.db", 3_000);
    home.write("Library/Application Support/com.example.code/x", 3_000);
    home.write("Library/Application Support/com.other.code/x", 3_000);
    let (result, bundles, _) = run(&home, &[]);
    let codes: Vec<_> = result
        .apps
        .iter()
        .filter(|app| app.name == "Code")
        .collect();
    assert_eq!(codes.len(), 2, "both copies listed");
    // In the scan, each offers only its bundle-ID folder, never the shared `Code`.
    for app in codes {
        let names: Vec<&str> = app
            .leftovers
            .iter()
            .map(|left| left.name.as_str())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("com."), "{names:?}");
    }

    let removed = bundles
        .iter()
        .find(|bundle| bundle.bundle_id.as_deref() == Some("com.example.code"))
        .expect("found");
    let others: Vec<Bundle> = bundles
        .iter()
        .filter(|bundle| bundle.id != removed.id)
        .cloned()
        .collect();
    let cancel = AtomicBool::new(false);
    let names = |installed: &[Bundle], running: &[RunningBundle]| {
        let mut names: Vec<String> =
            leftovers_for_removed(removed, installed, running, &home.0, now_ms(), &cancel)
                .into_iter()
                .map(|item| item.name)
                .collect();
        names.sort_unstable();
        names
    };
    // The other `Code` is still installed: only the bundle-ID folder is offered.
    assert_eq!(names(&others, &[]), ["com.example.code"]);
    // Nor while an app called `Code` runs (one without a bundle on the list).
    let running = [RunningBundle {
        id: "com.third.code".to_owned(),
        name: "code".to_owned(),
    }];
    assert_eq!(names(&[], &running), ["com.example.code"]);
    // With no other `Code` about, the name matches again.
    assert_eq!(names(&[], &[]), ["Code", "com.example.code"]);
}

#[test]
fn app_bundles_in_an_applications_folder_or_two_folders_below_are_in_bounds() {
    let folders = app_folders(Some(Path::new("/Users/someone")));
    let inside = |path: &str| bundle_in_bounds(Path::new(path), &folders);
    assert!(inside("/Applications/Fake.app"));
    assert!(inside("/Users/someone/Applications/Fake.app"));
    assert!(inside("/Applications/Suite/Fake.app"));
    assert!(inside("/Applications/Suite/Tools/Fake.app"));
    assert!(inside(
        "/Users/someone/Applications/Chrome Apps.localized/Web.app"
    ));
    assert!(!inside("/Applications/Fake"));
    assert!(!inside("/Applications/A/B/C/Fake.app"), "too deep");
    assert!(!inside("/Applications/Host.app/Contents/Helper.app"));
    assert!(!inside("/Applications/Host.app/Helper.app"));
    assert!(!inside("/Applications/Driver.bundle/Setup.app"));
    assert!(!inside("/Applications/.hidden/Fake.app"));
    assert!(inside("/Applications/Cisco Packet Tracer 9.0.1/Tracer.app"));
    assert!(!inside("/Applications/../Users/Fake.app"));
    assert!(!inside("/Applications/Suite/../Fake.app"));
    assert!(!inside("/ApplicationsX/Fake.app"));
    // macOS's own apps are listed but never in bounds for removal.
    assert!(!inside("/System/Applications/Notes.app"));
    assert!(!inside("/System/Applications/Utilities/Terminal.app"));
}

#[test]
fn an_apps_cache_or_leftover_finds_its_installed_app_for_the_icon() {
    let home = Home::new("owner");
    home.app("Fake", "com.example.Fake");
    let (_, bundles, review) = run(&home, &[]);
    let state = AppsState::default();
    state.finish(Some((bundles, review)));
    let fake = Some(home.0.join("Applications/Fake.app"));
    assert_eq!(state.owner_path(&Kind::AppCache, "com.example.Fake"), fake);
    assert_eq!(state.owner_path(&Kind::AppCache, "Fake"), fake);
    assert_eq!(
        state.owner_path(
            &Kind::AppLeftover {
                owner: "com.example.fake".to_owned()
            },
            "com.example.Fake.plist"
        ),
        fake
    );
    assert_eq!(state.owner_path(&Kind::AppCache, "com.example.Other"), None);
    assert_eq!(state.owner_path(&Kind::Installer, "Fake.dmg"), None);
}

#[test]
fn apps_in_subfolders_are_found_two_levels_deep_never_inside_bundles_or_symlinks() {
    let home = Home::new("subfolders");
    home.app("Top", "com.example.Top");
    home.app_in("Applications/Suite", "Editor", "com.example.Editor");
    home.app_in("Applications/Vendor/Tools", "Setup", "com.example.Setup");
    home.app_in("Applications/Web Apps.localized", "Web", "com.example.Web");
    home.app_in("Applications/A/B/C", "Deep", "com.example.Deep");
    home.app_in("Applications/Driver.bundle", "Inner", "com.example.Inner");
    home.app_in(
        "Applications/Top.app/Contents",
        "Helper",
        "com.example.Helper",
    );
    home.app_in("Applications/.hidden", "Hidden", "com.example.Hidden");
    home.app_in("Elsewhere", "Linked", "com.example.Linked");
    std::os::unix::fs::symlink(
        home.0.join("Elsewhere"),
        home.0.join("Applications/Linked Folder"),
    )
    .expect("symlink");
    // The folder is also given as an apps folder of its own: its apps are listed once.
    let cancel = AtomicBool::new(false);
    let (result, bundles, _) = scan(
        &[
            home.0.join("Applications"),
            home.0.join("Applications/Suite"),
        ],
        &[],
        Some(&home.0),
        &[],
        None,
        now_ms(),
        &cancel,
    );
    assert_eq!(names(&result), ["Editor", "Setup", "Top", "Web"]);
    let editor = result
        .apps
        .iter()
        .find(|app| app.name == "Editor")
        .expect("Editor");
    assert!(editor.location.ends_with("Applications/Suite"));
    assert!(!editor.apple);
    let folders = [home.0.join("Applications")];
    assert!(bundles
        .iter()
        .all(|bundle| bundle_in_bounds(&bundle.path, &folders)));
}

#[test]
fn apples_store_apps_can_be_removed_but_macos_apps_never_and_neither_offers_leftovers() {
    let home = Home::new("apple");
    home.app("Xcode", "com.apple.dt.Xcode");
    home.write("Applications/Xcode.app/Contents/_MASReceipt/receipt", 100);
    home.app("Old Apple Tool", "com.apple.OldTool");
    home.app_in("System Apps", "Notes", "com.apple.Notes");
    home.app_in("System Apps/Utilities", "Terminal", "com.apple.Terminal");
    home.app_in("System Apps", "Third", "com.example.Third");
    home.write("Library/Application Support/Xcode/data", 1_000);
    home.write("Library/Caches/Xcode/blob", 1_000);
    let cancel = AtomicBool::new(false);
    let (result, bundles, _) = scan(
        &[home.0.join("Applications")],
        &[home.0.join("System Apps")],
        Some(&home.0),
        &[],
        None,
        now_ms(),
        &cancel,
    );
    let apple = |name: &str| {
        result
            .apps
            .iter()
            .find(|app| app.name == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .apple
    };
    assert!(!apple("Xcode"), "from the App Store: removable");
    assert!(apple("Old Apple Tool"));
    assert!(apple("Notes") && apple("Terminal") && apple("Third"));
    assert!(result.apps.iter().all(|app| app.leftovers.is_empty()));
    let xcode = bundles
        .iter()
        .find(|bundle| bundle.name == "Xcode")
        .expect("Xcode");
    assert!(xcode.apple_made());
    assert!(leftovers_for_removed(xcode, &bundles, &[], &home.0, now_ms(), &cancel).is_empty());
    assert!(caches_of(xcode, &bundles, &[], &home.0, now_ms(), &cancel).is_empty());
}

#[test]
fn leftover_bounds_follow_the_owner_and_never_reach_apples_data() {
    let home = Path::new("/Users/someone");
    let leftover = |path: &str, owner: &str| {
        policy::within_bounds(
            &Kind::AppLeftover {
                owner: owner.to_owned(),
            },
            Path::new(path),
            home,
            None,
            |_| Vec::new(),
        )
    };
    assert!(leftover(
        "/Users/someone/Library/Application Support/com.x.App",
        "com.x.App"
    ));
    assert!(leftover(
        "/Users/someone/Library/Preferences/com.x.App.plist",
        "com.x.app"
    ));
    assert!(leftover(
        "/Users/someone/Library/Group Containers/TEAM123456.com.x.App",
        "com.x.App"
    ));
    // Another app, deeper inside, Apple's, or a folder that isn't a leftover folder.
    assert!(!leftover(
        "/Users/someone/Library/Application Support/com.x.Other",
        "com.x.App"
    ));
    assert!(!leftover(
        "/Users/someone/Library/Application Support/com.x.App/inner",
        "com.x.App"
    ));
    assert!(!leftover(
        "/Users/someone/Library/Preferences/com.apple.dock.plist",
        "com.apple.dock"
    ));
    assert!(!leftover(
        "/Users/someone/Library/Keychains/com.x.App",
        "com.x.App"
    ));
    assert!(!leftover("/Users/someone/Documents/com.x.App", "com.x.App"));
    // Restoring needs no owner, only a leftover folder.
    assert!(policy::restorable(
        "apps",
        Path::new("/Users/someone/Library/Logs/com.x.App"),
        &[],
        home,
        None
    ));
    assert!(!policy::restorable(
        "apps",
        Path::new("/Users/someone/Library/Keychains/x"),
        &[],
        home,
        None
    ));
}
