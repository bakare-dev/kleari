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
        self.write(&format!("Applications/{name}.app/Contents/Info.plist"), 0);
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
                .join(format!("Applications/{name}.app/Contents/Info.plist")),
            plist,
        )
        .expect("plist");
        self.write(
            &format!("Applications/{name}.app/Contents/MacOS/{name}"),
            40_000,
        );
    }
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
    let cancel = AtomicBool::new(false);
    scan(
        &[home.0.join("Applications")],
        Some(&home.0),
        running,
        now_ms(),
        &cancel,
    )
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
    let items = leftovers_for_removed(bundle, &home.0, now_ms(), &cancel);
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
fn only_app_bundles_directly_in_an_applications_folder_are_in_bounds() {
    let folders = app_folders(Some(Path::new("/Users/someone")));
    assert!(bundle_in_bounds(
        Path::new("/Applications/Fake.app"),
        &folders
    ));
    assert!(bundle_in_bounds(
        Path::new("/Users/someone/Applications/Fake.app"),
        &folders
    ));
    assert!(!bundle_in_bounds(Path::new("/Applications/Fake"), &folders));
    assert!(!bundle_in_bounds(
        Path::new("/Applications/Suite/Fake.app"),
        &folders
    ));
    assert!(!bundle_in_bounds(
        Path::new("/System/Applications/Notes.app"),
        &folders
    ));
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
