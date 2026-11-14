//! The Developer page: developer caches, project build folders and Docker's disk use, all
//! read-only. Caches come from a fixed catalog; projects are found by marker files in the
//! home folder (without Library or the Trash) and in folders the user adds through the macOS
//! folder picker; Docker answers one read-only question over its local socket. Nothing is
//! stored; the result goes to the window that asked.

// Cleanup reuses the catalog, project search, Docker read and safe measuring.
pub(crate) mod catalog;
pub(crate) mod docker;
mod measure;
pub(crate) mod projects;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;

pub use docker::DockerState;
pub use measure::Measured;
pub(crate) use measure::{measure_dir, open_absolute, open_beneath, Links};
pub use projects::Stack;

use crate::ids::new_id;
use crate::storage::fsdir::{DirHandle, FileStat};
use crate::system_monitor::open_paths::{self, OpenPaths};
use crate::system_monitor::{display_path, non_local_mounts};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheResult {
    /// The catalog ID; the frontend has its name and explanation.
    pub id: &'static str,
    pub locations: Vec<String>,
    pub version: Option<String>,
    pub measured: Measured,
    /// Sends it to Cleanup's review (`send_developer_items_to_review`); `None` when it can't
    /// be identified (several locations, or it changed while measured).
    pub review_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildFolder {
    pub name: String,
    pub measured: Measured,
    pub review_id: Option<String>,
}

/// What a review ID stands for. Kept in Rust; the frontend only has the ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewKind {
    Cache(&'static str),
    Build { name: String, project: PathBuf },
}

/// A cache or build folder the last scan measured, which the page may send to review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewItem {
    pub id: String,
    pub kind: ReviewKind,
    pub path: PathBuf,
    pub stat: FileStat,
    pub measured: Measured,
}

/// `lstat` of the absolute `path`, opened from `/` without following symlinks.
fn identify(path: &Path) -> Option<FileStat> {
    let parent = path.parent()?;
    let name =
        std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.file_name()?)).ok()?;
    let (dir, _) = open_absolute(parent)?;
    dir.stat_child(&name).ok().filter(|stat| stat.is_dir())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
    pub name: String,
    pub location: String,
    pub stack: Stack,
    pub last_commit_ms: Option<u64>,
    /// A file in it, or the folder itself, is open in some app (an editor, a terminal, a
    /// dev server) as the scan read it; nothing in it is suggested then. Checked again
    /// before anything moves.
    pub in_use: bool,
    pub bytes: u64,
    pub folders: Vec<BuildFolder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperScan {
    pub status: ScanStatus,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    /// Caches that exist, largest first.
    pub caches: Vec<CacheResult>,
    /// Projects with build folders, largest first.
    pub projects: Vec<ProjectResult>,
    pub projects_truncated: bool,
    /// Folders the project search couldn't list; projects inside them are missing.
    pub unreadable_folders: u64,
    /// Where projects were searched, shown as `~/…`.
    pub roots: Vec<String>,
    /// Of those, the ones that couldn't be searched (on a non-local volume, the mount table
    /// unreadable, or the folder couldn't be opened).
    pub skipped_roots: Vec<String>,
    pub docker: DockerState,
    /// What each review ID stands for; kept in `DeveloperState`, never sent.
    #[serde(skip)]
    pub review_items: Vec<ReviewItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Caches,
    Projects,
    Docker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: Phase,
    /// Caches measured, or projects found so far.
    pub done: u32,
}

fn add(total: &mut Measured, part: Measured) {
    total.bytes = total.bytes.saturating_add(part.bytes);
    total.files = total.files.saturating_add(part.files);
    total.lower_bound |= part.lower_bound;
    total.parts.extend(part.parts);
}

/// Caches smaller than this (an empty folder's metadata) aren't listed.
const MIN_CACHE_BYTES: u64 = 1_000_000;

fn under(path: &Path, mounts: &[PathBuf]) -> bool {
    mounts.iter().any(|mount| path.starts_with(mount))
}

/// Opens a catalog location from the home folder (`~/…`) or `/`, one component at a time
/// relative to each open parent, without following symlinks anywhere on the path (the home
/// folder's own ancestors included) and, below the base, only on the base's volume.
fn open_cache(path: &str, home: Option<&Path>) -> Option<(DirHandle, u64)> {
    let (base, relative) = match path.strip_prefix("~/") {
        Some(rest) => (home?, rest),
        None => (Path::new("/"), path.strip_prefix('/')?),
    };
    let (dir, dev) = open_absolute(base)?;
    open_beneath(&dir, dev, Path::new(relative)).map(|cache| (cache, dev))
}

/// Measures every cache in the catalog that exists. The mount table is read before each
/// location; one under a non-local mount is skipped, as is everything when the table can't
/// be read.
fn caches(
    home: Option<&Path>,
    excluded: &[PathBuf],
    links: &mut Links,
    cancel: &AtomicBool,
    now_ms: u64,
    review: &mut Vec<ReviewItem>,
    progress: &mut dyn FnMut(ScanProgress),
) -> Vec<CacheResult> {
    let mut found = Vec::new();
    for (index, cache) in catalog::CACHES.iter().enumerate() {
        let mut total: Option<Measured> = None;
        let mut locations = Vec::new();
        // A cache in one location can be sent to review; its identity is taken first.
        let mut single: Option<(PathBuf, FileStat)> = None;
        for &location in cache.paths {
            let Some(path) = catalog::resolve(location, home) else {
                continue;
            };
            // In, or containing, a folder excluded in Settings: never opened or measured.
            if crate::preferences::touches_excluded(&path, excluded) {
                continue;
            }
            let Some(mounts) = non_local_mounts() else {
                return found;
            };
            if under(&path, &mounts) {
                continue;
            }
            let stat = identify(&path);
            if let Some((dir, dev)) = open_cache(location, home) {
                let measured = measure_dir(dir, dev, links, cancel);
                locations.push(display_path(&path, home));
                if cache.paths.len() == 1 {
                    single = stat.map(|stat| (path.clone(), stat));
                }
                match &mut total {
                    Some(total) => add(total, measured),
                    None => total = Some(measured),
                }
            }
        }
        if let Some(measured) = total.filter(|measured| measured.bytes >= MIN_CACHE_BYTES) {
            let review_id = single.map(|(path, stat)| {
                let id = new_id(now_ms);
                review.push(ReviewItem {
                    id: id.clone(),
                    kind: ReviewKind::Cache(cache.id),
                    path,
                    stat,
                    measured: measured.clone(),
                });
                id
            });
            found.push(CacheResult {
                id: cache.id,
                locations,
                version: cache.version.and_then(catalog::read_version),
                measured,
                review_id,
            });
        }
        progress(ScanProgress {
            phase: Phase::Caches,
            done: u32::try_from(index + 1).unwrap_or(u32::MAX),
        });
    }
    found.sort_by(|a, b| b.measured.bytes.cmp(&a.measured.bytes).then(a.id.cmp(b.id)));
    found
}

/// Where projects are searched: the home folder plus added folders that aren't inside it.
pub fn search_roots(home: Option<&Path>, added: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = home.map(Path::to_path_buf).into_iter().collect();
    for folder in added {
        if !roots.iter().any(|root| folder.starts_with(root)) {
            roots.push(folder.clone());
        }
    }
    roots
}

/// Runs the scan read-only: caches, then projects, then Docker. `started_at_ms` is recorded
/// in the result. `open` is what is open now (`None`: couldn't be read, so every project
/// counts as in use).
pub fn scan(
    home: Option<&Path>,
    added: &[PathBuf],
    excluded: &[PathBuf],
    open: Option<&OpenPaths>,
    cancel: &AtomicBool,
    started_at_ms: u64,
    mut progress: impl FnMut(ScanProgress),
) -> DeveloperScan {
    let started = std::time::Instant::now();
    // One hard-link set for the whole scan: pnpm links store packages into each project's
    // node_modules, so the store and the projects aren't counted twice.
    let mut links = Links::default();
    let mut review = Vec::new();
    let caches = caches(
        home,
        excluded,
        &mut links,
        cancel,
        started_at_ms,
        &mut review,
        &mut progress,
    );

    let roots: Vec<PathBuf> = search_roots(home, added)
        .into_iter()
        .filter(|root| !excluded.iter().any(|folder| root.starts_with(folder)))
        .collect();
    // The folders the user excluded in Settings are never searched.
    let skip: Vec<PathBuf> = home
        .map(|home| vec![home.join("Library"), home.join(".Trash")])
        .unwrap_or_default()
        .into_iter()
        .chain(excluded.iter().cloned())
        .collect();
    let mut results = Vec::new();
    let mut found = 0_u32;
    // Build folders are measured through the project's open folder while the search holds
    // it, never reopened by path.
    let discovery = projects::discover(
        &roots,
        &skip,
        projects::Limits::default(),
        cancel,
        non_local_mounts,
        |project, dir, dev| {
            let mut folders = Vec::new();
            for name in &project.outputs {
                // A build folder that contains an excluded folder isn't measured.
                if crate::preferences::touches_excluded(&project.path.join(name), excluded) {
                    continue;
                }
                let stat = std::ffi::CString::new(name.as_bytes())
                    .ok()
                    .and_then(|c_name| dir.stat_child(&c_name).ok());
                if let Some(folder) = open_beneath(dir, dev, Path::new(name)) {
                    let measured = measure_dir(folder, dev, &mut links, cancel);
                    let review_id = stat.filter(FileStat::is_dir).map(|stat| {
                        let id = new_id(started_at_ms);
                        review.push(ReviewItem {
                            id: id.clone(),
                            kind: ReviewKind::Build {
                                name: name.clone(),
                                project: project.path.clone(),
                            },
                            path: project.path.join(name),
                            stat,
                            measured: measured.clone(),
                        });
                        id
                    });
                    folders.push(BuildFolder {
                        name: name.clone(),
                        measured,
                        review_id,
                    });
                }
            }
            let bytes = folders
                .iter()
                .map(|folder| folder.measured.bytes)
                .fold(0_u64, u64::saturating_add);
            if bytes > 0 {
                let outputs: Vec<PathBuf> = folders
                    .iter()
                    .map(|folder| project.path.join(&folder.name))
                    .collect();
                results.push(ProjectResult {
                    location: display_path(&project.path, home),
                    name: project.name,
                    stack: project.stack,
                    last_commit_ms: project.last_commit_ms,
                    in_use: open_paths::in_use(open, &project.path, &outputs, home),
                    bytes,
                    folders,
                });
            }
            found = found.saturating_add(1);
            progress(ScanProgress {
                phase: Phase::Projects,
                done: found,
            });
        },
    );
    results.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.location.cmp(&b.location))
    });

    progress(ScanProgress {
        phase: Phase::Docker,
        done: 0,
    });
    let docker = docker::read(home, cancel);
    let cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);
    DeveloperScan {
        status: if cancelled {
            ScanStatus::Cancelled
        } else {
            ScanStatus::Completed
        },
        started_at_ms,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        caches,
        projects: results,
        projects_truncated: discovery.truncated,
        unreadable_folders: discovery.unreadable,
        roots: roots.iter().map(|root| display_path(root, home)).collect(),
        skipped_roots: discovery
            .skipped_roots
            .iter()
            .map(|root| display_path(root, home))
            .collect(),
        docker,
        review_items: review,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AddedFolder {
    Added { name: String },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBusy;

/// The running scan, the folders added this session and what the last scan's review IDs
/// stand for. Nothing is persisted.
#[derive(Default)]
pub struct DeveloperState {
    active: Mutex<Option<Arc<AtomicBool>>>,
    added: Mutex<Vec<PathBuf>>,
    review: Mutex<Vec<ReviewItem>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // State is replaced wholesale, never left half-updated, so a poisoned lock's data is
    // still consistent.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl DeveloperState {
    pub fn begin(&self) -> Result<Arc<AtomicBool>, ScanBusy> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(ScanBusy);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        Ok(cancel)
    }

    pub fn end(&self) {
        *lock(&self.active) = None;
    }

    pub fn cancel(&self) {
        if let Some(cancel) = &*lock(&self.active) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Adds a folder picked in the macOS panel. Returns its name, or `None` if it is no
    /// longer a folder.
    pub fn add(&self, chosen: &Path) -> Option<String> {
        let folder = std::fs::canonicalize(chosen).ok()?;
        if !folder.is_dir() {
            return None;
        }
        let name = folder.file_name().map_or_else(
            || folder.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let mut added = lock(&self.added);
        if !added.contains(&folder) {
            added.push(folder);
        }
        Some(name)
    }

    pub fn added(&self) -> Vec<PathBuf> {
        lock(&self.added).clone()
    }

    /// Stops searching a folder added through the panel; `false` if it wasn't added.
    pub fn remove(&self, folder: &Path) -> bool {
        let mut added = lock(&self.added);
        let before = added.len();
        added.retain(|entry| entry != folder);
        added.len() != before
    }

    /// Replaces the review items with the latest scan's.
    pub fn set_review(&self, items: Vec<ReviewItem>) {
        *lock(&self.review) = items;
    }

    /// The items for these review IDs from the latest scan; unknown IDs are skipped.
    pub fn review_items(&self, ids: &[String]) -> Vec<ReviewItem> {
        let review = lock(&self.review);
        ids.iter()
            .filter_map(|id| review.iter().find(|item| &item.id == id).cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_folders_inside_the_home_folder_are_already_searched() {
        let home = Path::new("/Users/someone");
        let roots = search_roots(
            Some(home),
            &[home.join("Code"), PathBuf::from("/Volumes/Work/projects")],
        );
        assert_eq!(
            roots,
            [
                PathBuf::from("/Users/someone"),
                PathBuf::from("/Volumes/Work/projects")
            ]
        );
        assert_eq!(
            search_roots(None, &[PathBuf::from("/x")]),
            [PathBuf::from("/x")]
        );
    }

    #[test]
    fn cache_locations_are_opened_without_following_symlinks() {
        let home = std::env::temp_dir().join(format!("kleari-cachelink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("elsewhere/_cacache")).expect("target");
        let home = std::fs::canonicalize(&home).expect("canonical");
        std::fs::create_dir_all(home.join(".cargo/registry")).expect("cargo");
        std::os::unix::fs::symlink(home.join("elsewhere"), home.join(".npm")).expect("link");
        assert!(open_cache("~/.cargo/registry", Some(&home)).is_some());
        assert!(open_cache("~/.npm/_cacache", Some(&home)).is_none());
        assert!(open_cache("~/.npm/_cacache", None).is_none());
        // A home folder reached through a symlinked ancestor isn't opened.
        std::os::unix::fs::symlink(&home, home.join("elsewhere/up")).expect("home link");
        assert!(open_cache("~/registry", Some(&home.join(".cargo"))).is_some());
        assert!(open_cache("~/registry", Some(&home.join("elsewhere/up/.cargo"))).is_none());
        std::fs::remove_dir_all(&home).expect("cleanup");
    }

    #[test]
    fn a_scan_of_a_fake_home_finds_caches_projects_and_counts_pnpm_links_once() {
        let home = std::env::temp_dir().join(format!("kleari-developer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let home = {
            std::fs::create_dir_all(&home).expect("home");
            std::fs::canonicalize(&home).expect("canonical")
        };
        let write = |relative: &str, bytes: usize| {
            let path = home.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("folders");
            std::fs::write(&path, vec![1_u8; bytes]).expect("file");
        };
        write(".npm/_cacache/content/a", 2_000_000);
        write("Library/pnpm/store/v3/files/pkg", 1_500_000);
        // Below the minimum: not listed.
        write(".cargo/registry/tiny", 1_000);
        write("Code/web/package.json", 10);
        std::fs::create_dir_all(home.join("Code/web/node_modules")).expect("node_modules");
        std::fs::hard_link(
            home.join("Library/pnpm/store/v3/files/pkg"),
            home.join("Code/web/node_modules/pkg"),
        )
        .expect("link");
        write("Code/web/node_modules/own", 20_000);
        write("Code/api/Cargo.toml", 10);
        write("Code/api/target/debug/app", 40_000);

        let cancel = AtomicBool::new(false);
        let scan = scan(Some(&home), &[], &[], None, &cancel, 0, |_| {});
        assert_eq!(scan.status, ScanStatus::Completed);
        // Caches outside the home folder (the host's /Library) may exist too; only check ours.
        let ids: Vec<&str> = scan
            .caches
            .iter()
            .filter(|cache| {
                cache
                    .locations
                    .iter()
                    .all(|location| location.starts_with('~'))
            })
            .map(|cache| cache.id)
            .collect();
        assert_eq!(ids, ["npm", "pnpmStore"]);
        assert_eq!(scan.caches[0].locations, ["~/.npm/_cacache"]);
        let names: Vec<&str> = scan
            .projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(names, ["api", "web"]);
        // The linked package was counted with the pnpm store, not again in node_modules.
        let web = &scan.projects[1];
        let own = std::fs::metadata(home.join("Code/web/node_modules/own"))
            .map(|m| std::os::unix::fs::MetadataExt::blocks(&m) * 512)
            .expect("own");
        assert_eq!(web.bytes, own);
        assert_eq!(scan.roots, ["~"]);

        // A folder excluded in Settings isn't searched.
        let excluded = super::scan(
            Some(&home),
            &[],
            &[home.join("Code/api")],
            None,
            &cancel,
            0,
            |_| {},
        );
        let names: Vec<&str> = excluded
            .projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(names, ["web"]);
        // An excluded cache folder isn't measured or listed either.
        let no_npm = super::scan(
            Some(&home),
            &[],
            &[home.join(".npm")],
            None,
            &cancel,
            0,
            |_| {},
        );
        assert!(no_npm.caches.iter().all(|cache| cache.id != "npm"));
        assert!(no_npm.caches.iter().any(|cache| cache.id == "pnpmStore"));
        // Nor is a project's build folder that holds an excluded folder.
        let inside = super::scan(
            Some(&home),
            &[],
            &[home.join("Code/api/target/debug")],
            None,
            &cancel,
            0,
            |_| {},
        );
        assert!(inside.projects.iter().all(|project| project.name != "api"));
        // Nor is anything when the home folder itself is excluded.
        let all = super::scan(
            Some(&home),
            &[],
            std::slice::from_ref(&home),
            None,
            &cancel,
            0,
            |_| {},
        );
        assert!(all.projects.is_empty());
        std::fs::remove_dir_all(&home).expect("cleanup");
    }
}
