//! Read-only storage discovery (docs/storage-discovery.md). The frontend never supplies a
//! path: the user picks a folder in a native panel opened from Rust, Rust stores it under a
//! grant ID, and every scan or query refers to that ID.

pub mod analysis;
pub(crate) mod category;
pub mod explorer;
// Also used by the System page's startup-item check (system_monitor::startup).
pub(crate) mod fsdir;
pub mod picker;
mod scanner;
pub mod volume;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use category::FileCategory;
use scanner::{root_identity, RootIdentity, TopLevelKind};
pub use scanner::{scan, Progress, ScanError, ScanOutcome};

/// Files returned per query.
pub const RESULT_LIMIT: usize = 50;
const MAX_NAME_FILTER_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FolderChoice {
    #[serde(rename_all = "camelCase")]
    Chosen {
        grant_id: String,
        display_name: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    Completed,
    /// Stopped by the user; totals and lists cover only what was read.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalsDto {
    pub bytes_on_disk: u64,
    pub files: u64,
    pub directories: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDto {
    pub unreadable: u64,
    pub symlinks_skipped: u64,
    pub other_volumes_skipped: u64,
    pub too_deep: u64,
    pub hard_links_unverified: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TopLevelKindDto {
    Folder,
    Files,
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopLevelDto {
    /// Stable within one scan; used for filtering so names never need to be unique.
    pub id: u32,
    /// Folder name with invalid UTF-8 escaped; empty for `files` and `others`.
    pub name: String,
    pub kind: TopLevelKindDto,
    pub bytes_on_disk: u64,
    pub files: u64,
    /// Items inside this row that couldn't be read; when non-zero the size is a lower bound.
    pub unreadable: u64,
    /// Top-level items this row covers (more than one for `others`).
    pub items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDto {
    pub relative_path: String,
    pub bytes_on_disk: u64,
    pub category: FileCategory,
    /// `id` of the top-level row the file belongs to.
    pub top_level: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub files: Vec<FileDto>,
    pub matched_count: u64,
    /// True when the scan had more files than the index keeps, so filters search only the
    /// largest ones.
    pub index_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub grant_id: String,
    pub display_name: String,
    pub status: ScanStatus,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub totals: TotalsDto,
    pub coverage: CoverageDto,
    pub top_level: Vec<TopLevelDto>,
    pub indexed_files: u64,
    pub largest: QueryResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub files: u64,
    pub bytes_on_disk: u64,
    pub current: String,
}

impl From<Progress> for ProgressDto {
    fn from(progress: Progress) -> Self {
        Self {
            files: progress.files,
            bytes_on_disk: progress.bytes_on_disk,
            current: progress.current,
        }
    }
}

/// Filters for the largest-files list. Unknown fields are rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileFilter {
    pub name: Option<String>,
    pub category: Option<FileCategory>,
    /// A top-level row `id` from the scan summary.
    pub top_level: Option<u32>,
    pub min_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFilter;

/// Case-insensitive substring match. ASCII needles are compared without allocating; other
/// needles fall back to full Unicode lowercasing of the path.
fn contains_case_insensitive(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_ascii() {
        let needle = needle_lower.as_bytes();
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn row_id(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// Returns the [`RESULT_LIMIT`] largest files matching every filter, plus the match count.
pub fn query(outcome: &ScanOutcome, filter: &FileFilter) -> Result<QueryResult, InvalidFilter> {
    let name = match &filter.name {
        Some(name) if name.chars().count() > MAX_NAME_FILTER_CHARS => return Err(InvalidFilter),
        Some(name) if !name.trim().is_empty() => Some(name.trim().to_lowercase()),
        _ => None,
    };
    let top_level = match filter.top_level {
        Some(id) => {
            let position = usize::try_from(id).map_err(|_| InvalidFilter)?;
            if position >= outcome.top_level.len() {
                return Err(InvalidFilter);
            }
            Some(position)
        }
        None => None,
    };

    let mut matched_count = 0u64;
    let mut files = Vec::new();
    for file in &outcome.index {
        let matches = filter.min_bytes.is_none_or(|min| file.bytes_on_disk >= min)
            && filter
                .category
                .is_none_or(|category| file.category == category)
            && top_level.is_none_or(|row| file.top_level == row)
            && name
                .as_deref()
                .is_none_or(|name| contains_case_insensitive(&file.relative_path, name));
        if !matches {
            continue;
        }
        matched_count += 1;
        if files.len() < RESULT_LIMIT {
            files.push(FileDto {
                relative_path: file.relative_path.clone(),
                bytes_on_disk: file.bytes_on_disk,
                category: file.category,
                top_level: row_id(file.top_level),
            });
        }
    }
    Ok(QueryResult {
        files,
        matched_count,
        index_truncated: outcome.index_truncated,
    })
}

pub fn summarize(
    grant_id: &str,
    display_name: &str,
    outcome: &ScanOutcome,
    started_at_ms: u64,
    duration_ms: u64,
) -> ScanSummary {
    let coverage = &outcome.coverage;
    ScanSummary {
        grant_id: grant_id.to_owned(),
        display_name: display_name.to_owned(),
        status: if outcome.cancelled {
            ScanStatus::Cancelled
        } else {
            ScanStatus::Completed
        },
        started_at_ms,
        duration_ms,
        totals: TotalsDto {
            bytes_on_disk: outcome.totals.bytes_on_disk,
            files: outcome.totals.files,
            directories: outcome.totals.directories,
        },
        coverage: CoverageDto {
            unreadable: coverage.unreadable,
            symlinks_skipped: coverage.symlinks_skipped,
            other_volumes_skipped: coverage.other_volumes_skipped,
            too_deep: coverage.too_deep,
            hard_links_unverified: coverage.hard_links_unverified,
        },
        top_level: outcome
            .top_level
            .iter()
            .enumerate()
            .map(|(position, row)| TopLevelDto {
                id: row_id(position),
                name: row.name.clone(),
                kind: match row.kind {
                    TopLevelKind::Folder => TopLevelKindDto::Folder,
                    TopLevelKind::Files => TopLevelKindDto::Files,
                    TopLevelKind::Others => TopLevelKindDto::Others,
                },
                bytes_on_disk: row.bytes_on_disk,
                files: row.files,
                unreadable: row.unreadable,
                items: row.items,
            })
            .collect(),
        indexed_files: u64::try_from(outcome.index.len()).unwrap_or(u64::MAX),
        largest: query(outcome, &FileFilter::default())
            .unwrap_or_else(|InvalidFilter| unreachable!("the empty filter is always valid")),
    }
}

/// A folder the user chose in the native panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub root: PathBuf,
    pub identity: RootIdentity,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    GrantNotFound,
    ScanInProgress,
    NoScanResult,
}

/// The last finished scan, shared so filter queries can run off the main thread.
pub struct LastScan {
    pub grant_id: String,
    pub outcome: ScanOutcome,
}

/// In-memory storage-discovery state for the app session; nothing here is persisted.
#[derive(Default)]
pub struct StorageState {
    grants: Mutex<HashMap<String, Grant>>,
    next_grant: AtomicU64,
    /// The running scan: its grant ID and cancellation token.
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
    /// The last finished scan, kept for filter queries. Dropped when a new scan starts.
    last: Mutex<Option<Arc<LastScan>>>,
}

/// The user's home folder from `HOME`, when it is an absolute path.
pub fn home_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_absolute().then_some(home)
}

/// The user's temporary folder, with its symlinked ancestors (`/var`) resolved once so it can
/// be opened without following symlinks.
pub fn temp_dir() -> Option<PathBuf> {
    std::fs::canonicalize(std::env::temp_dir()).ok()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // State is replaced wholesale, never left half-updated, so a poisoned lock's data is
    // still consistent.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl StorageState {
    /// Registers a chosen folder. Returns `None` if it is not (or is no longer) a directory.
    pub fn add_grant(&self, chosen: &Path) -> Option<(String, String)> {
        let root = std::fs::canonicalize(chosen).ok()?;
        let identity = root_identity(&root)?;
        let display_name = root.file_name().map_or_else(
            || fsdir::display_name(root.as_os_str()),
            fsdir::display_name,
        );
        let id = format!("g{}", self.next_grant.fetch_add(1, Ordering::Relaxed) + 1);
        lock(&self.grants).insert(
            id.clone(),
            Grant {
                root,
                identity,
                display_name: display_name.clone(),
            },
        );
        Some((id, display_name))
    }

    /// Marks a scan as running and drops the previous result to free its index.
    pub fn begin_scan(&self, grant_id: &str) -> Result<(Grant, Arc<AtomicBool>), StateError> {
        let grant = lock(&self.grants)
            .get(grant_id)
            .cloned()
            .ok_or(StateError::GrantNotFound)?;
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(StateError::ScanInProgress);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((grant_id.to_owned(), Arc::clone(&cancel)));
        *lock(&self.last) = None;
        Ok((grant, cancel))
    }

    /// Clears the running scan and keeps its outcome (if any) for queries.
    pub fn end_scan(&self, grant_id: &str, outcome: Option<ScanOutcome>) {
        *lock(&self.active) = None;
        if let Some(outcome) = outcome {
            *lock(&self.last) = Some(Arc::new(LastScan {
                grant_id: grant_id.to_owned(),
                outcome,
            }));
        }
    }

    /// Requests cancellation of the running scan for `grant_id`; returns whether one was running.
    pub fn cancel(&self, grant_id: &str) -> bool {
        match &*lock(&self.active) {
            Some((active_id, cancel)) if active_id == grant_id => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// The last scan for `grant_id`, to query off the main thread.
    pub fn last_scan(&self, grant_id: &str) -> Result<Arc<LastScan>, StateError> {
        match &*lock(&self.last) {
            Some(last) if last.grant_id == grant_id => Ok(Arc::clone(last)),
            _ => Err(StateError::NoScanResult),
        }
    }
}

#[cfg(test)]
mod tests;
