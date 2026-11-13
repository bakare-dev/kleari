//! Local notifications (a scan finished, quarantine passed the reminder size), through macOS's
//! notification centre. Nothing is sent anywhere else. macOS asks the user once, when a
//! notification is first turned on in Settings; a refusal is shown with a way to System
//! Settings › Notifications. Sending is skipped when the user hasn't allowed it.

use serde::Serialize;

// Only macOS reports the other states; on other targets (Linux CI) it is always Unavailable.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationAccess {
    Allowed,
    Denied,
    /// macOS hasn't asked yet.
    NotAsked,
    /// Not here (Kleari isn't running from its app bundle, or macOS didn't answer).
    Unavailable,
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotificationRequest, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter,
    };

    use super::NotificationAccess;

    /// How long to wait for macOS's answer. The permission prompt itself doesn't block it:
    /// macOS answers the request once the user chooses, so asking waits longer.
    const ANSWER: Duration = Duration::from_secs(3);
    const PROMPT: Duration = Duration::from_secs(120);

    /// The notification centre aborts the process when it isn't running from an app bundle
    /// (`tauri dev`), so it is only touched from one.
    fn center() -> Option<objc2::rc::Retained<UNUserNotificationCenter>> {
        let bundle = NSBundle::mainBundle();
        let in_app = bundle.bundleIdentifier().is_some()
            && bundle.bundlePath().to_string().ends_with(".app");
        in_app.then(UNUserNotificationCenter::currentNotificationCenter)
    }

    pub fn status() -> NotificationAccess {
        let Some(center) = center() else {
            return NotificationAccess::Unavailable;
        };
        let (sender, receiver) = mpsc::channel();
        let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            // SAFETY: macOS passes a valid settings object for the duration of the call.
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            // The receiver is gone only if the wait timed out; nothing to report then.
            let _ = sender.send(status);
        });
        center.getNotificationSettingsWithCompletionHandler(&block);
        match receiver.recv_timeout(ANSWER) {
            Ok(UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Provisional) => {
                NotificationAccess::Allowed
            }
            Ok(UNAuthorizationStatus::Denied) => NotificationAccess::Denied,
            Ok(UNAuthorizationStatus::NotDetermined) => NotificationAccess::NotAsked,
            _ => NotificationAccess::Unavailable,
        }
    }

    pub fn request() -> NotificationAccess {
        let Some(center) = center() else {
            return NotificationAccess::Unavailable;
        };
        let (sender, receiver) = mpsc::channel();
        let block = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            // The receiver is gone only if the wait timed out; the status is read again then.
            let _ = sender.send(granted.as_bool());
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
        match receiver.recv_timeout(PROMPT) {
            Ok(true) => NotificationAccess::Allowed,
            Ok(false) => NotificationAccess::Denied,
            Err(_) => status(),
        }
    }

    /// Shows a notification, only if macOS currently allows Kleari's (the user may have
    /// turned them off in System Settings since turning them on here).
    pub fn send(identifier: &str, title: &str, body: &str, sound: bool) {
        if status() != NotificationAccess::Allowed {
            return;
        }
        let Some(center) = center() else {
            return;
        };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        if sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(identifier),
            &content,
            None,
        );
        // Delivery is macOS's; a failure leaves only the missing notification.
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    mod delegate {
        use std::sync::OnceLock;

        use objc2::rc::Retained;
        use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
        use objc2::{define_class, msg_send, ClassType};
        use objc2_user_notifications::{
            UNNotification, UNNotificationPresentationOptions, UNNotificationResponse,
            UNUserNotificationCenter, UNUserNotificationCenterDelegate,
        };

        /// The app, for opening a window when a notification is clicked.
        static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

        define_class!(
            // SAFETY: NSObject has no subclassing requirements, and `Delegate` keeps no state
            // and doesn't implement Drop. macOS may call it on any thread.
            #[unsafe(super(NSObject))]
            #[name = "KleariNotificationDelegate"]
            struct Delegate;

            unsafe impl NSObjectProtocol for Delegate {}

            unsafe impl UNUserNotificationCenterDelegate for Delegate {
                #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
                fn did_receive(
                    &self,
                    _center: &UNUserNotificationCenter,
                    response: &UNNotificationResponse,
                    completion_handler: &block2::DynBlock<dyn Fn()>,
                ) {
                    let identifier = response.notification().request().identifier().to_string();
                    if let Some(app) = APP.get() {
                        super::super::clicked(app, &identifier);
                    }
                    completion_handler.call(());
                }

                // Shown even while Kleari is in front, as a scheduled scan may finish then.
                #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
                fn will_present(
                    &self,
                    _center: &UNUserNotificationCenter,
                    _notification: &UNNotification,
                    completion_handler: &block2::DynBlock<
                        dyn Fn(UNNotificationPresentationOptions),
                    >,
                ) {
                    completion_handler.call((UNNotificationPresentationOptions::Banner
                        | UNNotificationPresentationOptions::List
                        | UNNotificationPresentationOptions::Sound,));
                }
            }
        );

        /// Handles clicks on Kleari's notifications from now on. Only from the app bundle,
        /// like everything else here.
        pub fn listen(app: tauri::AppHandle) {
            let Some(center) = super::center() else {
                return;
            };
            if APP.set(app).is_err() {
                return;
            }
            // SAFETY: `new` on an NSObject subclass with no ivars returns an initialised object.
            let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::class(), new] };
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            // The centre holds its delegate weakly; this one lives as long as Kleari does.
            std::mem::forget(delegate);
        }
    }

    pub use delegate::listen;
}

#[cfg(target_os = "macos")]
pub use macos::{request, send, status};

/// Clicking a notification about a scan opens the review; the others just bring Kleari
/// forward, as macOS does by itself.
#[cfg(target_os = "macos")]
fn clicked(app: &tauri::AppHandle, identifier: &str) {
    if identifier != crate::cleanup_commands::SCAN_NOTIFICATION {
        return;
    }
    let handle = app.clone();
    // Windows are touched on the main thread; if that can't be reached, macOS has still
    // brought Kleari forward.
    let _ = app.run_on_main_thread(move || {
        let _ = crate::menubar::open_at(&handle, crate::menubar::Destination::Review);
    });
}

/// From now on, clicking a scan's notification opens the review.
#[cfg(target_os = "macos")]
pub fn open_review_on_click(app: tauri::AppHandle) {
    macos::listen(app);
}

#[cfg(not(target_os = "macos"))]
pub fn open_review_on_click(_app: tauri::AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn status() -> NotificationAccess {
    NotificationAccess::Unavailable
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> NotificationAccess {
    NotificationAccess::Unavailable
}

#[cfg(not(target_os = "macos"))]
pub fn send(_identifier: &str, _title: &str, _body: &str, _sound: bool) {}
