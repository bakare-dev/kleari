//! Kleari's own log, on this Mac only: `~/Library/Logs/Kleari/kleari.log` (owner-only), for
//! "Report a problem". It records what Kleari did and what went wrong (the Mouse event tap
//! starting and stopping, shortcut recording, macOS's system shortcuts turned off and back on,
//! the keyboard lock, failed window actions, the bundle-identifier move, errors commands
//! returned, panics), never what it saw: every message is fixed text written in the code
//! (`&'static str`), with at most a number (an error code, a count, seconds). No key, file
//! name, path, window title or address can reach it. A panic's message is the one exception
//! written at run time, so anything in it that looks like a path is replaced.
//!
//! Each line is written as it happens (events are rare: nothing is logged per mouse or key
//! event). At 1 MB the file becomes `kleari.1.log`, replacing the one before, so the log
//! never holds more than about 2 MB. Nothing is sent anywhere.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "kleari.log";
const ROLLED_NAME: &str = "kleari.1.log";
/// Past this, the file is rolled over.
const MAX_BYTES: u64 = 1024 * 1024;
/// A panic message is cut to this many characters.
const MAX_PANIC_TEXT: usize = 300;

struct Writer {
    folder: PathBuf,
    file: File,
    size: u64,
}

static LOG: Mutex<Option<Writer>> = Mutex::new(None);

/// The log's folder for `home`.
pub fn folder(home: &Path) -> PathBuf {
    home.join("Library/Logs/Kleari")
}

/// The log file for `home`.
pub fn file(home: &Path) -> PathBuf {
    folder(home).join(FILE_NAME)
}

fn open(folder: &Path) -> Option<Writer> {
    // Owner-only; an existing folder or file keeps the permissions it has.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .ok()?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(folder.join(FILE_NAME))
        .ok()?;
    let size = file.metadata().ok()?.len();
    Some(Writer {
        folder: folder.to_path_buf(),
        file,
        size,
    })
}

/// Opens the log in `folder` and records panics in it (after macOS's usual report). Without
/// a log (the folder can't be made), Kleari runs the same and records nothing.
pub fn start(folder: &Path, version: &str) {
    let writer = open(folder);
    let opened = writer.is_some();
    *LOG.lock().unwrap_or_else(PoisonError::into_inner) = writer;
    if !opened {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        write_line("panic", &format!("{place} {}", redact(&message)));
        previous(info);
    }));
    write_line(
        "kleari",
        &format!("started, version {}", printable(version)),
    );
}

/// Records `what` (fixed text) under `area`.
pub fn note(area: &'static str, what: &'static str) {
    write_line(area, what);
}

/// Records `what` with a number: an error code, a count or a length in seconds.
pub fn note_number(area: &'static str, what: &'static str, number: i64) {
    write_line(area, &format!("{what} ({number})"));
}

/// Records `what` about `detail` (both fixed text, such as an Accessibility attribute's
/// name) with a number.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn note_detail(area: &'static str, what: &'static str, detail: &'static str, number: i64) {
    write_line(area, &format!("{what}: {detail} ({number})"));
}

/// Records an error a command returned to the page, by its code's name.
pub fn command_error(code: crate::commands::CommandErrorCode) {
    write_line("command", &format!("returned error {code:?}"));
}

fn write_line(area: &str, text: &str) {
    let line = format!("{} [{area}] {text}\n", timestamp(now_ms()));
    let mut log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(writer) = log.as_mut() else {
        return;
    };
    if writer.size + line.len() as u64 > MAX_BYTES {
        let folder = writer.folder.clone();
        // The rolled file replaces the one before; if the rename fails the log is reopened
        // (and keeps growing past the limit until a rename works).
        let _ = std::fs::rename(folder.join(FILE_NAME), folder.join(ROLLED_NAME));
        *log = open(&folder);
    }
    if let Some(writer) = log.as_mut() {
        // A line that can't be written is lost; logging never stops what is being logged.
        if writer.file.write_all(line.as_bytes()).is_ok() {
            writer.size += line.len() as u64;
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// `ms` since 1970 as an ISO 8601 time in UTC, to the millisecond.
fn timestamp(ms: u64) -> String {
    let (days, rest) = (ms / 86_400_000, ms % 86_400_000);
    let (year, month, day) = civil(i64::try_from(days).unwrap_or(0));
    let (hour, minute, second, milli) = (
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z")
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `text` on one line, cut to `MAX_PANIC_TEXT` characters, with each word that could be a
/// path or address (it has a `/` or starts with `~`) replaced by `<path>`.
fn redact(text: &str) -> String {
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|word| {
            if word.contains('/') || word.starts_with('~') {
                "<path>"
            } else {
                word
            }
        })
        .collect();
    printable(&words.join(" "))
        .chars()
        .take(MAX_PANIC_TEXT)
        .collect()
}

/// `text` without control characters, so a line stays one line.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_utc_iso_8601() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(timestamp(1_795_036_435_946), "2026-11-18T21:13:55.946Z");
        assert_eq!(timestamp(4_107_542_399_999), "2100-02-28T23:59:59.999Z");
    }

    #[test]
    fn a_panic_message_loses_anything_like_a_path() {
        assert_eq!(
            redact("couldn't open /Users/someone/Secret file.txt: denied"),
            "couldn't open <path> file.txt: denied"
        );
        assert_eq!(redact("in ~/Documents now"), "in <path> now");
        assert_eq!(redact("line one\nline two"), "line one line two");
        assert_eq!(redact(&"x".repeat(1000)).len(), MAX_PANIC_TEXT);
    }

    #[test]
    fn the_log_rolls_over_at_its_limit_and_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("kleari-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut writer = open(&folder).expect("open");
        let mode = std::fs::metadata(folder.join(FILE_NAME))
            .expect("file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        writer.size = MAX_BYTES;
        *LOG.lock().unwrap_or_else(PoisonError::into_inner) = Some(writer);
        note("test", "after the limit");
        let rolled = folder.join(ROLLED_NAME);
        assert!(rolled.is_file());
        let fresh = std::fs::read_to_string(folder.join(FILE_NAME)).expect("fresh");
        // Other tests running now may add their own lines; this one starts the new file.
        assert!(fresh.contains("[test] after the limit\n"));
        assert!((fresh.len() as u64) < MAX_BYTES);
        *LOG.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let _ = std::fs::remove_dir_all(&folder);
    }
}
