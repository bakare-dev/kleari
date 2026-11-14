//! Preview and approval. A preview re-reads each selected item and binds it to its identity
//! and size at that moment. Approval checks every bound item again before anything moves:
//! same identity, same size, same place, still eligible with the apps running now. If any
//! item differs, nothing moves and the preview must be rebuilt. Only then is the batch
//! recorded (so every move has a record of where the item came from) and each item moved,
//! one at a time, with its outcome reported and recorded. A move that fails leaves the item
//! where it was; nothing is ever deleted here.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::policy::{self, Eligibility};
use super::{assess_now, current_state, new_id, Bound, Candidate, CleanupState, Preview};
use crate::history::History;
use crate::persistence::{BatchRecord, Event, EventKind, ItemRecord, ItemState, Unavailable};
use crate::quarantine::Failure;
use crate::system_monitor::open_paths::OpenPaths;
use crate::system_monitor::running_apps::RunningBundle;

/// What the checks need: where things may be, what is running, where the quarantine is.
pub struct Checks<'a> {
    pub home: &'a Path,
    pub temp: Option<&'a Path>,
    pub running: &'a [RunningBundle],
    /// What is open now; `None` (couldn't be read) counts every project as in use.
    pub open: Option<&'a OpenPaths>,
    pub store_dev: Option<u64>,
    /// Folders added through the picker, where build folders may also come from.
    pub added: &'a [PathBuf],
    pub cancel: &'a AtomicBool,
    /// The user's current exclusions and filters: an item they now leave out isn't planned,
    /// and if it was, approval moves nothing.
    pub keep: &'a dyn Fn(&Candidate) -> bool,
    /// Folders excluded now: an item in or containing one is refused before it is measured.
    pub excluded: &'a [PathBuf],
}

impl Checks<'_> {
    fn in_bounds(&self, item: &Candidate) -> bool {
        let home = self.home;
        policy::within_bounds(&item.kind, &item.path, home, self.temp, |id| {
            policy::catalog_paths(home, id)
        }) || match &item.kind {
            policy::Kind::BuildFolder { name } => {
                policy::build_folder_in_added(&item.path, name, home, self.added)
            }
            _ => false,
        }
    }

    /// The added folder a build folder outside the home folder's fixed places is allowed
    /// under, recorded with it so a restore can check its place again.
    fn added_root(&self, item: &Candidate) -> Option<PathBuf> {
        let home = self.home;
        let policy::Kind::BuildFolder { name } = &item.kind else {
            return None;
        };
        if policy::within_bounds(&item.kind, &item.path, home, self.temp, |id| {
            policy::catalog_paths(home, id)
        }) {
            return None;
        }
        self.added
            .iter()
            .find(|root| {
                policy::build_folder_in_added(&item.path, name, home, std::slice::from_ref(root))
            })
            .cloned()
    }

    /// Not in, and not containing, a folder excluded now. Checked before anything about the
    /// item is read again, so an excluded folder is never measured.
    fn clear_of_exclusions(&self, item: &Candidate) -> bool {
        !crate::preferences::touches_excluded(&item.path, self.excluded)
    }

    /// Kept by the user's current preferences, judged on what is there now: the size just
    /// measured (`bytes`) and the modification time read again. If that can't be read, it isn't
    /// kept.
    fn kept_now(&self, item: &Candidate, bytes: u64) -> bool {
        let Some(stat) = super::stat_path(&item.path) else {
            return false;
        };
        let fresh = Candidate {
            bytes,
            modified_ms: u64::try_from(stat.modified_s)
                .ok()
                .and_then(|seconds| seconds.checked_mul(1000))
                // A folder's newest file was measured at the scan; its own time can only be
                // newer, so the later of the two is used.
                .map(|ms| item.modified_ms.map_or(ms, |scanned| scanned.max(ms))),
            ..item.clone()
        };
        (self.keep)(&fresh)
    }

    /// Eligible right now: with the apps running now, its project not in use and on the
    /// quarantine's volume.
    fn eligible_now(&self, item: &Candidate) -> bool {
        let same_volume = self.store_dev.is_some_and(|dev| dev == item.identity.dev);
        let file_name = super::display_name_of(&item.path);
        item.eligible()
            && assess_now(
                &item.kind,
                &file_name,
                item.facts,
                self.running,
                item.project_in_use(self.open),
                same_volume,
            )
            .eligibility
                == Eligibility::Eligible
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlannedState {
    /// Will move when the preview is approved.
    Planned,
    /// Changed since the scan (gone, replaced); left out until you scan again.
    Changed,
    /// Not eligible now (an app opened, or it is no longer allowed); left out.
    NotEligible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItem {
    pub id: String,
    pub state: PlannedState,
    /// The size measured for the preview (for planned items), otherwise the scan's.
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub id: String,
    pub prepared_at_ms: u64,
    pub items: Vec<PlannedItem>,
}

/// Makes a preview of the selected items: each is checked and measured again now.
pub fn create_preview(
    state: &CleanupState,
    ids: &[String],
    checks: &Checks<'_>,
    now_ms: u64,
) -> PreviewView {
    let mut planned = Vec::with_capacity(ids.len());
    let mut bound = Vec::new();
    for item in state.items(ids) {
        let (state_now, bytes) = if !checks.eligible_now(&item)
            || !checks.in_bounds(&item)
            || !checks.clear_of_exclusions(&item)
        {
            (PlannedState::NotEligible, item.bytes)
        } else {
            match current_state(&item.path, checks.cancel) {
                // Left out by the user's preferences as it is now (smaller, recently changed,
                // or excluded since the scan): not planned.
                Some((identity, bytes))
                    if identity == item.identity && !checks.kept_now(&item, bytes) =>
                {
                    (PlannedState::NotEligible, bytes)
                }
                Some((identity, bytes)) if identity == item.identity => {
                    bound.push(Bound {
                        id: item.id.clone(),
                        identity,
                        bytes,
                    });
                    (PlannedState::Planned, bytes)
                }
                _ => {
                    state.mark_changed(&item.id);
                    (PlannedState::Changed, item.bytes)
                }
            }
        };
        planned.push(PlannedItem {
            id: item.id,
            state: state_now,
            bytes,
        });
    }
    let id = new_id(now_ms);
    state.set_preview(Some(Preview {
        id: id.clone(),
        items: bound,
    }));
    PreviewView {
        id,
        prepared_at_ms: now_ms,
        items: planned,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveFailure {
    NeedsAccess,
    Missing,
    Changed,
    OtherVolume,
    Exists,
    FolderUnavailable,
    QuarantineUnavailable,
    Failed,
}

fn move_failure(failure: Failure) -> MoveFailure {
    match failure {
        Failure::NeedsAccess => MoveFailure::NeedsAccess,
        Failure::Missing => MoveFailure::Missing,
        Failure::Changed => MoveFailure::Changed,
        Failure::OtherVolume => MoveFailure::OtherVolume,
        Failure::Exists => MoveFailure::Exists,
        Failure::ParentUnavailable => MoveFailure::FolderUnavailable,
        Failure::StoreUnavailable => MoveFailure::QuarantineUnavailable,
        // Handled separately: the rename happened.
        Failure::Other | Failure::Unconfirmed => MoveFailure::Failed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum MoveState {
    Checking,
    Moved,
    /// The rename happened but couldn't be checked. The record stays `moving` and is resolved
    /// against the quarantine folder at the next launch; it is never reported as left in
    /// place.
    Unconfirmed,
    Failed {
        reason: MoveFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveProgress {
    pub id: String,
    #[serde(flatten)]
    pub state: MoveState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    pub id: String,
    #[serde(flatten)]
    pub state: MoveState,
    /// The quarantine item it became, set only when it moved and was confirmed (`Moved`), so
    /// "Delete permanently" can delete exactly what this approval moved, through Quarantine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ApproveOutcome {
    /// There is no such preview (it was replaced, or the review changed since).
    NoPreview,
    /// Something changed after the preview was made; nothing moved.
    Stale { changed: Vec<String> },
    /// The history can't be written, so nothing moved.
    #[serde(rename_all = "camelCase")]
    HistoryUnavailable { reason: Unavailable },
    #[serde(rename_all = "camelCase")]
    Finished {
        batch_id: String,
        finished_at_ms: u64,
        items: Vec<MoveResult>,
    },
}

/// Approves the preview `preview_id`: checks every item again, then moves each one into a new
/// quarantine batch named by `origin`.
pub fn approve(
    state: &CleanupState,
    preview_id: &str,
    checks: &Checks<'_>,
    history: &History,
    origin: &str,
    now_ms: impl Fn() -> u64,
    mut progress: impl FnMut(MoveProgress),
) -> ApproveOutcome {
    let (db, store) = (&history.db, &history.store);
    let Some(preview) = state.preview().filter(|preview| preview.id == preview_id) else {
        return ApproveOutcome::NoPreview;
    };
    let ids: Vec<String> = preview.items.iter().map(|entry| entry.id.clone()).collect();
    let items = state.items(&ids);
    let mut bound: Vec<(&Candidate, &Bound)> = Vec::new();
    for entry in &preview.items {
        // Every bound item must still be in the review; otherwise the preview is void.
        let Some(item) = items.iter().find(|item| item.id == entry.id) else {
            return ApproveOutcome::NoPreview;
        };
        bound.push((item, entry));
    }

    // Everything is checked before anything moves.
    let mut changed = Vec::new();
    for (item, entry) in &bound {
        let same = checks.eligible_now(item)
            && checks.in_bounds(item)
            && checks.clear_of_exclusions(item)
            && current_state(&item.path, checks.cancel) == Some((entry.identity, entry.bytes))
            && checks.kept_now(item, entry.bytes);
        if !same {
            changed.push(item.id.clone());
        }
    }
    if !changed.is_empty() {
        state.set_preview(None);
        return ApproveOutcome::Stale { changed };
    }

    if let Some(reason) = db.unavailable() {
        return ApproveOutcome::HistoryUnavailable { reason };
    }
    let started = now_ms();
    let batch = BatchRecord {
        id: new_id(started),
        created_ms: started,
        origin: origin.to_owned(),
    };
    let records: Vec<ItemRecord> = bound
        .iter()
        .map(|(item, entry)| ItemRecord {
            id: new_id(started),
            batch_id: batch.id.clone(),
            name: item.name.clone(),
            category: item.category().as_str().to_owned(),
            original: item.path.clone(),
            root: checks.added_root(item),
            bytes: entry.bytes,
            dev: entry.identity.dev,
            ino: entry.identity.ino,
            is_dir: entry.identity.is_dir,
            state: ItemState::Moving,
            updated_ms: started,
        })
        .collect();
    if let Err(reason) = db.begin_batch(&batch, &records) {
        return ApproveOutcome::HistoryUnavailable { reason };
    }

    let mut results = Vec::with_capacity(bound.len());
    let mut lines = Vec::with_capacity(bound.len());
    let mut moved_ids = Vec::new();
    let mut moved_bytes = 0_u64;
    for ((item, entry), record) in bound.iter().zip(&records) {
        progress(MoveProgress {
            id: item.id.clone(),
            state: MoveState::Checking,
        });
        // Immediately before this item's rename: the preferences once more, on its time read
        // now (an earlier item's move may have taken a while), then `move_in` checks the
        // identity. A change in between leaves it where it is.
        let outcome = if checks.clear_of_exclusions(item) && checks.kept_now(item, entry.bytes) {
            store.move_in(&item.path, entry.identity, &batch.id, &record.id)
        } else {
            Err(Failure::Changed)
        };
        let state_now = match outcome {
            Ok(actual) => {
                // A swapped entry that couldn't be moved back is what is held now: the record
                // says so, so it can be found and restored. If that write fails the record
                // keeps the old identity and the item stays `moving`, which the next launch
                // leaves unresolved rather than concluding anything.
                let identity_ok = actual == entry.identity
                    || db
                        .set_identity(&record.id, actual.dev, actual.ino, actual.is_dir)
                        .is_ok();
                // It is in quarantine either way. Only a fully recorded move is reported as
                // moved (restorable from Quarantine now). Otherwise the record stays `moving`,
                // is shown as pending in Quarantine, and is reported as unconfirmed.
                if identity_ok && db.set_state(&record.id, ItemState::Held, now_ms()).is_ok() {
                    MoveState::Moved
                } else {
                    MoveState::Unconfirmed
                }
            }
            // The rename happened; the record stays `moving` for the next launch to resolve
            // from the folder, and the item is never reported as left in place.
            Err(Failure::Unconfirmed) => MoveState::Unconfirmed,
            Err(failure) => {
                // Nothing moved. The record can only be closed if the history still writes; a
                // `moving` record left behind is resolved against the folder on the next open.
                let _ = db.set_state(&record.id, ItemState::NotMoved, now_ms());
                MoveState::Failed {
                    reason: move_failure(failure),
                }
            }
        };
        if matches!(outcome, Ok(_) | Err(Failure::Unconfirmed)) {
            moved_ids.push(item.id.clone());
            moved_bytes = moved_bytes.saturating_add(entry.bytes);
        }
        progress(MoveProgress {
            id: item.id.clone(),
            state: state_now,
        });
        lines.push((
            record.id.clone(),
            match state_now {
                MoveState::Moved => "moved",
                MoveState::Unconfirmed | MoveState::Checking => "unconfirmed",
                MoveState::Failed { .. } => "notMoved",
            },
        ));
        results.push(MoveResult {
            id: item.id.clone(),
            held_id: matches!(state_now, MoveState::Moved).then(|| record.id.clone()),
            state: state_now,
        });
    }
    let failed = results.len() - moved_ids.len();
    let finished = now_ms();
    // The outcome is already shown; a failed event write leaves the item records intact.
    let _ = db.record_event(&Event {
        at_ms: finished,
        kind: EventKind::Quarantine,
        batch_id: Some(batch.id.clone()),
        items: u64::try_from(results.len()).unwrap_or(u64::MAX),
        bytes: moved_bytes,
        failed: u64::try_from(failed).unwrap_or(u64::MAX),
        outcome: match (moved_ids.len(), failed) {
            (_, 0) => "completed",
            (0, _) => "failed",
            _ => "partial",
        },
        subject: None,
        lines,
    });
    state.remove(&moved_ids);
    ApproveOutcome::Finished {
        batch_id: batch.id,
        finished_at_ms: finished,
        items: results,
    }
}
