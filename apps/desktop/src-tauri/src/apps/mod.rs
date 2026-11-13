//! The Apps page: installed apps with what each keeps in the user's Library, leftovers of
//! apps that are gone, and installers already downloaded (ADR-017).
//!
//! Read-only. Every folder is opened relative to its open parent without following
//! symlinks (`fsdir`), and an app's `Info.plist` is the only file content read (at most
//! 256 KB, parsed as untrusted input). Removing an app moves its bundle to the Trash through
//! macOS (`remove_app`); its leftovers, other apps' leftovers and installers go through
//! Cleanup's review, preview and quarantine like everything else Kleari moves.

use std::ffi::{CStr, CString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::cleanup::policy::{is_installer_name, leftover_matches, Kind, LEFTOVER_FOLDERS};
use crate::developer::{measure_dir, open_absolute, Links, Measured};
use crate::ids::new_id;
use crate::storage::fsdir::{DirHandle, FileStat};
use crate::system_monitor::display_path;
use crate::system_monitor::running_apps::RunningBundle;

const MAX_APPS: usize = 2_000;
const MAX_ENTRIES_PER_FOLDER: usize = 20_000;
const MAX_PLIST_BYTES: u64 = 256 * 1024;
/// A leftover changed this recently is probably still used by something (a helper, a
/// command-line tool), so it isn't offered as a leftover of a removed app.
const ORPHAN_IDLE_DAYS: u64 = 30;
const DAY_S: i64 = 86_400;
/// Leftover folders searched for leftovers of apps that are gone. Caches are Cleanup's, and
/// the containers belong to sandboxed apps macOS manages: those are only offered for an app
/// you remove here.
const ORPHAN_FOLDERS: &[&str] = &[
    "Library/Application Support",
    "Library/Preferences",
    "Library/Preferences/ByHost",
    "Library/Saved Application State",
    "Library/HTTPStorages",
    "Library/WebKit",
    "Library/Logs",
    "Library/Cookies",
    "Library/Application Scripts",
    "Library/LaunchAgents",
];
/// Top-level domains that start a bundle ID (`com.example.App`).
const ID_PREFIXES: &[&str] = &[
    "com", "org", "net", "io", "dev", "app", "co", "me", "de", "uk", "fr", "jp", "ru", "ch", "nl",
    "se", "us", "ca", "info", "biz", "tv", "ai", "so", "sh", "cc", "is", "it", "pl",
];

/// An installed app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    /// Opaque; the backend keeps the bundle's path and identity.
    pub id: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    /// The folder it is in, shown as `~/…` or `/Applications`.
    pub location: String,
    pub bytes: u64,
    pub lower_bound: bool,
    pub running: bool,
    /// Part of macOS: Kleari never removes it.
    pub apple: bool,
    /// What it keeps in the Library, removed with it on request.
    pub leftovers: Vec<LeftoverEntry>,
    pub leftover_bytes: u64,
}

/// An entry in a Library folder belonging to an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverEntry {
    /// For a leftover of an app that is gone: the ID to send it to the review with.
    pub review_id: Option<String>,
    pub name: String,
    /// The Library folder it is in, shown as `~/Library/…`.
    pub location: String,
    /// The app it belongs to: its name, or its bundle ID when the app is gone.
    pub owner: String,
    pub bytes: u64,
    pub lower_bound: bool,
    pub modified_ms: Option<u64>,
}

/// An installer in Downloads or on the Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerEntry {
    pub review_id: String,
    pub name: String,
    pub location: String,
    pub bytes: u64,
    pub modified_ms: Option<u64>,
    /// An installed app's name starts its file name: probably already installed.
    pub installed_app: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsScan {
    pub status: ScanStatus,
    pub apps: Vec<AppEntry>,
    pub orphans: Vec<LeftoverEntry>,
    pub installers: Vec<InstallerEntry>,
}

/// An app bundle the last scan found, kept in Rust; the page has only its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub id: String,
    pub path: PathBuf,
    pub stat: FileStat,
    pub name: String,
    pub bundle_id: Option<String>,
    pub apple: bool,
}

/// Something the page may send to the review: what it is and where, kept in Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewItem {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub path: PathBuf,
    pub stat: FileStat,
    pub measured: Measured,
    /// Its app was removed with Kleari just now.
    pub owner_removed: bool,
}

/// One Library folder's entries, read once and matched against every app.
struct FolderEntries {
    folder: PathBuf,
    entries: Vec<(String, FileStat)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The scan in progress (one at a time), and what the last one found.
#[derive(Default)]
pub struct AppsState {
    active: Mutex<Option<Arc<AtomicBool>>>,
    bundles: Mutex<Vec<Bundle>>,
    review: Mutex<Vec<ReviewItem>>,
}

/// A scan is already running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBusy;

impl AppsState {
    pub fn begin(&self) -> Result<Arc<AtomicBool>, ScanBusy> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(ScanBusy);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        Ok(cancel)
    }

    /// Ends the scan and keeps what it found (only a completed scan replaces the last one).
    pub fn finish(&self, found: Option<(Vec<Bundle>, Vec<ReviewItem>)>) {
        if let Some((bundles, review)) = found {
            *lock(&self.bundles) = bundles;
            *lock(&self.review) = review;
        }
        *lock(&self.active) = None;
    }

    pub fn cancel(&self) {
        if let Some(cancel) = &*lock(&self.active) {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn bundle(&self, id: &str) -> Option<Bundle> {
        lock(&self.bundles)
            .iter()
            .find(|bundle| bundle.id == id)
            .cloned()
    }

    pub fn review_items(&self, ids: &[String]) -> Vec<ReviewItem> {
        let review = lock(&self.review);
        ids.iter()
            .filter_map(|id| review.iter().find(|item| &item.id == id).cloned())
            .collect()
    }

    /// Forgets a removed app.
    pub fn forget(&self, id: &str) {
        lock(&self.bundles).retain(|bundle| bundle.id != id);
    }
}

/// Where apps are installed: `/Applications`, its `Utilities` folder and `~/Applications`.
pub fn app_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
    ];
    if let Some(home) = home {
        folders.push(home.join("Applications"));
    }
    folders
}

/// Whether `path` is an app bundle directly in one of `folders`.
pub fn bundle_in_bounds(path: &Path, folders: &[PathBuf]) -> bool {
    path.extension().is_some_and(|extension| extension == "app")
        && path
            .parent()
            .is_some_and(|parent| folders.iter().any(|folder| folder == parent))
}

/// Runs the read-only scan of the apps in `folders` (`app_folders`) and the home folder.
pub fn scan(
    folders: &[PathBuf],
    home: Option<&Path>,
    running: &[RunningBundle],
    now_ms: u64,
    cancel: &AtomicBool,
) -> (AppsScan, Vec<Bundle>, Vec<ReviewItem>) {
    let mut links = Links::default();
    let mut bundles = Vec::new();
    let mut apps = Vec::new();
    for folder in folders {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        read_apps(
            folder,
            home,
            running,
            now_ms,
            cancel,
            &mut links,
            &mut bundles,
            &mut apps,
        );
    }
    let library = home
        .map(|home| read_library(home, LEFTOVER_FOLDERS))
        .unwrap_or_default();
    let mut review = Vec::new();
    for app in &mut apps {
        let Some(bundle) = bundles.iter().find(|bundle| bundle.id == app.id) else {
            continue;
        };
        if bundle.apple {
            continue;
        }
        for (path, stat, owner) in leftovers_of(bundle, &library) {
            let measured = measure(&path, &stat, &mut links, cancel);
            app.leftover_bytes = app.leftover_bytes.saturating_add(measured.bytes);
            app.leftovers
                .push(leftover_entry(None, &path, home, owner, &measured));
        }
    }
    let installed: Vec<&Bundle> = bundles.iter().collect();
    let mut orphans = Vec::new();
    if let Some(home) = home {
        let orphan_folders: Vec<&FolderEntries> = library
            .iter()
            .filter(|folder| {
                ORPHAN_FOLDERS
                    .iter()
                    .any(|name| folder.folder == home.join(name))
            })
            .collect();
        for folder in orphan_folders {
            for (name, stat) in &folder.entries {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Some(owner) = orphan_owner(name, &installed, running) else {
                    continue;
                };
                let path = folder.folder.join(name);
                let measured = measure(&path, stat, &mut links, cancel);
                let newest = measured.newest_modified_s.max(stat.modified_s);
                if idle_days(newest, now_ms) < ORPHAN_IDLE_DAYS {
                    continue;
                }
                let id = new_id(now_ms);
                orphans.push(leftover_entry(
                    Some(id.clone()),
                    &path,
                    Some(home),
                    owner.clone(),
                    &measured,
                ));
                review.push(ReviewItem {
                    id,
                    kind: Kind::AppLeftover { owner },
                    name: name.clone(),
                    path,
                    stat: *stat,
                    measured,
                    owner_removed: false,
                });
            }
        }
    }
    let installers = home
        .map(|home| find_installers(home, &bundles, now_ms, &mut review))
        .unwrap_or_default();
    apps.sort_by(|a, b| {
        b.bytes
            .saturating_add(b.leftover_bytes)
            .cmp(&a.bytes.saturating_add(a.leftover_bytes))
    });
    orphans.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    let status = if cancel.load(Ordering::Relaxed) {
        ScanStatus::Cancelled
    } else {
        ScanStatus::Completed
    };
    (
        AppsScan {
            status,
            apps,
            orphans,
            installers,
        },
        bundles,
        review,
    )
}

/// The leftovers of the app `bundle`, read again now, ready for the review: used right after
/// the app is moved to the Trash.
pub fn leftovers_for_removed(
    bundle: &Bundle,
    home: &Path,
    now_ms: u64,
    cancel: &AtomicBool,
) -> Vec<ReviewItem> {
    let library = read_library(home, LEFTOVER_FOLDERS);
    let mut links = Links::default();
    leftovers_of(bundle, &library)
        .into_iter()
        .map(|(path, stat, owner)| {
            let measured = measure(&path, &stat, &mut links, cancel);
            ReviewItem {
                id: new_id(now_ms),
                name: display_name(&path),
                kind: Kind::AppLeftover { owner },
                path,
                stat,
                measured,
                owner_removed: true,
            }
        })
        .collect()
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn idle_days(modified_s: i64, now_ms: u64) -> u64 {
    let now_s = i64::try_from(now_ms / 1000).unwrap_or(i64::MAX);
    u64::try_from(now_s.saturating_sub(modified_s) / DAY_S).unwrap_or(0)
}

fn modified_ms(seconds: i64) -> Option<u64> {
    u64::try_from(seconds)
        .ok()
        .filter(|&seconds| seconds > 0)
        .and_then(|seconds| seconds.checked_mul(1000))
}

fn leftover_entry(
    review_id: Option<String>,
    path: &Path,
    home: Option<&Path>,
    owner: String,
    measured: &Measured,
) -> LeftoverEntry {
    LeftoverEntry {
        review_id,
        name: display_name(path),
        location: path
            .parent()
            .map(|parent| display_path(parent, home))
            .unwrap_or_default(),
        owner,
        bytes: measured.bytes,
        lower_bound: measured.lower_bound,
        modified_ms: modified_ms(measured.newest_modified_s),
    }
}

/// The size of a file or folder, through handles.
fn measure(path: &Path, stat: &FileStat, links: &mut Links, cancel: &AtomicBool) -> Measured {
    let file = || Measured {
        bytes: stat.blocks.saturating_mul(512),
        files: 1,
        lower_bound: false,
        parts: Vec::new(),
        newest_modified_s: stat.modified_s,
    };
    if !stat.is_dir() {
        return file();
    }
    let opened = path
        .parent()
        .and_then(open_absolute)
        .and_then(|(parent, _)| {
            let name =
                CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.file_name()?)).ok()?;
            parent.open_child(&name).ok()
        });
    match opened {
        Some((dir, opened)) if opened.dev == stat.dev && opened.ino == stat.ino => {
            measure_dir(dir, stat.dev, links, cancel)
        }
        _ => Measured {
            lower_bound: true,
            ..file()
        },
    }
}

/// Each leftover folder's entries (not following symlinks).
fn read_library(home: &Path, folders: &[&str]) -> Vec<FolderEntries> {
    folders
        .iter()
        .filter_map(|folder| {
            let folder = home.join(folder);
            let (mut dir, _) = open_absolute(&folder)?;
            let mut entries = Vec::new();
            while let Some(Ok(name)) = dir.next_name() {
                if entries.len() >= MAX_ENTRIES_PER_FOLDER {
                    break;
                }
                let Ok(text) = name.to_str() else {
                    continue;
                };
                let Ok(stat) = dir.stat_child(&name) else {
                    continue;
                };
                if stat.is_symlink() || !(stat.is_dir() || stat.is_file()) {
                    continue;
                }
                entries.push((text.to_owned(), stat));
            }
            Some(FolderEntries { folder, entries })
        })
        .collect()
}

/// The entries of `library` belonging to `bundle`, matched by bundle ID or by its name.
fn leftovers_of(bundle: &Bundle, library: &[FolderEntries]) -> Vec<(PathBuf, FileStat, String)> {
    let owners: Vec<&str> = bundle
        .bundle_id
        .as_deref()
        .into_iter()
        .chain(std::iter::once(bundle.name.as_str()))
        .collect();
    let mut found = Vec::new();
    for folder in library {
        for (name, stat) in &folder.entries {
            if let Some(owner) = owners.iter().find(|owner| leftover_matches(name, owner)) {
                found.push((folder.folder.join(name), *stat, (*owner).to_owned()));
            }
        }
    }
    found
}

/// The bundle ID a Library entry is named for, when it looks like one, isn't Apple's, and no
/// installed or running app (or another app of the same vendor) could own it.
fn orphan_owner(name: &str, installed: &[&Bundle], running: &[RunningBundle]) -> Option<String> {
    let lower = name.to_lowercase();
    let stem = [".plist", ".savedstate", ".binarycookies"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix))
        .unwrap_or(&lower);
    let mut parts: Vec<&str> = stem.split('.').collect();
    // `ByHost/<id>.<host UUID>.plist`: the host part isn't the app's.
    if parts
        .last()
        .is_some_and(|last| last.len() == 36 && last.matches('-').count() == 4)
    {
        parts.pop();
    }
    if parts.len() < 3
        || !ID_PREFIXES.contains(&parts[0])
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    {
        return None;
    }
    let owner = parts.join(".");
    if owner.starts_with("com.apple.") {
        return None;
    }
    let vendor = format!("{}.{}.", parts[0], parts[1]);
    let claimed = |id: &str| {
        let id = id.to_lowercase();
        leftover_matches(name, &id)
            || owner.starts_with(&format!("{id}."))
            || id == owner
            || id.starts_with(&vendor)
    };
    if installed
        .iter()
        .filter_map(|bundle| bundle.bundle_id.as_deref())
        .any(claimed)
        || running.iter().any(|app| claimed(&app.id))
    {
        return None;
    }
    // A name that matches the leftover rules for the owner it gives, so its bounds hold.
    leftover_matches(name, &owner).then_some(owner)
}

#[allow(clippy::too_many_arguments)]
fn read_apps(
    folder: &Path,
    home: Option<&Path>,
    running: &[RunningBundle],
    now_ms: u64,
    cancel: &AtomicBool,
    links: &mut Links,
    bundles: &mut Vec<Bundle>,
    apps: &mut Vec<AppEntry>,
) {
    let Some((mut dir, dev)) = open_absolute(folder) else {
        return;
    };
    while let Some(Ok(name)) = dir.next_name() {
        if bundles.len() >= MAX_APPS || cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(stem) = name
            .to_str()
            .ok()
            .and_then(|name| name.strip_suffix(".app"))
        else {
            continue;
        };
        // A symlinked app (Safari links into the system) isn't followed or offered.
        let Ok(stat) = dir.stat_child(&name) else {
            continue;
        };
        if !stat.is_dir() || stat.dev != dev {
            continue;
        }
        let path = folder.join(name.to_str().unwrap_or_default());
        let info = read_info(&dir, &name, dev).unwrap_or_default();
        let display = info.name.clone().unwrap_or_else(|| stem.to_owned());
        let apple = info
            .bundle_id
            .as_deref()
            .is_some_and(|id| id.to_lowercase().starts_with("com.apple."));
        let measured = match dir.open_child(&name) {
            Ok((bundle, opened)) if opened.ino == stat.ino => {
                measure_dir(bundle, dev, links, cancel)
            }
            _ => Measured {
                bytes: 0,
                files: 0,
                lower_bound: true,
                parts: Vec::new(),
                newest_modified_s: 0,
            },
        };
        let id = new_id(now_ms);
        let is_running = info.bundle_id.as_deref().is_some_and(|bundle_id| {
            running
                .iter()
                .any(|app| app.id.eq_ignore_ascii_case(bundle_id))
        });
        apps.push(AppEntry {
            id: id.clone(),
            name: display.clone(),
            bundle_id: info.bundle_id.clone(),
            version: info.version,
            location: display_path(folder, home),
            bytes: measured.bytes,
            lower_bound: measured.lower_bound,
            running: is_running,
            apple,
            leftovers: Vec::new(),
            leftover_bytes: 0,
        });
        bundles.push(Bundle {
            id,
            path,
            stat,
            name: display,
            bundle_id: info.bundle_id,
            apple,
        });
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Info {
    bundle_id: Option<String>,
    name: Option<String>,
    version: Option<String>,
}

fn read_info(applications: &DirHandle, bundle: &CStr, dev: u64) -> Option<Info> {
    use std::os::unix::fs::MetadataExt;

    let (app, opened) = applications.open_child(bundle).ok()?;
    if opened.dev != dev {
        return None;
    }
    let (contents, opened) = app.open_child(c"Contents").ok()?;
    if opened.dev != dev {
        return None;
    }
    let file = contents.open_file_child(c"Info.plist").ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.dev() != dev || metadata.len() > MAX_PLIST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PLIST_BYTES).read_to_end(&mut bytes).ok()?;
    Some(parse_info(&bytes))
}

/// The bundle ID, display name and version from an `Info.plist` (XML or binary).
fn parse_info(bytes: &[u8]) -> Info {
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) else {
        return Info::default();
    };
    let Some(dictionary) = value.as_dictionary() else {
        return Info::default();
    };
    let text = |key: &str| {
        dictionary
            .get(key)?
            .as_string()
            .filter(|text| !text.is_empty() && text.len() <= 255 && !text.as_bytes().contains(&0))
            .map(str::to_owned)
    };
    Info {
        bundle_id: text("CFBundleIdentifier"),
        name: text("CFBundleDisplayName").or_else(|| text("CFBundleName")),
        version: text("CFBundleShortVersionString"),
    }
}

/// Installers directly in Downloads and on the Desktop.
fn find_installers(
    home: &Path,
    bundles: &[Bundle],
    now_ms: u64,
    review: &mut Vec<ReviewItem>,
) -> Vec<InstallerEntry> {
    let normalized = |text: &str| -> String {
        text.chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_lowercase()
    };
    let mut found = Vec::new();
    for folder in ["Downloads", "Desktop"] {
        let folder = home.join(folder);
        let Some((mut dir, _)) = open_absolute(&folder) else {
            continue;
        };
        while let Some(Ok(name)) = dir.next_name() {
            if found.len() >= MAX_ENTRIES_PER_FOLDER {
                break;
            }
            let Ok(text) = name.to_str() else {
                continue;
            };
            if !is_installer_name(text) {
                continue;
            }
            let Ok(stat) = dir.stat_child(&name) else {
                continue;
            };
            // A `.dmg` or `.pkg` is a file; a flat package folder isn't offered.
            if !stat.is_file() {
                continue;
            }
            let stem = normalized(
                Path::new(text)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default(),
            );
            let installed_app = bundles
                .iter()
                .filter(|bundle| !bundle.apple)
                .find(|bundle| {
                    let app = normalized(&bundle.name);
                    app.len() >= 3 && stem.starts_with(&app)
                })
                .map(|bundle| bundle.name.clone());
            let id = new_id(now_ms);
            let path = folder.join(text);
            let measured = Measured {
                bytes: stat.blocks.saturating_mul(512),
                files: 1,
                lower_bound: false,
                parts: Vec::new(),
                newest_modified_s: stat.modified_s,
            };
            found.push(InstallerEntry {
                review_id: id.clone(),
                name: text.to_owned(),
                location: display_path(&folder, Some(home)),
                bytes: measured.bytes,
                modified_ms: modified_ms(stat.modified_s),
                installed_app,
            });
            review.push(ReviewItem {
                id,
                kind: Kind::Installer,
                name: text.to_owned(),
                path,
                stat,
                measured,
                owner_removed: false,
            });
        }
    }
    found.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
    found
}

#[cfg(test)]
mod tests;
