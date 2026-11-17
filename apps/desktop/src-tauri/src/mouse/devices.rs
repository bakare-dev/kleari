//! Which external mice are connected, and how, so the Mouse page can show the settings apply to
//! them and never to a trackpad.
//!
//! The list comes from IOKit's registry of pointing devices: their name, how they connect and
//! whether they are built in. Reading it needs no permission and opens no device, so nothing a
//! device sends is seen here. Mice behave the same however they connect (USB, a wireless
//! receiver, Bluetooth): macOS turns them all into the same mouse events, which the event tap
//! reads (`tap.rs`). Read only when the page asks; never polled.

use serde::Serialize;

/// How a mouse is connected, as the page names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    Usb,
    /// A USB receiver (2.4 GHz or similar) the mouse talks to wirelessly.
    Receiver,
    Bluetooth,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mouse {
    pub name: String,
    pub connection: Connection,
}

/// What a connected pointing device is, from its IOKit properties: an external mouse (and how
/// it connects), or `None` for the built-in trackpad, a Magic Trackpad or a tablet-like device.
pub fn classify(product: &str, transport: &str, built_in: bool) -> Option<Connection> {
    let name = product.to_lowercase();
    // The MacBook's own keyboard and trackpad come over SPI or FIFO, marked built in.
    if built_in || matches!(transport, "SPI" | "FIFO") {
        return None;
    }
    // Trackpads (a Magic Trackpad, or another make's) keep macOS's own settings.
    if name.contains("trackpad") || name.contains("touchpad") {
        return None;
    }
    let receiver = [
        "receiver", "unifying", "bolt", "dongle", "wireless", "2.4g", "nano",
    ]
    .iter()
    .any(|word| name.contains(word));
    Some(match transport {
        "USB" if receiver => Connection::Receiver,
        "USB" => Connection::Usb,
        "Bluetooth" | "Bluetooth Low Energy" | "BluetoothLowEnergy" => Connection::Bluetooth,
        _ => Connection::Other,
    })
}

/// The external mice connected now, each once (a mouse often shows several HID interfaces),
/// by name.
#[cfg(target_os = "macos")]
pub fn connected() -> Vec<Mouse> {
    let mut mice: Vec<Mouse> = macos::pointing_devices()
        .into_iter()
        .filter_map(|(product, transport, built_in)| {
            classify(&product, &transport, built_in).map(|connection| Mouse {
                name: if product.trim().is_empty() {
                    "Mouse".to_owned()
                } else {
                    product.trim().to_owned()
                },
                connection,
            })
        })
        .collect();
    mice.sort_by(|a, b| a.name.cmp(&b.name));
    mice.dedup();
    mice
}

#[cfg(not(target_os = "macos"))]
pub fn connected() -> Vec<Mouse> {
    Vec::new()
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString};
    use objc2_io_kit::{IOHIDDevice, IOHIDManager};

    /// HID usage page "Generic Desktop", and its usages "Pointer" and "Mouse".
    const GENERIC_DESKTOP: i32 = 0x01;
    const POINTER: i32 = 0x01;
    const MOUSE: i32 = 0x02;

    fn matching(usage: i32) -> CFRetained<CFDictionary<CFString, CFNumber>> {
        CFDictionary::from_slices(
            &[
                &*CFString::from_static_str("DeviceUsagePage"),
                &*CFString::from_static_str("DeviceUsage"),
            ],
            &[
                &*CFNumber::new_i32(GENERIC_DESKTOP),
                &*CFNumber::new_i32(usage),
            ],
        )
    }

    fn text(device: &IOHIDDevice, key: &'static str) -> String {
        device
            .property(&CFString::from_static_str(key))
            .and_then(|value| value.downcast_ref::<CFString>().map(ToString::to_string))
            .unwrap_or_default()
    }

    /// IOKit stores it as a boolean on some Macs and a number on others.
    fn built_in(device: &IOHIDDevice) -> bool {
        let Some(value) = device.property(&CFString::from_static_str("Built-In")) else {
            return false;
        };
        if let Some(flag) = value.downcast_ref::<CFBoolean>() {
            return flag.as_bool();
        }
        value
            .downcast_ref::<CFNumber>()
            .and_then(CFNumber::as_i64)
            .is_some_and(|number| number != 0)
    }

    /// Each pointing device's product name, transport and whether it is built in.
    pub fn pointing_devices() -> Vec<(String, String, bool)> {
        let manager = IOHIDManager::new(None, 0);
        let pointer = matching(POINTER);
        let mouse = matching(MOUSE);
        let both = CFArray::from_objects(&[&*pointer, &*mouse]);
        // SAFETY: an array of matching dictionaries, as the call expects.
        unsafe { manager.set_device_matching_multiple(Some(both.as_opaque())) };
        let Some(devices) = manager.devices() else {
            return Vec::new();
        };
        let count = usize::try_from(devices.count()).unwrap_or(0);
        let mut values: Vec<*const std::ffi::c_void> = vec![std::ptr::null(); count];
        // SAFETY: `values` has room for every member of the set.
        unsafe { devices.values(values.as_mut_ptr()) };
        values
            .into_iter()
            .filter_map(|value| {
                // SAFETY: the set holds IOHIDDevices, alive while `devices` is.
                let device = unsafe { value.cast::<IOHIDDevice>().as_ref() }?;
                Some((
                    text(device, "Product"),
                    text(device, "Transport"),
                    built_in(device),
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_trackpad_and_a_magic_trackpad_are_not_mice() {
        // As this MacBook reports its own (seen with a probe of the IOKit registry).
        assert_eq!(
            classify("Apple Internal Keyboard / Trackpad", "FIFO", true),
            None
        );
        assert_eq!(
            classify("Apple Internal Keyboard / Trackpad", "SPI", false),
            None
        );
        assert_eq!(classify("Magic Trackpad", "Bluetooth", false), None);
        assert_eq!(classify("Magic Trackpad", "USB", false), None);
    }

    #[test]
    fn mice_are_mice_however_they_connect() {
        assert_eq!(
            classify("MX Master 3S", "Bluetooth Low Energy", false),
            Some(Connection::Bluetooth)
        );
        assert_eq!(
            classify("Magic Mouse", "Bluetooth", false),
            Some(Connection::Bluetooth)
        );
        assert_eq!(
            classify("USB Optical Mouse", "USB", false),
            Some(Connection::Usb)
        );
        assert_eq!(
            classify("USB Receiver", "USB", false),
            Some(Connection::Receiver)
        );
        assert_eq!(
            classify("Logi Bolt Receiver", "USB", false),
            Some(Connection::Receiver)
        );
        assert_eq!(
            classify("2.4G Wireless Mouse", "USB", false),
            Some(Connection::Receiver)
        );
        assert_eq!(classify("Gaming Mouse", "", false), Some(Connection::Other));
    }

    #[test]
    fn the_page_gets_tagged_names() {
        let text = serde_json::to_string(&Mouse {
            name: "MX Master 3S".to_owned(),
            connection: Connection::Bluetooth,
        })
        .unwrap_or_default();
        assert_eq!(text, r#"{"name":"MX Master 3S","connection":"bluetooth"}"#);
    }
}
