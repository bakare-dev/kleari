//! Memory and CPU time for processes macOS doesn't report to Kleari directly (those of root
//! and other accounts), read from Apple's own `/bin/ps` (ADR-010). `ps` is installed setuid
//! root on the sealed system volume, so it can read what an unprivileged app can't, without
//! a prompt or an administrator password.
//!
//! The boundary is narrow: an absolute path, fixed arguments, no shell, an empty environment
//! apart from `LC_ALL=C`, no input, a time limit, bounded output, and output parsed as
//! untrusted text. Nothing is written and nothing is stored.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PS: &str = "/bin/ps";
/// Process ID, owner UID, resident memory in KiB, and cumulative CPU time, without headers.
const ARGUMENTS: [&str; 3] = ["-ax", "-o", "pid=,uid=,rss=,time="];
const TIME_LIMIT: Duration = Duration::from_millis(1500);
const MAX_OUTPUT: u64 = 1024 * 1024;
const MAX_LINES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsReading {
    pub uid: u32,
    pub memory_bytes: u64,
    pub cpu_time_ms: u64,
}

/// Parses `ps`'s CPU time: `[dd-][hh:]mm:ss.cc`, where minutes can exceed 59.
fn parse_time(text: &str) -> Option<u64> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0, *minutes, *seconds),
        [hours, minutes, seconds] => (hours.parse::<u64>().ok()?, *minutes, *seconds),
        _ => return None,
    };
    let minutes = minutes.parse::<u64>().ok()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let whole = whole.parse::<u64>().ok()?;
    if whole >= 60 || fraction.len() > 3 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let fraction_ms = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<3}").parse::<u64>().ok()?
    };
    let total_minutes = days
        .checked_mul(24 * 60)?
        .checked_add(hours.checked_mul(60)?)?
        .checked_add(minutes)?;
    total_minutes
        .checked_mul(60_000)?
        .checked_add(whole * 1000)?
        .checked_add(fraction_ms)
}

/// Parses `pid uid rss time` lines. Malformed lines are skipped, never guessed.
pub fn parse(output: &str) -> HashMap<u32, PsReading> {
    output
        .lines()
        .take(MAX_LINES)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let uid = fields.next()?.parse::<u32>().ok()?;
            let rss_kib = fields.next()?.parse::<u64>().ok()?;
            let cpu_time_ms = parse_time(fields.next()?)?;
            if fields.next().is_some() {
                return None;
            }
            Some((
                pid,
                PsReading {
                    uid,
                    memory_bytes: rss_kib.checked_mul(1024)?,
                    cpu_time_ms,
                },
            ))
        })
        .collect()
}

/// Runs `/bin/ps` once. `None` if it can't run, exits with an error or takes too long.
pub fn read() -> Option<HashMap<u32, PsReading>> {
    let mut child = Command::new(PS)
        .args(ARGUMENTS)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    // Read on another thread so a full pipe can't stall `ps` while this one waits.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout
            .take(MAX_OUTPUT)
            .read_to_string(&mut text)
            .ok()
            .map(|_| text)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < TIME_LIMIT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                // Too slow or unwaitable: stop it, and report no reading. The kill result
                // doesn't matter, since the process is reaped either way.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = reader.join().ok().flatten()?;
    status.filter(std::process::ExitStatus::success)?;
    Some(parse(&text))
}
