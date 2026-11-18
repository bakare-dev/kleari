//! One Kleari at a time. Two copies would each install their event taps and handle every
//! mouse event twice (seen when a development build and a release ran together; they are
//! different apps to macOS, so its own single-launch rule doesn't cover them). Both share the
//! data folder, so the first to start holds an exclusive lock on `kleari.lock` in it for as
//! long as it runs (macOS releases it when the process ends, even in a crash). A later launch
//! that finds it held asks the running Kleari, through a distributed notification on this Mac,
//! to show its window, and quits before opening anything. Nothing polls: the lock is taken
//! once, and the running Kleari only reacts when the notification arrives.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const LOCK_NAME: &str = "kleari.lock";

/// How taking the lock went.
#[derive(Debug)]
pub enum Claim {
    /// This is the only Kleari; the lock lasts as long as the file stays open (it is only
    /// held, never read).
    First { _lock: File },
    /// Another Kleari holds the lock.
    Taken,
    /// The lock couldn't be made or read: Kleari runs anyway (one copy is the usual case).
    Unknown,
}

/// Takes the lock in `data_folder`, without waiting.
pub fn claim(data_folder: &Path) -> Claim {
    if std::fs::create_dir_all(data_folder).is_err() {
        return Claim::Unknown;
    }
    let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(data_folder.join(LOCK_NAME))
    else {
        return Claim::Unknown;
    };
    // SAFETY: a valid descriptor owned by `file`, which outlives the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Claim::First { _lock: file };
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
        Claim::Taken
    } else {
        Claim::Unknown
    }
}

/// The notification's name, per bundle identifier, so another app's can't be mistaken for it.
fn notification_name(identifier: &str) -> String {
    format!("{identifier}.show")
}

/// Asks the running Kleari to show its window. It carries nothing else.
#[cfg(target_os = "macos")]
pub fn ask_first_to_show(identifier: &str) {
    use objc2_foundation::{NSDistributedNotificationCenter, NSString};
    let name = NSString::from_str(&notification_name(identifier));
    // SAFETY: no user info is passed. Delivered at once even while the running Kleari is in
    // the background, where distributed notifications are otherwise held back.
    unsafe {
        NSDistributedNotificationCenter::defaultCenter()
            .postNotificationName_object_userInfo_deliverImmediately(&name, None, None, true);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ask_first_to_show(_identifier: &str) {}

/// In the running Kleari: shows the main window whenever a later launch asks.
#[cfg(target_os = "macos")]
pub fn show_when_asked(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};
    use std::ptr::NonNull;

    let name = NSString::from_str(&notification_name(&app.config().identifier));
    let app = app.clone();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        crate::log::note("instance", "another launch asked this Kleari to show");
        let shown = app.clone();
        // Tauri's window calls go to the main thread themselves; this only hands it over.
        let _ = app.run_on_main_thread(move || crate::menubar::show_main(&shown));
    });
    // SAFETY: no object filter or queue (delivered on the main thread's run loop, which
    // registered it); the block only captures the app handle, which is Send and Sync.
    let observer =
        unsafe {
            NSDistributedNotificationCenter::defaultCenter()
                .addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block)
        };
    // Observed for as long as Kleari runs.
    std::mem::forget(observer);
}

#[cfg(not(target_os = "macos"))]
pub fn show_when_asked(_app: &tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_finds_the_lock_taken_until_the_first_ends() {
        let folder = std::env::temp_dir().join(format!("kleari-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let first = claim(&folder);
        assert!(matches!(first, Claim::First { .. }));
        // flock locks belong to the open file, so a second open in this process conflicts
        // just as another process's would.
        assert!(matches!(claim(&folder), Claim::Taken));
        drop(first);
        assert!(matches!(claim(&folder), Claim::First { .. }));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_notification_is_named_for_the_identifier() {
        assert_eq!(notification_name("dev.kleari.app"), "dev.kleari.app.show");
    }
}
