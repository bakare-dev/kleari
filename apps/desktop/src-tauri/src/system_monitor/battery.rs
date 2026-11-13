//! OS adapter for the internal battery, read from macOS IOKit power sources (the same source
//! as the menu-bar battery). Read-only; no shell execution or privileges.

/// Raw power-source values before availability decisions; keys follow `IOPSKeys.h`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawBatteryFields {
    pub is_present: Option<bool>,
    pub current_capacity: Option<i64>,
    pub max_capacity: Option<i64>,
    pub is_charging: Option<bool>,
    pub is_charged: Option<bool>,
    /// `Some(true)` for "AC Power", `Some(false)` for "Battery Power".
    pub on_ac_power: Option<bool>,
    /// Minutes; macOS reports -1 while it is still calculating.
    pub time_to_empty: Option<i64>,
    pub time_to_full: Option<i64>,
}

// Only the macOS adapter constructs `NotPresent` and `Present`; other targets (Linux CI)
// always report `Unreadable`, so the variants are legitimately unused there.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawBattery {
    /// The power-source list was read and contains no internal battery.
    NotPresent,
    /// The power-source API failed or is not available on this platform.
    Unreadable,
    Present(RawBatteryFields),
}

#[cfg(target_os = "macos")]
pub fn read_battery() -> RawBattery {
    macos::read()
}

#[cfg(not(target_os = "macos"))]
pub fn read_battery() -> RawBattery {
    RawBattery::Unreadable
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;

    use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFNumber, CFString, CFType};
    use objc2_io_kit::{
        kIOPSACPowerValue, kIOPSBatteryPowerValue, kIOPSCurrentCapacityKey,
        kIOPSInternalBatteryType, kIOPSIsChargedKey, kIOPSIsChargingKey, kIOPSIsPresentKey,
        kIOPSMaxCapacityKey, kIOPSPowerSourceStateKey, kIOPSTimeToEmptyKey,
        kIOPSTimeToFullChargeKey, kIOPSTypeKey, IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList,
        IOPSGetPowerSourceDescription,
    };

    use super::{RawBattery, RawBatteryFields};

    type Description = CFDictionary<CFString, CFType>;

    pub fn read() -> RawBattery {
        let Some(blob) = IOPSCopyPowerSourcesInfo() else {
            return RawBattery::Unreadable;
        };
        // SAFETY: `blob` is the value returned by IOPSCopyPowerSourcesInfo, as required.
        let Some(list) = (unsafe { IOPSCopyPowerSourcesList(Some(&blob)) }) else {
            return RawBattery::Unreadable;
        };
        // SAFETY: IOKit documents the list as a CFArray of power-source CFTypeRefs.
        let list: &CFArray<CFType> = unsafe { list.cast_unchecked() };

        for source in list.iter() {
            // SAFETY: `blob` and `source` come from the two calls above, as documented.
            let Some(description) =
                (unsafe { IOPSGetPowerSourceDescription(Some(&blob), Some(&source)) })
            else {
                continue;
            };
            // SAFETY: IOPSKeys.h documents CFString keys; values are read as CFType and
            // checked with `downcast_ref` before use.
            let description: &Description = unsafe { description.cast_unchecked() };
            if string(description, kIOPSTypeKey).as_deref()
                == kIOPSInternalBatteryType.to_str().ok()
            {
                return RawBattery::Present(fields(description));
            }
        }
        RawBattery::NotPresent
    }

    fn fields(description: &Description) -> RawBatteryFields {
        let state = string(description, kIOPSPowerSourceStateKey);
        let on_ac_power = match state.as_deref() {
            Some(value) if Some(value) == kIOPSACPowerValue.to_str().ok() => Some(true),
            Some(value) if Some(value) == kIOPSBatteryPowerValue.to_str().ok() => Some(false),
            _ => None,
        };
        RawBatteryFields {
            is_present: boolean(description, kIOPSIsPresentKey),
            current_capacity: integer(description, kIOPSCurrentCapacityKey),
            max_capacity: integer(description, kIOPSMaxCapacityKey),
            is_charging: boolean(description, kIOPSIsChargingKey),
            is_charged: boolean(description, kIOPSIsChargedKey),
            on_ac_power,
            time_to_empty: integer(description, kIOPSTimeToEmptyKey),
            time_to_full: integer(description, kIOPSTimeToFullChargeKey),
        }
    }

    fn value(
        description: &Description,
        key: &CStr,
    ) -> Option<objc2_core_foundation::CFRetained<CFType>> {
        let key = CFString::from_str(key.to_str().ok()?);
        description.get(&key)
    }

    fn string(description: &Description, key: &CStr) -> Option<String> {
        value(description, key)?
            .downcast_ref::<CFString>()
            .map(ToString::to_string)
    }

    fn integer(description: &Description, key: &CStr) -> Option<i64> {
        value(description, key)?
            .downcast_ref::<CFNumber>()?
            .as_i64()
    }

    fn boolean(description: &Description, key: &CStr) -> Option<bool> {
        value(description, key)?
            .downcast_ref::<CFBoolean>()
            .map(CFBoolean::as_bool)
    }
}
