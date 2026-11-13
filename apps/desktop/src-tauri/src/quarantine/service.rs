//! Listing, restoring and permanently deleting held items, with each outcome recorded in the
//! history. Paths stay in Rust: the frontend gets item IDs, names and `~/…` locations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde::Serialize;

use super::{Failure, Identity, Presence, Store};
use crate::persistence::{Database, Event, EventKind, ItemRecord, ItemState, Unavailable};
use crate::system_monitor::display_path;

/// Where items may go back to: the home folder, the user's temporary folder, and folders
/// chosen in the macOS folder panel this session. A stored original path is checked against
/// them (`policy::restorable`) before it is used. Nothing read from the history widens it.
pub struct Scope<'a> {
    pub home: &'a Path,
    pub temp: Option<&'a Path>,
    /// Folders the user chose in the folder panel since Kleari opened.
    pub added: &'a [PathBuf],
    /// Folders excluded in Settings now: what is at an original place in or holding one isn't
    /// looked at. An explicit restore may still put the item back there.
    pub excluded: &'a [PathBuf],
}

impl Scope<'_> {
    /// Whether the record's original path is a place its category could have come from. A
    /// path that isn't (a tampered or unexpected record) is never restored to or revealed.
    pub fn allows(&self, item: &ItemRecord) -> bool {
        crate::cleanup::policy::restorable(
            &item.category,
            &item.original,
            self.added,
            self.home,
            self.temp,
        )
    }

    /// Whether the record's original place may be looked at: allowed, and not in or holding a
    /// folder excluded now.
    pub fn may_probe(&self, item: &ItemRecord) -> bool {
        self.allows(item) && !crate::preferences::touches_excluded(&item.original, self.excluded)
    }

    /// The added folder a build folder was approved under, when it would be allowed once
    /// the user chooses that folder again. The recorded folder is only a hint for what to
    /// ask for: the user's choice in the folder panel is what allows the restore.
    pub fn needed_folder<'i>(&self, item: &'i ItemRecord) -> Option<&'i Path> {
        let root = item.root.as_deref()?;
        let chosen = Scope {
            added: &[root.to_path_buf()],
            ..*self
        };
        (!self.allows(item) && chosen.allows(item)).then_some(root)
    }
}

/// One file-changing operation at a time (a cleanup, a restore or a permanent delete), the
/// stop request for a running delete, and the folders chosen in Quarantine this session to
/// restore into (a restore-only grant: they are never scanned).
#[derive(Default)]
pub struct QuarantineState {
    busy: Mutex<Option<Holder>>,
    /// Signalled when the holder leaves, for those waiting behind a settle.
    released: Condvar,
    stop: Arc<AtomicBool>,
    restore_folders: Mutex<Vec<PathBuf>>,
}

/// What holds the lock: a user's operation (moves, restores, deletes), or a short settle of
/// unfinished records (`reconcile`) that anything else waits behind instead of failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    Operation,
    Settle,
}

/// Held while an operation or a settle runs; released on drop.
pub struct OperationGuard<'a> {
    state: &'a QuarantineState,
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.state.busy) = None;
        self.state.released.notify_all();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // The holder is replaced whole, and the folder list is only ever pushed to.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

impl QuarantineState {
    /// Allows restores into `folder` (chosen in the folder panel) until Kleari quits.
    pub fn grant_restore_folder(&self, folder: PathBuf) {
        let mut folders = lock(&self.restore_folders);
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }

    pub fn restore_folders(&self) -> Vec<PathBuf> {
        lock(&self.restore_folders).clone()
    }

    /// Withdraws a restore-only grant; `false` if there wasn't one.
    pub fn revoke_restore_folder(&self, folder: &Path) -> bool {
        let mut folders = lock(&self.restore_folders);
        let before = folders.len();
        folders.retain(|entry| entry != folder);
        folders.len() != before
    }

    /// Starts an operation, or `Busy` if another operation is running. A settle in progress
    /// is waited for: it is short and is never a reason to refuse.
    pub fn begin(&self) -> Result<OperationGuard<'_>, Busy> {
        self.hold(Holder::Operation)
    }

    /// Starts settling unfinished records, or `Busy` if an operation is running (it is
    /// settled on a later read). Waits for another settle to finish.
    pub fn begin_settle(&self) -> Result<OperationGuard<'_>, Busy> {
        self.hold(Holder::Settle)
    }

    fn hold(&self, holder: Holder) -> Result<OperationGuard<'_>, Busy> {
        let mut busy = lock(&self.busy);
        while *busy == Some(Holder::Settle) {
            busy = self
                .released
                .wait(busy)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if busy.is_some() {
            return Err(Busy);
        }
        *busy = Some(holder);
        if holder == Holder::Operation {
            self.stop.store(false, Ordering::Relaxed);
        }
        Ok(OperationGuard { state: self })
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Asks a running permanent delete to stop after the current item.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Whether a held item can go back now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Restorable,
    /// Something exists at the original place; a restore won't overwrite it.
    Conflict,
    /// The folder it came from is gone or can't be opened safely.
    FolderMissing,
    /// It isn't in the quarantine folder (removed outside Kleari).
    Missing,
    /// A move or restore whose outcome isn't confirmed yet; it is resolved from the folders
    /// at the next launch. Nothing can be done with it until then.
    Pending,
    /// Kleari couldn't verify it: the quarantine folder can't be inspected, or something
    /// else is under its name. Nothing can be done with it until it can be verified.
    Unverified,
    /// It came from a folder added outside the home folder; choose that folder in the
    /// folder panel to restore it. It can still be deleted.
    NeedsFolder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub category: String,
    /// The folder it came from, shown as `~/…`.
    pub from: String,
    pub bytes: u64,
    pub status: ItemStatus,
    /// Choosing the folder it came from in the folder panel would let Kleari restore it
    /// (`needsFolder`), or settle an interrupted restore (`pending`).
    pub choose_folder: bool,
    /// The folder the history records it was added from, shown as `~/…`: only a hint, since
    /// the history can be edited, and left out when it is too broad to be a project folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needed_folder: Option<String>,
}

/// A recorded added folder too broad to show as where an item came from: the disk's
/// top-level folders (`/Users`, `/Volumes`, `/private`), the home folder or anything above it,
/// or a volume's own folder (`/Volumes/Disk`). Decided from the path alone: the path comes
/// from the history, so listing never touches it (it could be on a network volume).
pub(super) fn broad(root: &Path, home: Option<&Path>) -> bool {
    let parts = root.components().count();
    parts <= 2
        || (root.starts_with("/Volumes") && parts <= 3)
        || home.is_some_and(|home| home.starts_with(root))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchView {
    pub id: String,
    pub created_ms: u64,
    pub origin: String,
    pub items: Vec<ItemView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum QuarantineList {
    Available {
        /// The quarantine folder, shown as `~/…`.
        folder: String,
        /// Newest first.
        batches: Vec<BatchView>,
    },
    /// The history can't be read, so nothing can be restored or deleted from here; the
    /// items themselves are untouched in the folder.
    Unavailable { reason: Unavailable, folder: String },
}

fn status_of(store: &Store, item: &ItemRecord, scope: Option<&Scope<'_>>) -> ItemStatus {
    let identity = Identity {
        dev: item.dev,
        ino: item.ino,
        is_dir: item.is_dir,
    };
    // Restore and delete act only on held records, so anything unresolved is shown as such,
    // whatever the folder shows.
    if item.state != ItemState::Held {
        return ItemStatus::Pending;
    }
    // A stored original path that isn't a place its category could have come from can't be
    // trusted, so the item can't be verified (nor restored) from this record, unless the user
    // can allow it by choosing its added folder again.
    let Some(scope) = scope else {
        return ItemStatus::Unverified;
    };
    if !scope.allows(item) {
        return if scope.needed_folder(item).is_some() {
            ItemStatus::NeedsFolder
        } else {
            ItemStatus::Unverified
        };
    }
    match store.presence(&item.batch_id, &item.id, identity) {
        Presence::Held => {}
        Presence::Absent => return ItemStatus::Missing,
        Presence::Unknown => return ItemStatus::Unverified,
    }
    // Its original place is in a folder excluded since: not looked at. It can still be
    // restored; the restore never overwrites, and reports a conflict if there is one.
    if !scope.may_probe(item) {
        return ItemStatus::Restorable;
    }
    match Store::original_taken(&item.original) {
        Some(true) => ItemStatus::Conflict,
        Some(false) => ItemStatus::Restorable,
        None => ItemStatus::FolderMissing,
    }
}

fn from_of(item: &ItemRecord, home: Option<&Path>) -> String {
    item.original
        .parent()
        .map_or_else(String::new, |parent| display_path(parent, home))
}

/// Resolves records a previous run left `moving` or `restoring` from what the folders prove.
/// What the quarantine folder proves settles any record; a conclusion drawn from the original
/// place is only drawn when that place is in `scope`, otherwise the record stays unresolved.
pub fn reconcile(
    db: &Database,
    store: &Store,
    scope: Option<&Scope<'_>>,
    at_ms: u64,
) -> Result<usize, Unavailable> {
    db.reconcile(
        |item| store.resolve_interrupted(item, scope.is_some_and(|scope| scope.may_probe(item))),
        at_ms,
    )
}

/// Everything held, grouped by batch, newest batch first. Without a scope (no home folder)
/// nothing can be verified.
pub fn list(db: &Database, store: &Store, scope: Option<&Scope<'_>>) -> QuarantineList {
    let home = scope.map(|scope| scope.home);
    let folder = display_path(&store.path(), home);
    let (items, batches) = match (db.held_items(), db.batches()) {
        (Ok(items), Ok(batches)) => (items, batches),
        (Err(reason), _) | (_, Err(reason)) => {
            return QuarantineList::Unavailable { reason, folder }
        }
    };
    let mut by_batch: HashMap<&str, Vec<ItemView>> = HashMap::new();
    for item in &items {
        // A held item can be restored once its folder is chosen; an interrupted restore can
        // then be settled from its original place. A `moving` record is settled from the
        // quarantine folder alone, so choosing a folder wouldn't help it.
        let needed = scope
            .filter(|_| matches!(item.state, ItemState::Held | ItemState::Restoring))
            .and_then(|scope| scope.needed_folder(item));
        by_batch
            .entry(item.batch_id.as_str())
            .or_default()
            .push(ItemView {
                id: item.id.clone(),
                name: item.name.clone(),
                category: item.category.clone(),
                from: from_of(item, home),
                bytes: item.bytes,
                status: status_of(store, item, scope),
                choose_folder: needed.is_some(),
                needed_folder: needed
                    .filter(|root| !broad(root, home))
                    .map(|root| display_path(root, home)),
            });
    }
    let mut views: Vec<BatchView> = batches
        .into_iter()
        .filter_map(|batch| {
            let items = by_batch.remove(batch.id.as_str())?;
            Some(BatchView {
                id: batch.id,
                created_ms: batch.created_ms,
                origin: batch.origin,
                items,
            })
        })
        .collect();
    views.reverse();
    QuarantineList::Available {
        folder,
        batches: views,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreOutcome {
    Restored,
    /// Something exists at the original place; the item stays in quarantine.
    Conflict,
    FolderMissing,
    /// It isn't in the quarantine folder, or it is no longer held.
    Missing,
    NeedsAccess,
    /// Whether the item was restored couldn't be confirmed either way (what arrived couldn't
    /// be checked, or a rollback couldn't be verified); the record stays `restoring` and the
    /// next launches try to resolve it.
    Unconfirmed,
    /// The record's original path isn't a place an item of its category comes from, so
    /// Kleari won't move anything there. Nothing changed.
    NotAllowed,
    /// It came from a folder added outside the home folder that hasn't been chosen in the
    /// folder panel this session. Nothing changed.
    NeedsFolder,
    Failed,
}

impl RestoreOutcome {
    /// As recorded in the history, matching its serialized name.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Restored => "restored",
            Self::Conflict => "conflict",
            Self::FolderMissing => "folderMissing",
            Self::Missing => "missing",
            Self::NeedsAccess => "needsAccess",
            Self::Unconfirmed => "unconfirmed",
            Self::NotAllowed => "notAllowed",
            Self::NeedsFolder => "needsFolder",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub id: String,
    pub outcome: RestoreOutcome,
}

fn held(db: &Database, id: &str) -> Result<Option<ItemRecord>, Unavailable> {
    Ok(db.item(id)?.filter(|item| item.state == ItemState::Held))
}

/// Restores one held item. The record says `restoring` before anything moves, so an outcome
/// the history couldn't record is resolved at the next launch rather than lost.
fn restore_one(
    db: &Database,
    store: &Store,
    scope: &Scope<'_>,
    id: &str,
    now_ms: u64,
) -> RestoreOutcome {
    let item = match held(db, id) {
        Ok(Some(item)) => item,
        Ok(None) => return RestoreOutcome::Missing,
        Err(_) => return RestoreOutcome::Failed,
    };
    // The destination comes from the stored record: it must be a place an item of its
    // category could have come from, or nothing moves.
    if !scope.allows(&item) {
        return if scope.needed_folder(&item).is_some() {
            RestoreOutcome::NeedsFolder
        } else {
            RestoreOutcome::NotAllowed
        };
    }
    // Without this record nothing moves: a restore must always be reconcilable.
    if db
        .set_state(&item.id, ItemState::Restoring, now_ms)
        .is_err()
    {
        return RestoreOutcome::Failed;
    }
    let identity = Identity {
        dev: item.dev,
        ino: item.ino,
        is_dir: item.is_dir,
    };
    match store.restore(&item.batch_id, &item.id, identity, &item.original) {
        Ok(()) => {
            // It is back. If this write fails the record stays `restoring` and the next launch
            // finds the item at its original place and records the restore.
            let _ = db.set_state(&item.id, ItemState::Restored, now_ms);
            RestoreOutcome::Restored
        }
        // The rename happened but couldn't be confirmed: the record stays `restoring` for the
        // next launch to resolve from the folders, and nothing is claimed either way.
        Err(Failure::Unconfirmed) => RestoreOutcome::Unconfirmed,
        Err(failure) => {
            // Nothing moved (or a replacement was moved straight back). If this write fails
            // the record stays `restoring` and the next launch finds the item still held.
            let _ = db.set_state(&item.id, ItemState::Held, now_ms);
            match failure {
                Failure::Exists => RestoreOutcome::Conflict,
                Failure::ParentUnavailable => RestoreOutcome::FolderMissing,
                Failure::Missing | Failure::Changed => RestoreOutcome::Missing,
                Failure::NeedsAccess => RestoreOutcome::NeedsAccess,
                _ => RestoreOutcome::Failed,
            }
        }
    }
}

/// Puts each item back where it came from. Restores never overwrite; each item's outcome is
/// returned and the operation is recorded. `Err` only when the history is unusable before
/// anything starts; after that, every item's real outcome is returned, even if a history
/// write fails.
pub fn restore_items(
    db: &Database,
    store: &Store,
    scope: &Scope<'_>,
    ids: &[String],
    now_ms: u64,
) -> Result<Vec<RestoreResult>, Unavailable> {
    if let Some(reason) = db.unavailable() {
        return Err(reason);
    }
    let mut results = Vec::with_capacity(ids.len());
    let mut bytes = 0_u64;
    for id in ids {
        let outcome = restore_one(db, store, scope, id, now_ms);
        if outcome == RestoreOutcome::Restored {
            if let Ok(Some(item)) = db.item(id) {
                bytes = bytes.saturating_add(item.bytes);
            }
        }
        results.push(RestoreResult {
            id: id.clone(),
            outcome,
        });
    }
    let restored = results
        .iter()
        .filter(|result| result.outcome == RestoreOutcome::Restored)
        .count();
    let failed = results.len() - restored;
    // The outcomes already happened; a failed event write mustn't hide them.
    let _ = db.record_event(&Event {
        at_ms: now_ms,
        kind: EventKind::Restore,
        batch_id: None,
        items: u64::try_from(results.len()).unwrap_or(u64::MAX),
        bytes,
        failed: u64::try_from(failed).unwrap_or(u64::MAX),
        outcome: match (restored, failed) {
            (_, 0) => "completed",
            (0, _) => "failed",
            _ => "partial",
        },
        subject: None,
        lines: results
            .iter()
            .map(|result| (result.id.clone(), result.outcome.as_str()))
            .collect(),
    });
    Ok(results)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeleteState {
    Deleting,
    Deleted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProgress {
    pub id: String,
    pub state: DeleteState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSummary {
    pub deleted: Vec<String>,
    /// Not deleted because of an error; still in quarantine (a folder may be partly deleted).
    pub failed: Vec<String>,
    /// Not started because the user stopped the delete; still in quarantine.
    pub kept: Vec<String>,
    pub stopped: bool,
}

/// Permanently deletes each held item, one at a time, stopping before the next item once
/// `stop` is set. Reports each item's progress and records the operation. `Err` only when the
/// history is unusable before anything starts; after that, what happened is always returned.
pub fn delete_items(
    db: &Database,
    store: &Store,
    ids: &[String],
    stop: &AtomicBool,
    now_ms: impl Fn() -> u64,
    mut progress: impl FnMut(DeleteProgress),
) -> Result<DeleteSummary, Unavailable> {
    if let Some(reason) = db.unavailable() {
        return Err(reason);
    }
    let mut summary = DeleteSummary {
        deleted: Vec::new(),
        failed: Vec::new(),
        kept: Vec::new(),
        stopped: false,
    };
    let mut bytes = 0_u64;
    for (index, id) in ids.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            summary.stopped = true;
            summary.kept.extend(ids[index..].iter().cloned());
            break;
        }
        progress(DeleteProgress {
            id: id.clone(),
            state: DeleteState::Deleting,
        });
        let deleted = match held(db, id) {
            Ok(Some(item)) => {
                let identity = Identity {
                    dev: item.dev,
                    ino: item.ino,
                    is_dir: item.is_dir,
                };
                match store.delete(&item.batch_id, &item.id, identity) {
                    // Already gone from the folder: only the record is left to close.
                    Ok(()) | Err(Failure::Missing) => {
                        // It is gone either way. If the record can't be closed it stays held,
                        // is listed as missing from the folder, and a later delete closes it.
                        let _ = db.set_state(&item.id, ItemState::Deleted, now_ms());
                        bytes = bytes.saturating_add(item.bytes);
                        true
                    }
                    Err(_) => false,
                }
            }
            Ok(None) | Err(_) => false,
        };
        progress(DeleteProgress {
            id: id.clone(),
            state: if deleted {
                DeleteState::Deleted
            } else {
                DeleteState::Failed
            },
        });
        if deleted {
            summary.deleted.push(id.clone());
        } else {
            summary.failed.push(id.clone());
        }
    }
    // The deletes already happened; a failed event write mustn't hide them.
    let _ = db.record_event(&Event {
        at_ms: now_ms(),
        kind: EventKind::Delete,
        batch_id: None,
        items: u64::try_from(ids.len()).unwrap_or(u64::MAX),
        bytes,
        // Everything not deleted: failures and the items a stop left untouched, so
        // `items - failed` is always what was deleted.
        failed: u64::try_from(ids.len().saturating_sub(summary.deleted.len())).unwrap_or(u64::MAX),
        outcome: if summary.stopped {
            "stopped"
        } else if summary.failed.is_empty() {
            "completed"
        } else if summary.deleted.is_empty() {
            "failed"
        } else {
            "partial"
        },
        subject: None,
        lines: summary
            .deleted
            .iter()
            .map(|id| (id.clone(), "deleted"))
            .chain(summary.failed.iter().map(|id| (id.clone(), "failed")))
            .chain(summary.kept.iter().map(|id| (id.clone(), "kept")))
            .collect(),
    });
    Ok(summary)
}
