//! This account's regular apps (`NSWorkspace.runningApplications`), and asking one to quit.
//!
//! Quit is Kleari's only action that changes anything (ADR-009). It is narrow by design:
//! - only a regular app of this account outside macOS's own folders, never Kleari itself,
//!   an app that ships with macOS, a background or a system process;
//! - only the polite request macOS sends when you choose Quit (`NSRunningApplication
//!   terminate`), which the app may answer by asking to save, or decline;
//! - the target is bound to the identity the page showed (start time to the microsecond and
//!   owner). Everything is checked in one main-thread step: the identity, then the AppKit app,
//!   then the identity again immediately before the request, so a reused PID is refused.

use serde::Serialize;

use std::path::Path;

use super::process_list::{is_quittable_bundle, ProcessIdentity, RunningApp};
use super::process_sampler::QuitTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
// Requested and Declined come from AppKit; on other targets (Linux CI) only tests build them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum QuitOutcome {
    /// macOS delivered the request; the app decides what happens next.
    Requested,
    /// The app has already quit, or its PID now belongs to another process.
    NotRunning,
    /// Kleari itself, another account's process, an app that ships with macOS, or not a
    /// regular app.
    NotAllowed,
    /// macOS didn't deliver the request.
    Declined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

/// The identity checks, run before and again immediately after the AppKit lookup. `token`
/// is the start time (microseconds) the page showed; `observed` is the PID's identity now.
// Only the macOS quit path calls it; on other targets (Linux CI) only tests do.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn identity_check(
    own_pid: u32,
    own_uid: u32,
    pid: u32,
    token: u64,
    observed: Option<ProcessIdentity>,
) -> Option<QuitOutcome> {
    if pid == own_pid {
        return Some(QuitOutcome::NotAllowed);
    }
    match observed {
        Some(identity) if identity.start_us != token || token == 0 => Some(QuitOutcome::NotRunning),
        Some(identity) if identity.uid != own_uid => Some(QuitOutcome::NotAllowed),
        Some(_) => None,
        None => Some(QuitOutcome::NotRunning),
    }
}

/// The app AppKit found must be the one the list showed: its resolved bundle equals the
/// listed one, and it is still outside macOS's folders. `observed` is already resolved.
// Only the macOS quit path calls it; on other targets (Linux CI) only tests do.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bundle_check(observed: Option<&Path>, expected: &Path) -> Option<QuitOutcome> {
    match observed {
        Some(bundle) if bundle != expected => Some(QuitOutcome::NotRunning),
        Some(bundle) if !is_quittable_bundle(bundle) => Some(QuitOutcome::NotAllowed),
        Some(_) => None,
        None => Some(QuitOutcome::NotRunning),
    }
}

/// A process's start time (microseconds) and owner, from `proc_pidinfo`. `None` if the
/// process is gone or macOS won't say.
#[cfg(target_os = "macos")]
pub fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: `info` is writable storage of exactly `size` bytes for PROC_PIDTBSDINFO.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: proc_pidinfo filled all `size` bytes.
    let info = unsafe { info.assume_init() };
    let start_us = info
        .pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)?;
    Some(ProcessIdentity {
        start_us,
        uid: info.pbi_uid,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn process_identity(_pid: u32) -> Option<ProcessIdentity> {
    None
}

/// Reads this account's regular apps on the main thread. Must be called off it.
#[cfg(target_os = "macos")]
pub fn read<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<RunningApp>, Unavailable> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::regular_apps());
    })
    .map_err(|_| Unavailable)?;
    receiver.recv().map_err(|_| Unavailable)
}

#[cfg(not(target_os = "macos"))]
pub fn read<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<Vec<RunningApp>, Unavailable> {
    Err(Unavailable)
}

/// Asks `target`, the app the latest list offered under `token`, to quit. Must be called off
/// the main thread; every check and the request run together on it.
#[cfg(target_os = "macos")]
pub fn quit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    target: QuitTarget,
    token: u64,
) -> Result<QuitOutcome, Unavailable> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::quit(&target, token));
    })
    .map_err(|_| Unavailable)?;
    receiver.recv().map_err(|_| Unavailable)
}

/// A running app's bundle identifier and name, for telling whether a cache is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningBundle {
    pub id: String,
    pub name: String,
}

/// Every running app of this account that has a bundle identifier, background agents
/// included (their caches are in use too). Read on the main thread; must be called off it.
#[cfg(target_os = "macos")]
pub fn running_bundles<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<RunningBundle>, Unavailable> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // The receiver only goes away if the waiting command was dropped; nothing to report.
        let _ = sender.send(macos::running_bundles());
    })
    .map_err(|_| Unavailable)?;
    receiver.recv().map_err(|_| Unavailable)
}

#[cfg(not(target_os = "macos"))]
pub fn running_bundles<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
) -> Result<Vec<RunningBundle>, Unavailable> {
    Err(Unavailable)
}

#[cfg(not(target_os = "macos"))]
pub fn quit<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _target: QuitTarget,
    _token: u64,
) -> Result<QuitOutcome, Unavailable> {
    Err(Unavailable)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};

    use super::super::process_sampler::resolve_bundle;
    use super::{
        bundle_check, identity_check, process_identity, QuitOutcome, QuitTarget, RunningApp,
        RunningBundle,
    };

    fn bundle_of(app: &NSRunningApplication) -> Option<PathBuf> {
        app.bundleURL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    pub fn running_bundles() -> Vec<RunningBundle> {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| !app.isTerminated())
            .filter_map(|app| {
                let id = app.bundleIdentifier()?.to_string();
                let name = app
                    .localizedName()
                    .map(|name| name.to_string())
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| id.clone());
                Some(RunningBundle { id, name })
            })
            .collect()
    }

    pub fn regular_apps() -> Vec<RunningApp> {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| {
                app.activationPolicy() == NSApplicationActivationPolicy::Regular
                    && !app.isTerminated()
            })
            .filter_map(|app| {
                let pid = u32::try_from(app.processIdentifier()).ok()?;
                let bundle = bundle_of(&app);
                let name = app
                    .localizedName()
                    .map(|name| name.to_string())
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| {
                        bundle
                            .as_deref()
                            .and_then(|bundle| bundle.file_stem())
                            .map(|stem| stem.to_string_lossy().into_owned())
                    })?;
                Some(RunningApp { pid, name, bundle })
            })
            .collect()
    }

    /// Runs on the main thread.
    pub fn quit(target: &QuitTarget, token: u64) -> QuitOutcome {
        let pid = target.pid;
        let own_pid = std::process::id();
        // SAFETY: getuid has no preconditions and cannot fail.
        let own_uid = unsafe { libc::getuid() };
        let check = || identity_check(own_pid, own_uid, pid, token, process_identity(pid));
        if let Some(outcome) = check() {
            return outcome;
        }
        let Ok(raw) = libc::pid_t::try_from(pid) else {
            return QuitOutcome::NotRunning;
        };
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(raw) else {
            return QuitOutcome::NotRunning;
        };
        if app.isTerminated() {
            return QuitOutcome::NotRunning;
        }
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            return QuitOutcome::NotAllowed;
        }
        // It must be the app the list showed, resolved the same way, and not a system app.
        let resolved = bundle_of(&app).and_then(|bundle| resolve_bundle(&bundle));
        if let Some(outcome) = bundle_check(resolved.as_deref(), &target.bundle) {
            return outcome;
        }
        // Again immediately before the request: the PID must still be the process shown.
        if let Some(outcome) = check() {
            return outcome;
        }
        if app.terminate() {
            QuitOutcome::Requested
        } else {
            QuitOutcome::Declined
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 501;

    fn identity(start_us: u64, uid: u32) -> Option<ProcessIdentity> {
        Some(ProcessIdentity { start_us, uid })
    }

    #[test]
    fn kleari_itself_is_never_asked_to_quit() {
        assert_eq!(
            identity_check(42, ME, 42, 10, identity(10, ME)),
            Some(QuitOutcome::NotAllowed)
        );
    }

    #[test]
    fn a_reused_or_gone_pid_is_not_asked_to_quit() {
        // Same PID, a start time one microsecond later: another process now has it.
        assert_eq!(
            identity_check(1, ME, 500, 10, identity(11, ME)),
            Some(QuitOutcome::NotRunning)
        );
        assert_eq!(
            identity_check(1, ME, 500, 10, None),
            Some(QuitOutcome::NotRunning)
        );
        assert_eq!(
            identity_check(1, ME, 500, 0, identity(0, ME)),
            Some(QuitOutcome::NotRunning)
        );
        // The process that was shown is still running.
        assert_eq!(identity_check(1, ME, 500, 10, identity(10, ME)), None);
    }

    #[test]
    fn only_the_listed_app_outside_macos_folders_passes_the_bundle_check() {
        let chrome = Path::new("/Applications/Google Chrome.app");
        assert_eq!(bundle_check(Some(chrome), chrome), None);
        // Another app now has the PID.
        assert_eq!(
            bundle_check(Some(Path::new("/Applications/Slack.app")), chrome),
            Some(QuitOutcome::NotRunning)
        );
        assert_eq!(bundle_check(None, chrome), Some(QuitOutcome::NotRunning));
        // A system app is refused even if it somehow matches.
        let safari = Path::new("/System/Cryptexes/App/System/Applications/Safari.app");
        assert_eq!(
            bundle_check(Some(safari), safari),
            Some(QuitOutcome::NotAllowed)
        );
    }

    #[test]
    fn another_accounts_process_is_refused() {
        assert_eq!(
            identity_check(1, ME, 500, 10, identity(10, 0)),
            Some(QuitOutcome::NotAllowed)
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn this_process_identifies_to_the_microsecond_and_is_refused() {
        let own = std::process::id();
        let found = process_identity(own).expect("this process is running");
        // SAFETY: getuid has no preconditions and cannot fail.
        assert_eq!(found.uid, unsafe { libc::getuid() });
        assert!(found.start_us > 1_000_000_000_000_000);
        assert_eq!(
            identity_check(own, found.uid, own, found.start_us, Some(found)),
            Some(QuitOutcome::NotAllowed)
        );
        assert_eq!(process_identity(u32::MAX), None);
    }

    #[test]
    fn outcomes_serialize_to_stable_names() {
        for (outcome, name) in [
            (QuitOutcome::Requested, "requested"),
            (QuitOutcome::NotRunning, "notRunning"),
            (QuitOutcome::NotAllowed, "notAllowed"),
            (QuitOutcome::Declined, "declined"),
        ] {
            assert_eq!(serde_json::to_value(outcome).expect("serializes"), name);
        }
    }
}
