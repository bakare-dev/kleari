//! Open at login, through macOS's own login items (`SMAppService.mainAppService`, macOS 13 and
//! later). macOS shows its own notice when Kleari is added, and the user can turn it off in
//! System Settings › General › Login Items at any time; Kleari reads the state back from
//! macOS rather than remembering it.

use serde::Serialize;

// Only macOS reports the other states; on other targets (Linux CI) it is always Unavailable.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginItem {
    Enabled,
    /// Not a login item. macOS may also report a main app that was never registered as "not
    /// found"; registering is the only way to tell, so that is offered as off too.
    Disabled,
    /// Added, but the user must allow it in System Settings › Login Items.
    RequiresApproval,
    /// macOS gave Kleari no login item to manage.
    Unavailable,
    /// This macOS is older than 13, which added the login items Kleari uses.
    Unsupported,
}

/// The outcome of turning Open at login on or off: what macOS reports now, and macOS's own
/// reason if it refused the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginChange {
    pub state: LoginItem,
    pub refusal: Option<String>,
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::NSError;

    use super::LoginItem;

    #[link(name = "ServiceManagement", kind = "framework")]
    extern "C" {}

    /// The app's own login item: `Err(Unsupported)` before macOS 13 (no `SMAppService`),
    /// `Err(Unavailable)` if macOS gives none.
    fn service() -> Result<*mut AnyObject, LoginItem> {
        let class = AnyClass::get(c"SMAppService").ok_or(LoginItem::Unsupported)?;
        // SAFETY: `mainAppService` is a class property returning an autoreleased SMAppService
        // (never nil on macOS 13+); it takes no arguments.
        let service: *mut AnyObject = unsafe { msg_send![class, mainAppService] };
        if service.is_null() {
            Err(LoginItem::Unavailable)
        } else {
            Ok(service)
        }
    }

    pub fn status() -> LoginItem {
        let service = match service() {
            Ok(service) => service,
            Err(state) => return state,
        };
        // SAFETY: `status` is a read-only NSInteger property of a live SMAppService.
        let status: isize = unsafe { msg_send![service, status] };
        match status {
            1 => LoginItem::Enabled,
            2 => LoginItem::RequiresApproval,
            // 0 is not registered; 3 (not found) is also what macOS reports for a main app
            // never registered, so both are off and registering is offered.
            _ => LoginItem::Disabled,
        }
    }

    /// Adds or removes Kleari as a login item; macOS's reason when it refuses.
    pub fn set(enabled: bool) -> Result<(), String> {
        let service = service().map_err(|_| "macOS has no login item for Kleari.".to_owned())?;
        let mut error: *mut NSError = std::ptr::null_mut();
        // SAFETY: both methods take an `NSError **` that macOS fills on failure with an
        // autoreleased error, and return BOOL.
        let done: Bool = unsafe {
            if enabled {
                msg_send![service, registerAndReturnError: &mut error]
            } else {
                msg_send![service, unregisterAndReturnError: &mut error]
            }
        };
        if done.as_bool() {
            return Ok(());
        }
        // SAFETY: on failure `error` is null or a valid NSError for this autorelease scope.
        let reason = unsafe { error.as_ref() }.map_or_else(
            || "macOS refused the change.".to_owned(),
            |error| error.localizedDescription().to_string(),
        );
        Err(reason)
    }
}

#[cfg(target_os = "macos")]
pub fn status() -> LoginItem {
    macos::status()
}

/// Turns Open at login on or off, then reads back what macOS now says.
#[cfg(target_os = "macos")]
pub fn set(enabled: bool) -> LoginChange {
    let refusal = macos::set(enabled).err();
    LoginChange {
        state: macos::status(),
        refusal,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> LoginItem {
    LoginItem::Unavailable
}

#[cfg(not(target_os = "macos"))]
pub fn set(_enabled: bool) -> LoginChange {
    LoginChange {
        state: LoginItem::Unavailable,
        refusal: None,
    }
}
