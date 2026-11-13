//! Scheduled scans (ADR-015): a read-only Cleanup scan Kleari runs on its own, daily or
//! weekly at a local hour, while it is running (in the menu bar). The findings replace the
//! review, as a scan you start does, and a notification says what was found; clicking it
//! opens the review. Nothing is moved: approval is still yours.
//!
//! A time missed while Kleari wasn't running or the Mac was asleep is caught up once, at the
//! next check. A schedule that was just turned on or changed waits for its next time instead
//! of scanning at once. A check waits while a scan or a quarantine move, restore or delete runs.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::cleanup::CleanupState;
use crate::cleanup_commands::{run_scan, ScanOrigin};
use crate::history::{unix_time_ms, History};
use crate::persistence::Database;
use crate::preferences::{self, Frequency, ScanSchedule};
use crate::quarantine::QuarantineState;

/// Where the schedule's own state is kept; the version changes if its shape does.
const KEY: &str = "scan_schedule.v1";
const DAY_SECS: u64 = 86_400;
const WEEK_SECS: u64 = 7 * DAY_SECS;
/// How often the schedule is checked. A scan starts within this long of its time.
const CHECK: Duration = Duration::from_secs(60);
/// Sent to every window when a scheduled scan has replaced the review.
pub const FINISHED_EVENT: &str = "scheduled-scan-finished";

/// The local wall-clock time: weekday (0 is Monday) and seconds since local midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalNow {
    pub weekday: u8,
    pub seconds: u64,
}

/// The local time at `unix_secs`, from the Mac's time zone.
#[cfg(unix)]
pub fn local_now(unix_secs: u64) -> Option<LocalNow> {
    let time = libc::time_t::try_from(unix_secs).ok()?;
    // SAFETY: an all-zero `tm` is a valid value; `localtime_r` fills it and touches nothing
    // else (unlike `localtime`, it uses no shared buffer).
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live locals for the duration of the call.
    let filled = unsafe { libc::localtime_r(&time, &mut tm) };
    if filled.is_null() {
        return None;
    }
    let weekday = u8::try_from((tm.tm_wday + 6) % 7).ok()?;
    let seconds = u64::try_from(tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec).ok()?;
    Some(LocalNow { weekday, seconds })
}

/// How long ago, in seconds, the schedule's latest time was (0 when it is exactly now);
/// `None` when it is off. Local wall-clock time, so on a day the clocks change it can be an
/// hour out.
pub fn since_due(now: LocalNow, schedule: &ScanSchedule) -> Option<u64> {
    let at = u64::from(schedule.hour) * 3600;
    match schedule.frequency {
        Frequency::Off => None,
        Frequency::Daily => Some((now.seconds + DAY_SECS - at) % DAY_SECS),
        Frequency::Weekly => {
            let days = (u64::from(now.weekday) + 7 - u64::from(schedule.weekday)) % 7;
            Some((days * DAY_SECS + now.seconds + WEEK_SECS - at) % WEEK_SECS)
        }
    }
}

/// When the schedule next runs, in Unix ms; `None` when off.
pub fn next_due_ms(now_ms: u64, now: LocalNow, schedule: &ScanSchedule) -> Option<u64> {
    let since = since_due(now, schedule)?;
    let period = match schedule.frequency {
        Frequency::Weekly => WEEK_SECS,
        _ => DAY_SECS,
    };
    Some(now_ms + (period - since) * 1000)
}

/// What the schedule has done, kept with the schedule it refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schedule: ScanSchedule,
    /// Times up to this one are dealt with: run, or passed before the schedule was set.
    pub settled_ms: u64,
    /// When this schedule last scanned.
    pub last_scan_ms: Option<u64>,
}

pub fn load(db: &Database) -> Option<Record> {
    let rows = db.settings().ok()?;
    let (_, text) = rows.into_iter().find(|(key, _)| key == KEY)?;
    serde_json::from_str(&text).ok()
}

fn save(db: &Database, record: &Record) -> bool {
    serde_json::to_string(record).is_ok_and(|text| db.set_setting(KEY, &text).is_ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to do yet.
    Wait,
    /// The schedule is new or changed: start counting from now, without scanning.
    Start,
    /// A time has passed since the last one dealt with: scan now.
    Scan,
}

pub fn decide(
    now_ms: u64,
    since_due_secs: Option<u64>,
    record: Option<&Record>,
    schedule: &ScanSchedule,
) -> Decision {
    let Some(since) = since_due_secs else {
        // Off (or the time unknown). Turning a schedule off is a change too: it is recorded,
        // so turning it back on later starts counting afresh instead of catching up.
        return match record {
            Some(record) if record.schedule != *schedule => Decision::Start,
            _ => Decision::Wait,
        };
    };
    match record {
        Some(record) if record.schedule == *schedule => {
            let due_ms = now_ms.saturating_sub(since * 1000);
            if due_ms > record.settled_ms {
                Decision::Scan
            } else {
                Decision::Wait
            }
        }
        _ => Decision::Start,
    }
}

/// When the schedule last scanned and next scans, for Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub next_ms: Option<u64>,
    pub last_scan_ms: Option<u64>,
}

pub fn status(db: &Database, schedule: &ScanSchedule) -> Status {
    let now_ms = unix_time_ms();
    let next_ms = local_now(now_ms / 1000).and_then(|now| next_due_ms(now_ms, now, schedule));
    let last_scan_ms = load(db)
        .filter(|record| record.schedule == *schedule)
        .and_then(|record| record.last_scan_ms);
    Status {
        next_ms,
        last_scan_ms,
    }
}

/// Checks the schedule once, scanning if a time has passed.
fn check(app: &AppHandle) {
    let history = app.state::<History>();
    let loaded = preferences::load(&history.db);
    // Unreadable settings: the saved schedule isn't known, so nothing runs.
    if loaded.source == "unavailable" {
        return;
    }
    let schedule = loaded.preferences.scan_schedule;
    let now_ms = unix_time_ms();
    let since = local_now(now_ms / 1000).and_then(|now| since_due(now, &schedule));
    let record = load(&history.db);
    match decide(now_ms, since, record.as_ref(), &schedule) {
        Decision::Wait => {}
        Decision::Start => {
            // If this can't be saved, the next check tries again; nothing scans meanwhile.
            let _ = save(
                &history.db,
                &Record {
                    schedule,
                    settled_ms: now_ms,
                    last_scan_ms: None,
                },
            );
        }
        Decision::Scan => {
            // Not while a quarantine move, restore or delete runs (the guard is dropped at
            // once: scanning only reads), nor during another scan. Tried again next check.
            if app.state::<QuarantineState>().begin().is_err() {
                return;
            }
            let cleanup = app.state::<CleanupState>();
            let Ok(cancel) = cleanup.begin_scan() else {
                return;
            };
            // Recorded first, so a failing scan isn't retried every minute until its next time.
            let recorded = save(
                &history.db,
                &Record {
                    schedule,
                    settled_ms: now_ms,
                    last_scan_ms: Some(now_ms),
                },
            );
            if recorded {
                let result = run_scan(
                    app,
                    schedule.mode,
                    loaded.preferences.scan_defaults,
                    &cancel,
                    ScanOrigin::Schedule,
                    |_| {},
                );
                if result.is_ok() {
                    // Open windows read the review again; one that missed this reads it on its
                    // next visit to Cleanup.
                    let _ = app.emit(FINISHED_EVENT, ());
                }
            }
            cleanup.end_scan();
        }
    }
}

/// Starts checking the schedule every minute, for as long as Kleari runs.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(CHECK);
        check(&app);
    });
}
