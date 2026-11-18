//! Battery health as macOS itself states it: the "Maximum Capacity" and "Condition" shown in
//! System Information and System Settings › Battery. macOS doesn't publish its own figure in
//! the IORegistry, and the raw capacities there (`NominalChargeCapacity`, `AppleRawMaxCapacity`)
//! can differ from it by several percent, so it is read from Apple's `system_profiler`.
//!
//! The boundary is the one `ps` has (ADR-010): an absolute path, fixed arguments, no shell,
//! an empty environment apart from `LC_ALL=C`, no input, a time limit, bounded output, and the
//! output parsed as untrusted JSON. Nothing is written and nothing is stored.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SYSTEM_PROFILER: &str = "/usr/sbin/system_profiler";
/// Power data only, in JSON, at the cheapest detail level (about 80 ms).
const ARGUMENTS: [&str; 4] = ["SPPowerDataType", "-json", "-detailLevel", "mini"];
const TIME_LIMIT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: u64 = 256 * 1024;

/// What macOS says about the battery's health.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportedHealth {
    /// "Maximum Capacity", as a percent.
    pub maximum_capacity_percent: Option<f64>,
    /// "Condition" / battery health: `Some(true)` when macOS recommends service.
    pub service_recommended: Option<bool>,
}

/// Reads it once. `None` if `system_profiler` can't run, fails, takes too long, or reports
/// no battery.
pub fn read() -> Option<ReportedHealth> {
    let mut child = Command::new(SYSTEM_PROFILER)
        .args(ARGUMENTS)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    // Read on another thread so a full pipe can't stall it while this one waits.
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
                // Too slow or unwaitable: stop it and report no reading; it is reaped either way.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = reader.join().ok().flatten()?;
    status.filter(std::process::ExitStatus::success)?;
    parse(&text)
}

/// Finds the battery health in `system_profiler`'s JSON.
pub fn parse(text: &str) -> Option<ReportedHealth> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let health = json
        .get("SPPowerDataType")?
        .as_array()?
        .iter()
        .find_map(|entry| entry.get("sppower_battery_health_info"))?;
    let maximum_capacity_percent = health
        .get("sppower_battery_health_maximum_capacity")
        .and_then(serde_json::Value::as_str)
        .and_then(|text| text.trim().trim_end_matches('%').trim().parse::<f64>().ok())
        .filter(|percent| percent.is_finite() && (1.0..=150.0).contains(percent))
        .map(|percent| percent.min(100.0));
    let service_recommended = health
        .get("sppower_battery_health")
        .and_then(serde_json::Value::as_str)
        .map(|condition| {
            let condition = condition.to_ascii_lowercase();
            !(condition == "good" || condition == "normal")
        });
    (maximum_capacity_percent.is_some() || service_recommended.is_some()).then_some(
        ReportedHealth {
            maximum_capacity_percent,
            service_recommended,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_figure_system_information_shows() {
        let text = r#"{"SPPowerDataType":[{"_name":"spbattery_information","sppower_battery_health_info":{"sppower_battery_cycle_count":312,"sppower_battery_health":"Good","sppower_battery_health_maximum_capacity":"96%"}},{"_name":"sppower_information"}]}"#;
        let health = parse(text).expect("health");
        assert_eq!(health.maximum_capacity_percent, Some(96.0));
        assert_eq!(health.service_recommended, Some(false));
    }

    #[test]
    fn service_recommended_and_odd_values() {
        let text = r#"{"SPPowerDataType":[{"sppower_battery_health_info":{"sppower_battery_health":"Service Recommended","sppower_battery_health_maximum_capacity":"lots"}}]}"#;
        let health = parse(text).expect("health");
        assert_eq!(health.maximum_capacity_percent, None);
        assert_eq!(health.service_recommended, Some(true));
        // A desktop Mac has no battery section.
        assert_eq!(
            parse(r#"{"SPPowerDataType":[{"_name":"sppower_information"}]}"#),
            None
        );
        assert_eq!(parse("not json"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn running_it_reads_without_changing_anything() {
        // A Mac without a battery reports none; with one, the figure is a percent.
        if let Some(health) = read() {
            if let Some(percent) = health.maximum_capacity_percent {
                assert!((1.0..=100.0).contains(&percent));
            }
        }
    }
}
