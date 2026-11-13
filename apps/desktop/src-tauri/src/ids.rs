//! Opaque IDs for items the frontend refers to (review items, quarantine batches and items).

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A new ID: hex digits only, unique within this run and across runs (the time and process
/// come first), so it is also safe as a file name in the quarantine folder.
pub fn new_id(now_ms: u64) -> String {
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{now_ms:x}{:x}{count:x}", std::process::id())
}
