//! The Activity page's log, read from the history: what Kleari scanned, sent to review,
//! moved, restored, deleted, which folder access was answered and which apps were asked to
//! quit. Items are named with their original folder shown as `~/…` (outside the home folder,
//! only the folder's own name); a full path is never sent. Events older than the retention preference are deleted at launch and when it changes.

use std::path::Path;

use serde::Serialize;

use crate::persistence::{Database, EventKind, StoredEvent, Unavailable};
use crate::system_monitor::display_path;

/// The most events listed at once; older ones stay in the history until pruned.
const MAX_EVENTS: u32 = 500;
/// The most item lines sent for one event; the rest are counted.
const MAX_LINES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ActivityList {
    #[serde(rename_all = "camelCase")]
    Available {
        retention_days: u32,
        /// Newest first.
        events: Vec<ActivityEvent>,
        /// More events exist in the kept window than were listed.
        truncated: bool,
    },
    /// The history can't be read; nothing is shown as having happened.
    Unavailable { reason: Unavailable },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: String,
    pub at_ms: u64,
    pub kind: EventKind,
    pub subject: Option<String>,
    pub items: u64,
    pub bytes: u64,
    pub failed: u64,
    pub outcome: String,
    pub lines: Vec<ActivityLine>,
    /// Item lines not sent (beyond `MAX_LINES`).
    pub more_lines: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLine {
    pub name: String,
    pub category: String,
    /// The folder it came from, shown as `~/…`.
    pub folder: String,
    pub bytes: u64,
    pub outcome: String,
}

/// An item's folder for the log: `~/…` inside the home folder; elsewhere only its last
/// component (`…/DerivedData`), so the log never shows a full path.
fn folder_of(original: &Path, home: Option<&Path>) -> String {
    let Some(parent) = original.parent() else {
        return String::new();
    };
    if home.is_some_and(|home| parent.starts_with(home)) {
        return display_path(parent, home);
    }
    parent
        .file_name()
        .map_or_else(String::new, |name| format!("…/{}", name.to_string_lossy()))
}

fn view(event: StoredEvent, home: Option<&Path>) -> ActivityEvent {
    let more_lines = u64::try_from(event.lines.len().saturating_sub(MAX_LINES)).unwrap_or(u64::MAX);
    ActivityEvent {
        id: event.id.to_string(),
        at_ms: event.at_ms,
        kind: event.kind,
        subject: event.subject,
        items: event.items,
        bytes: event.bytes,
        failed: event.failed,
        outcome: event.outcome,
        lines: event
            .lines
            .into_iter()
            .take(MAX_LINES)
            .map(|line| ActivityLine {
                folder: folder_of(&line.original, home),
                name: line.name,
                category: line.category,
                bytes: line.bytes,
                outcome: line.outcome,
            })
            .collect(),
        more_lines,
    }
}

/// The events kept since `since_ms`, newest first.
pub fn list(
    db: &Database,
    home: Option<&Path>,
    since_ms: u64,
    retention_days: u32,
) -> ActivityList {
    // One more than shown tells whether there are more.
    match db.events_since(since_ms, MAX_EVENTS + 1) {
        Ok(mut events) => {
            let truncated = events.len() > MAX_EVENTS as usize;
            events.truncate(MAX_EVENTS as usize);
            ActivityList::Available {
                retention_days,
                events: events.into_iter().map(|event| view(event, home)).collect(),
                truncated,
            }
        }
        Err(reason) => ActivityList::Unavailable { reason },
    }
}
