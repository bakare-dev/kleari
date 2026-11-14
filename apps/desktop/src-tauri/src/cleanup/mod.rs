//! Cleanup: a read-only scan finds items, the user reviews them, a preview binds the chosen
//! ones to their identity and size, and only an explicit approval moves them into quarantine
//! (ADR-003, ADR-012). Items sent from the Storage and Developer pages join the same review.
//!
//! Everything about an item that matters for safety stays here: its real path and identity
//! never leave Rust; the frontend gets an opaque ID, names, a `~/…` location and the policy's
//! decision. Eligibility is decided by `policy::assess` and checked again, together with the
//! item's place (`policy::within_bounds`), identity and size, immediately before the move.

pub mod execute;
pub mod intake;
pub mod policy;
pub mod scan;
pub mod scope;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::developer::{measure_dir, open_absolute, Links};
use crate::quarantine::Identity;
use crate::storage::category::{category_of, FileCategory};
use crate::storage::fsdir::FileStat;
use crate::system_monitor::display_path;
use crate::system_monitor::open_paths::{self, OpenPaths};
use crate::system_monitor::running_apps::RunningBundle;
use policy::{Assessment, Category, Certainty, Eligibility, Evidence, Kind, Risk, WhyNot};
pub use scan::{ScanMode, ScanOptions};

/// Where an item reached the review from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Scan,
    Storage,
    Developer,
    /// The Apps page: leftovers of removed apps and installers.
    Apps,
}

/// One item as Rust keeps it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub kind: Kind,
    /// The absolute path, never sent to the frontend.
    pub path: PathBuf,
    pub identity: Identity,
    pub name: String,
    /// Its folder, shown as `~/…`.
    pub location: String,
    pub bytes: u64,
    pub files: u64,
    /// The size is a minimum (something couldn't be read).
    pub lower_bound: bool,
    pub modified_ms: Option<u64>,
    pub assessment: Assessment,
    /// What the scan observed, kept so the policy can run again before anything moves.
    pub facts: StoredFacts,
    pub source: Source,
    /// The project a build folder belongs to.
    pub project: Option<PathBuf>,
}

/// The facts behind an assessment that don't change from moment to moment. Running apps,
/// what is open (whether a project is in use) and the volume are read again whenever the
/// policy re-runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoredFacts {
    pub lockfile: bool,
    pub idle_days: u64,
    pub larger_than: u64,
    /// For app leftovers: their app was removed with Kleari.
    pub owner_removed: bool,
}

const XCODE_BUNDLE: &str = "com.apple.dt.Xcode";
const SIMULATOR_BUNDLE: &str = "com.apple.iphonesimulator";

/// Runs the policy for `kind` with the stored facts, the apps running now and whether its
/// project is in use now.
pub fn assess_now(
    kind: &Kind,
    name: &str,
    stored: StoredFacts,
    running: &[RunningBundle],
    project_in_use: bool,
    same_volume: bool,
) -> Assessment {
    // A leftover belongs to its app, matched by bundle ID or name; a cache to the app whose
    // bundle ID it is named.
    let running_app = match kind {
        Kind::AppLeftover { owner } => running
            .iter()
            .find(|app| app.id.eq_ignore_ascii_case(owner) || app.name.eq_ignore_ascii_case(owner)),
        // `<bundle ID>.plist` belongs to the app with that bundle ID.
        Kind::BrokenPreference => {
            let id = name.strip_suffix(".plist").unwrap_or(name);
            running.iter().find(|app| app.id.eq_ignore_ascii_case(id))
        }
        _ => running.iter().find(|app| app.id.eq_ignore_ascii_case(name)),
    }
    .map(|app| app.name.as_str());
    policy::assess(
        kind,
        &policy::Facts {
            name,
            running_app,
            xcode_running: running.iter().any(|app| app.id == XCODE_BUNDLE),
            simulator_running: running.iter().any(|app| app.id == SIMULATOR_BUNDLE),
            lockfile: stored.lockfile,
            project_active: project_in_use,
            idle_days: stored.idle_days,
            larger_than: stored.larger_than,
            same_volume,
            owner_removed: stored.owner_removed,
        },
    )
}

impl Candidate {
    /// Whether this item's project has a file or its folder open in some app now. Without a
    /// reading of what is open, every project counts as in use (fail closed). The item is the
    /// project's build folder, the only part that counts for a project at the home folder.
    pub fn project_in_use(&self, open: Option<&OpenPaths>) -> bool {
        let home = crate::storage::home_dir();
        self.project.as_deref().is_some_and(|project| {
            open_paths::in_use(
                open,
                project,
                std::slice::from_ref(&self.path),
                home.as_deref(),
            )
        })
    }

    /// Runs the policy again now that the item's project is known.
    pub fn reassess(
        &mut self,
        running: &[RunningBundle],
        open: Option<&OpenPaths>,
        same_volume: bool,
    ) {
        let name = display_name_of(&self.path);
        let in_use = self.project_in_use(open);
        self.assessment = assess_now(&self.kind, &name, self.facts, running, in_use, same_volume);
    }
}

/// One item as the Review page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub name: String,
    pub category: Category,
    /// The folder it is in, shown as `~/…`.
    pub location: String,
    /// The item itself, shown as `~/…` (for Copy path).
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub lower_bound: bool,
    pub modified_ms: Option<u64>,
    pub risk: Risk,
    pub certainty: Certainty,
    pub eligibility: Eligibility,
    pub why_not: Option<WhyNot>,
    pub evidence: Vec<Evidence>,
    pub source: Source,
    /// The Developer catalog ID of a known tool cache; the frontend has its name.
    pub tool: Option<&'static str>,
    /// What kind of file a large or old file is (video, disk image, …), by its extension.
    pub file_category: Option<FileCategory>,
}

impl Candidate {
    pub fn category(&self) -> Category {
        policy::category(&self.kind)
    }

    pub fn finding(&self) -> Finding {
        Finding {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category(),
            location: self.location.clone(),
            path: format!(
                "{}/{}",
                self.location.trim_end_matches('/'),
                display_name_of(&self.path)
            ),
            bytes: self.bytes,
            files: self.files,
            lower_bound: self.lower_bound,
            modified_ms: self.modified_ms,
            risk: self.assessment.risk,
            certainty: self.assessment.certainty,
            eligibility: self.assessment.eligibility,
            why_not: self.assessment.why_not,
            evidence: self.assessment.evidence.clone(),
            source: self.source,
            tool: match self.kind {
                Kind::Catalog(id) => Some(id),
                _ => None,
            },
            file_category: matches!(self.kind, Kind::LargeFile | Kind::OldFile)
                .then(|| category_of(&self.path)),
        }
    }

    pub fn eligible(&self) -> bool {
        self.assessment.eligibility == Eligibility::Eligible
    }
}

/// The current review: the last scan's findings plus anything sent from Storage or
/// Developer since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub scan: Option<ScanInfo>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanInfo {
    pub mode: ScanMode,
    pub started_at_ms: u64,
    pub cancelled: bool,
}

pub use crate::ids::new_id;

/// Whether a finding is kept after the user's preferences: nothing inside a folder they
/// excluded, nothing smaller than their minimum size, and no large file changed recently.
/// These only leave findings out; nothing becomes eligible because of them.
pub fn kept_by_preferences(
    item: &Candidate,
    preferences: &crate::preferences::Preferences,
    excluded: &[PathBuf],
    now_ms: u64,
) -> bool {
    let recent = preferences
        .recent_cutoff_ms(now_ms)
        .is_some_and(|cutoff| item.modified_ms.is_some_and(|ms| ms >= cutoff));
    // Nothing inside an excluded folder, and nothing that contains one (it would be moved too).
    !crate::preferences::touches_excluded(&item.path, excluded)
        && item.bytes >= preferences.min_size_bytes()
        && !(matches!(item.kind, Kind::LargeFile) && recent)
}

/// Whether Show in Finder may reveal a review item: not in, or containing, a folder excluded
/// now (checked first, so an excluded path isn't read), and still the item that was found.
pub fn revealable(item: &Candidate, excluded: &[PathBuf]) -> bool {
    !crate::preferences::touches_excluded(&item.path, excluded)
        && stat_path(&item.path).is_some_and(|stat| Identity::of(&stat) == item.identity)
}

/// `lstat` of an absolute path, opened from `/` without following a symlink anywhere on the
/// way (and never under a non-local mount).
pub fn stat_path(path: &Path) -> Option<FileStat> {
    let (parent, name) = crate::quarantine::split(path)?;
    let (dir, _) = open_absolute(parent)?;
    dir.stat_child(&name).ok()
}

/// What an item is now: its identity and size. `None` if it is gone, isn't a plain folder
/// or file, or can't be opened safely.
pub fn current_state(path: &Path, cancel: &AtomicBool) -> Option<(Identity, u64)> {
    let (parent, name) = crate::quarantine::split(path)?;
    let (dir, _) = open_absolute(parent)?;
    let stat = dir.stat_child(&name).ok()?;
    if stat.is_symlink() {
        return None;
    }
    let identity = Identity::of(&stat);
    if !stat.is_dir() {
        return stat
            .is_file()
            .then(|| (identity, stat.blocks.saturating_mul(512)));
    }
    let (child, opened) = dir.open_child(&name).ok()?;
    if opened.dev != stat.dev || opened.ino != stat.ino {
        return None;
    }
    let measured = measure_dir(child, stat.dev, &mut Links::default(), cancel);
    Some((identity, measured.bytes))
}

pub fn display_name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, crate::storage::fsdir::display_name)
}

pub fn location_of(path: &Path, home: Option<&Path>) -> String {
    path.parent()
        .map_or_else(String::new, |parent| display_path(parent, home))
}

/// A preview the user is looking at: each item bound to the identity and size checked when
/// it was made.
#[derive(Debug, Clone)]
pub struct Preview {
    pub id: String,
    pub items: Vec<Bound>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The review item's ID.
    pub id: String,
    pub identity: Identity,
    pub bytes: u64,
}

#[derive(Default)]
struct Inner {
    scan: Option<(ScanInfo, Vec<Candidate>)>,
    sent: Vec<Candidate>,
    preview: Option<Preview>,
    /// Folders added through the picker this session.
    added: Vec<PathBuf>,
}

/// The review, the current preview and the running scan. Nothing here is persisted; the
/// history of what moved is in the database.
#[derive(Default)]
pub struct CleanupState {
    inner: Mutex<Inner>,
    active: Mutex<Option<Arc<AtomicBool>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // State is replaced wholesale, never left half-updated, so a poisoned lock's data is
    // still consistent.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBusy;

impl CleanupState {
    pub fn begin_scan(&self) -> Result<Arc<AtomicBool>, ScanBusy> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(ScanBusy);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        Ok(cancel)
    }

    pub fn end_scan(&self) {
        *lock(&self.active) = None;
    }

    pub fn cancel_scan(&self) {
        if let Some(cancel) = &*lock(&self.active) {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Replaces the scan findings. The preview is dropped: it may refer to them.
    pub fn set_scan(&self, info: ScanInfo, items: Vec<Candidate>) {
        let mut inner = lock(&self.inner);
        // An item sent from another page stays; the scan's own copy of it is dropped.
        let sent: Vec<&Path> = inner.sent.iter().map(|item| item.path.as_path()).collect();
        let items: Vec<Candidate> = items
            .into_iter()
            .filter(|item| !sent.contains(&item.path.as_path()))
            .collect();
        inner.scan = Some((info, items));
        inner.preview = None;
    }

    /// Adds items sent from Storage or Developer. One already in the review (by path) is
    /// replaced by the new one. Returns the IDs now in the review for them.
    pub fn add_sent(&self, items: Vec<Candidate>) -> Vec<String> {
        let mut inner = lock(&self.inner);
        let mut ids = Vec::with_capacity(items.len());
        for item in items {
            inner.sent.retain(|existing| existing.path != item.path);
            if let Some((_, scanned)) = inner.scan.as_mut() {
                scanned.retain(|existing| existing.path != item.path);
            }
            ids.push(item.id.clone());
            inner.sent.push(item);
        }
        inner.preview = None;
        ids
    }

    pub fn review(&self) -> Review {
        let inner = lock(&self.inner);
        let mut findings: Vec<Finding> = inner
            .sent
            .iter()
            .chain(inner.scan.iter().flat_map(|(_, items)| items.iter()))
            .map(Candidate::finding)
            .collect();
        findings.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        Review {
            scan: inner.scan.as_ref().map(|(info, _)| *info),
            findings,
        }
    }

    /// The items with these IDs, in the order given; unknown IDs are skipped.
    pub fn items(&self, ids: &[String]) -> Vec<Candidate> {
        let inner = lock(&self.inner);
        ids.iter()
            .filter_map(|id| {
                inner
                    .sent
                    .iter()
                    .chain(inner.scan.iter().flat_map(|(_, items)| items.iter()))
                    .find(|item| &item.id == id)
                    .cloned()
            })
            .collect()
    }

    /// The one item with this ID.
    pub fn item(&self, id: &str) -> Option<Candidate> {
        self.items(std::slice::from_ref(&id.to_owned()))
            .into_iter()
            .next()
    }

    pub fn set_preview(&self, preview: Option<Preview>) {
        lock(&self.inner).preview = preview;
    }

    pub fn preview(&self) -> Option<Preview> {
        lock(&self.inner).preview.clone()
    }

    /// Removes moved items from the review and drops the preview.
    pub fn remove(&self, ids: &[String]) {
        let mut inner = lock(&self.inner);
        inner.sent.retain(|item| !ids.contains(&item.id));
        if let Some((_, items)) = inner.scan.as_mut() {
            items.retain(|item| !ids.contains(&item.id));
        }
        inner.preview = None;
    }

    /// Marks an item as changed since the scan: not selectable until the next scan.
    pub fn mark_changed(&self, id: &str) {
        let mut inner = lock(&self.inner);
        let Inner { sent, scan, .. } = &mut *inner;
        for item in sent
            .iter_mut()
            .chain(scan.iter_mut().flat_map(|(_, items)| items.iter_mut()))
            .filter(|item| item.id == id && item.eligible())
        {
            item.assessment.eligibility = Eligibility::Rescan;
            item.assessment.why_not = Some(WhyNot::ChangedSinceScan);
        }
    }

    pub fn add_folder(&self, folder: PathBuf) -> bool {
        let mut inner = lock(&self.inner);
        if inner.added.contains(&folder) {
            return false;
        }
        inner.added.push(folder);
        true
    }

    pub fn added_folders(&self) -> Vec<PathBuf> {
        lock(&self.inner).added.clone()
    }

    /// Stops scanning a folder added through the panel; `false` if it wasn't added.
    pub fn remove_folder(&self, folder: &Path) -> bool {
        let mut inner = lock(&self.inner);
        let before = inner.added.len();
        inner.added.retain(|added| added != folder);
        inner.added.len() != before
    }
}
