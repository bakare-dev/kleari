//! Kleari's bundle identifier changed from `dev.bakare.kleari` to `dev.kleari.app` (beta 4).
//! At the first launch under the new identifier, before any window opens, what Kleari kept
//! under the old one moves over: its Application Support folder (history, settings and the
//! quarantine folder, whose items are recorded relative to it) and WebKit's storage (the
//! appearance and list choices), each by one rename on the same disk. Kleari's other old
//! folders hold only what macOS rebuilds (caches, window state), so they are deleted.
//!
//! macOS ties permissions to the identifier, so the new one starts with none: the interface
//! shows a one-time sheet to allow them again (`pending`), and the old identifier's
//! permissions are reset so System Settings doesn't list a second, dead "Kleari". `tccutil`
//! finds an app by its identifier through LaunchServices, which no longer knows the old one
//! once the update replaced the bundle, so the reset goes through a stub (`stub_reset`).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::persistence::Database;

pub const OLD_IDENTIFIER: &str = "dev.bakare.kleari";
const TCCUTIL: &str = "/usr/bin/tccutil";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
/// How long each of those programs may take before it is stopped.
const TIME_LIMIT: Duration = Duration::from_secs(10);
/// The setting that holds a move whose sheet hasn't been closed yet.
const PENDING_KEY: &str = "identity_move.v1";
/// What the setting holds once the sheet was closed.
const DISMISSED: &str = "dismissed";

/// What became of the old identifier's data at this launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// There was nothing under the old identifier (a new install, or already moved).
    NothingToMove,
    /// Moved; whether the old identifier's permissions were reset.
    Moved { old_permissions_reset: bool },
    /// Both identifiers have a data folder (the old one made again by an older Kleari after
    /// the move). Neither is merged into the other; the old one is left as it is.
    BothExist,
    /// A Kleari under the old identifier is running: nothing moves while it uses the folder.
    OldRunning,
    /// The rename failed; this run uses the old folder where it is, and the next tries again.
    Failed,
}

/// The data folder this run uses, and what happened to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub outcome: Outcome,
    pub data_folder: PathBuf,
}

fn support(home: &Path, identifier: &str) -> PathBuf {
    home.join("Library/Application Support").join(identifier)
}

fn webkit(home: &Path, identifier: &str) -> PathBuf {
    home.join("Library/WebKit").join(identifier)
}

/// Kleari's old folders that hold only what macOS makes again on its own.
pub fn disposable(home: &Path, identifier: &str) -> [PathBuf; 4] {
    let library = home.join("Library");
    [
        library.join("Caches").join(identifier),
        library.join("HTTPStorages").join(identifier),
        library
            .join("Saved Application State")
            .join(format!("{identifier}.savedState")),
        library
            .join("Preferences")
            .join(format!("{identifier}.plist")),
    ]
}

fn exists(path: &Path) -> bool {
    // Anything that can't be looked up counts as there, so it is never replaced.
    !matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

/// Moves the old identifier's folders to `new_identifier`'s. Runs before Tauri starts, so
/// nothing of Kleari's has the new folders open yet. `old_running` says whether a Kleari with
/// the old identifier is running now; `reset` resets the old identifier's permissions.
pub fn migrate(
    home: &Path,
    new_identifier: &str,
    old_running: bool,
    reset: impl FnOnce() -> bool,
) -> Migration {
    let old = support(home, OLD_IDENTIFIER);
    let new = support(home, new_identifier);
    let at = |outcome, data_folder: &Path| Migration {
        outcome,
        data_folder: data_folder.to_path_buf(),
    };
    if !exists(&old) {
        return at(Outcome::NothingToMove, &new);
    }
    if exists(&new) {
        return at(Outcome::BothExist, &new);
    }
    if old_running {
        return at(Outcome::OldRunning, &old);
    }
    if std::fs::rename(&old, &new).is_err() {
        return at(Outcome::Failed, &old);
    }
    // The appearance and list choices; if this rename fails Kleari starts with the defaults.
    let (old_webkit, new_webkit) = (webkit(home, OLD_IDENTIFIER), webkit(home, new_identifier));
    if exists(&old_webkit) && !exists(&new_webkit) {
        let _ = std::fs::rename(&old_webkit, &new_webkit);
    }
    for path in disposable(home, OLD_IDENTIFIER) {
        remove(&path);
    }
    let old_permissions_reset = reset();
    at(
        Outcome::Moved {
            old_permissions_reset,
        },
        &new,
    )
}

/// Deletes one of Kleari's own disposable folders or files. A symbolic link is removed
/// itself, never followed. What can't be deleted stays; macOS no longer uses it.
fn remove(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(_) => true,
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path).is_ok(),
        Ok(_) => std::fs::remove_file(path).is_ok(),
    }
}

/// Whether a Kleari with the old identifier is running now.
#[cfg(target_os = "macos")]
pub fn old_running() -> bool {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    let identifier = NSString::from_str(OLD_IDENTIFIER);
    !NSRunningApplication::runningApplicationsWithBundleIdentifier(&identifier).is_empty()
}

#[cfg(not(target_os = "macos"))]
pub fn old_running() -> bool {
    false
}

/// Runs one of Apple's tools with fixed arguments, no shell and no environment, and says how
/// it exited; `None` if it couldn't start or took too long (it is stopped then).
fn run(command: &mut Command) -> Option<ExitStatus> {
    let mut child = command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if started.elapsed() < TIME_LIMIT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                // The kill result doesn't matter: the process is reaped either way.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// `tccutil reset All <identifier>`: macOS forgets every permission it gave that app. False
/// if macOS doesn't know an app with that identifier, or the tool failed.
pub fn reset_permissions(identifier: &str) -> bool {
    run(Command::new(TCCUTIL).args(["reset", "All", identifier]))
        .is_some_and(|status| status.success())
}

/// The stub's Info.plist: just enough for LaunchServices to register an app with
/// `identifier`.
fn stub_info_plist(identifier: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\
         <key>CFBundleExecutable</key><string>stub</string>\
         <key>CFBundleIdentifier</key><string>{identifier}</string>\
         <key>CFBundleName</key><string>Kleari (old)</string>\
         <key>CFBundlePackageType</key><string>APPL</string>\
         </dict></plist>\n"
    )
}

/// Writes the stub bundle at `bundle`: its Info.plist and an executable that does nothing
/// (LaunchServices doesn't register a bundle without one). It is never run.
fn write_stub(bundle: &Path, identifier: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let executable = bundle.join("Contents/MacOS/stub");
    std::fs::create_dir_all(bundle.join("Contents/MacOS"))?;
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        stub_info_plist(identifier),
    )?;
    std::fs::write(&executable, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
}

/// Resets the permissions of `identifier` (the old one, outside tests). When macOS no longer
/// knows it, a stub with it is registered in `caches` (Kleari's own caches folder;
/// LaunchServices ignores temporary folders) just long enough for `tccutil` to find it, then
/// unregistered and deleted.
pub fn stub_reset(caches: &Path, identifier: &str) -> bool {
    if reset_permissions(identifier) {
        return true;
    }
    let bundle = caches.join("KleariOldIdentity.app");
    // A stub a stopped run left behind is replaced.
    let _ = std::fs::remove_dir_all(&bundle);
    if write_stub(&bundle, identifier).is_err() {
        let _ = std::fs::remove_dir_all(&bundle);
        return false;
    }
    let registered =
        run(Command::new(LSREGISTER).arg("-f").arg(&bundle)).is_some_and(|status| status.success());
    let reset = registered && reset_permissions(identifier);
    let _ = run(Command::new(LSREGISTER).arg("-u").arg(&bundle));
    let _ = std::fs::remove_dir_all(&bundle);
    reset
}

/// What the re-grant sheet needs, while it hasn't been closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// False: System Settings may still list the old "Kleari", for the user to remove.
    pub old_permissions_reset: bool,
}

/// Records a move whose sheet should show. A failed write means no sheet; Kleari works the
/// same, and Settings still offers every permission.
pub fn record(db: &Database, outcome: Outcome) {
    if let Outcome::Moved {
        old_permissions_reset,
    } = outcome
    {
        let value = if old_permissions_reset {
            "reset"
        } else {
            "not-reset"
        };
        let _ = db.set_setting(PENDING_KEY, value);
    }
}

/// The move whose sheet is still to show, if any.
pub fn pending(db: &Database) -> Option<Pending> {
    let settings = db.settings().ok()?;
    let value = settings
        .iter()
        .find_map(|(key, value)| (key == PENDING_KEY).then_some(value.as_str()))?;
    match value {
        "reset" => Some(Pending {
            old_permissions_reset: true,
        }),
        "not-reset" => Some(Pending {
            old_permissions_reset: false,
        }),
        _ => None,
    }
}

/// The sheet was closed: it doesn't show again.
pub fn dismiss(db: &Database) -> Result<(), crate::persistence::Unavailable> {
    db.set_setting(PENDING_KEY, DISMISSED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Home(PathBuf);

    impl Home {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("kleari-identity-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("Library/Application Support")).expect("support");
            fs::create_dir_all(root.join("Library/WebKit")).expect("webkit");
            Self(root)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const NEW: &str = "dev.kleari.app";

    fn old_support(home: &Home) -> PathBuf {
        support(&home.0, OLD_IDENTIFIER)
    }

    #[test]
    fn a_new_install_moves_nothing_and_resets_nothing() {
        let home = Home::new("new");
        let migration = migrate(&home.0, NEW, false, || panic!("must not reset"));
        assert_eq!(migration.outcome, Outcome::NothingToMove);
        assert_eq!(migration.data_folder, support(&home.0, NEW));
    }

    #[test]
    fn the_old_folders_move_with_their_contents_and_caches_go() {
        let home = Home::new("moves");
        let old = old_support(&home);
        fs::create_dir_all(old.join("Quarantine/batch")).expect("old");
        fs::write(old.join("history.sqlite3"), "db").expect("db");
        fs::write(old.join("Quarantine/batch/item"), "held").expect("item");
        fs::create_dir_all(webkit(&home.0, OLD_IDENTIFIER).join("WebsiteData")).expect("webkit");
        for path in disposable(&home.0, OLD_IDENTIFIER) {
            fs::create_dir_all(path.parent().expect("parent")).expect("library");
            fs::write(&path, "x").expect("disposable");
        }
        let migration = migrate(&home.0, NEW, false, || true);
        assert_eq!(
            migration.outcome,
            Outcome::Moved {
                old_permissions_reset: true
            }
        );
        let new = support(&home.0, NEW);
        assert_eq!(migration.data_folder, new);
        assert_eq!(
            fs::read_to_string(new.join("history.sqlite3")).expect("db"),
            "db"
        );
        assert_eq!(
            fs::read_to_string(new.join("Quarantine/batch/item")).expect("item"),
            "held"
        );
        assert!(!old.exists());
        assert!(webkit(&home.0, NEW).join("WebsiteData").is_dir());
        assert!(!webkit(&home.0, OLD_IDENTIFIER).exists());
        assert!(disposable(&home.0, OLD_IDENTIFIER)
            .iter()
            .all(|path| !path.exists()));
    }

    #[test]
    fn a_failed_reset_is_reported() {
        let home = Home::new("noreset");
        fs::create_dir_all(old_support(&home)).expect("old");
        let migration = migrate(&home.0, NEW, false, || false);
        assert_eq!(
            migration.outcome,
            Outcome::Moved {
                old_permissions_reset: false
            }
        );
    }

    #[test]
    fn nothing_is_merged_when_both_exist() {
        let home = Home::new("both");
        fs::create_dir_all(old_support(&home)).expect("old");
        fs::write(old_support(&home).join("history.sqlite3"), "old").expect("old db");
        fs::create_dir_all(support(&home.0, NEW)).expect("new");
        let migration = migrate(&home.0, NEW, false, || panic!("must not reset"));
        assert_eq!(migration.outcome, Outcome::BothExist);
        assert_eq!(migration.data_folder, support(&home.0, NEW));
        assert_eq!(
            fs::read_to_string(old_support(&home).join("history.sqlite3")).expect("kept"),
            "old"
        );
        assert!(!support(&home.0, NEW).join("history.sqlite3").exists());
    }

    #[test]
    fn nothing_moves_while_the_old_kleari_runs() {
        let home = Home::new("running");
        fs::create_dir_all(old_support(&home)).expect("old");
        let migration = migrate(&home.0, NEW, true, || panic!("must not reset"));
        assert_eq!(migration.outcome, Outcome::OldRunning);
        assert_eq!(migration.data_folder, old_support(&home));
        assert!(old_support(&home).exists());
        assert!(!support(&home.0, NEW).exists());
    }

    #[test]
    fn an_old_webkit_folder_never_replaces_a_new_one() {
        let home = Home::new("webkit");
        fs::create_dir_all(old_support(&home)).expect("old");
        fs::create_dir_all(webkit(&home.0, OLD_IDENTIFIER)).expect("old webkit");
        fs::create_dir_all(webkit(&home.0, NEW).join("keep")).expect("new webkit");
        let _ = migrate(&home.0, NEW, false, || true);
        assert!(webkit(&home.0, NEW).join("keep").is_dir());
        assert!(webkit(&home.0, OLD_IDENTIFIER).is_dir());
    }

    #[test]
    fn a_symlinked_cache_is_removed_not_followed() {
        let home = Home::new("symlink");
        fs::create_dir_all(old_support(&home)).expect("old");
        let target = home.0.join("elsewhere");
        fs::create_dir_all(&target).expect("target");
        fs::write(target.join("keep"), "mine").expect("file");
        let cache = &disposable(&home.0, OLD_IDENTIFIER)[0];
        fs::create_dir_all(cache.parent().expect("parent")).expect("caches");
        std::os::unix::fs::symlink(&target, cache).expect("link");
        let _ = migrate(&home.0, NEW, false, || true);
        assert!(fs::symlink_metadata(cache).is_err());
        assert_eq!(
            fs::read_to_string(target.join("keep")).expect("kept"),
            "mine"
        );
    }

    #[test]
    fn the_sheet_shows_once_after_a_move() {
        let db = Database::in_memory();
        assert_eq!(pending(&db), None);
        record(&db, Outcome::NothingToMove);
        record(&db, Outcome::BothExist);
        assert_eq!(pending(&db), None);
        record(
            &db,
            Outcome::Moved {
                old_permissions_reset: false,
            },
        );
        assert_eq!(
            pending(&db),
            Some(Pending {
                old_permissions_reset: false
            })
        );
        dismiss(&db).expect("dismiss");
        assert_eq!(pending(&db), None);
    }

    #[test]
    fn the_stub_names_only_the_identifier_given() {
        let plist = stub_info_plist(OLD_IDENTIFIER);
        assert!(plist.contains("<string>dev.bakare.kleari</string>"));
        assert!(plist.contains("<key>CFBundlePackageType</key><string>APPL</string>"));
        let home = Home::new("stub");
        let bundle = home.0.join("Stub.app");
        write_stub(&bundle, OLD_IDENTIFIER).expect("stub");
        assert!(bundle.join("Contents/Info.plist").is_file());
        assert!(bundle.join("Contents/MacOS/stub").is_file());
    }

    /// On a real Mac (`cargo test -- --ignored stub_reset`): an identifier macOS doesn't know
    /// can't be reset, and can through the stub, which is gone afterwards. Uses a made-up
    /// identifier, so no real app's permissions change. Not in temp: LaunchServices ignores it.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "runs lsregister and tccutil on this Mac"]
    fn stub_reset_resolves_an_identifier_macos_doesnt_know() {
        let identifier = "com.example.kleari-stub-probe";
        let caches = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/identity-stub");
        fs::create_dir_all(&caches).expect("caches");
        assert!(!reset_permissions(identifier));
        assert!(stub_reset(&caches, identifier));
        assert!(!caches.join("KleariOldIdentity.app").exists());
        assert!(!reset_permissions(identifier));
        let _ = fs::remove_dir_all(&caches);
    }
}
