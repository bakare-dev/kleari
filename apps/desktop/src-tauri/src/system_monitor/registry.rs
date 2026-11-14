//! Read-only IORegistry property lookups (GPU core count, battery health). Values are copied
//! as CF snapshots; no service is opened and nothing is written.

/// Integer properties of the first registered service of an IOKit class.
#[cfg(target_os = "macos")]
pub fn integers(class: &str, keys: &[&str]) -> Option<Vec<Option<i64>>> {
    macos::with_service(class, |service| {
        keys.iter()
            .map(|key| macos::integer(service, key))
            .collect()
    })
}

/// Integer values inside a dictionary property of the first service of an IOKit class.
#[cfg(target_os = "macos")]
pub fn dictionary_integers(class: &str, property: &str, keys: &[&str]) -> Option<Vec<Option<i64>>> {
    macos::with_service(class, |service| {
        macos::dictionary_integers(service, property, keys)
    })
}

/// For every registered service of an IOKit class: the integer properties `keys` and the
/// string property `name_key` (a device's name).
#[cfg(target_os = "macos")]
pub fn each(class: &str, name_key: &str, keys: &[&str]) -> Vec<(Option<String>, Vec<Option<i64>>)> {
    macos::each_service(class, |service| {
        (
            macos::string(service, name_key),
            keys.iter()
                .map(|key| macos::integer(service, key))
                .collect(),
        )
    })
}

/// Runs `use_service` with the first registered service of an IOKit class (its registry
/// entry), released afterwards. For the SMC's read-only user client (`sensors`).
#[cfg(target_os = "macos")]
pub fn open_service<T>(class: &str, use_service: impl FnOnce(u32) -> T) -> Option<T> {
    macos::with_service(class, use_service)
}

#[cfg(not(target_os = "macos"))]
pub fn each(
    _class: &str,
    _name_key: &str,
    _keys: &[&str],
) -> Vec<(Option<String>, Vec<Option<i64>>)> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn integers(_class: &str, _keys: &[&str]) -> Option<Vec<Option<i64>>> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn dictionary_integers(
    _class: &str,
    _property: &str,
    _keys: &[&str],
) -> Option<Vec<Option<i64>>> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;

    use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
    use objc2_io_kit::{
        io_iterator_t, io_registry_entry_t, kIOMainPortDefault, IOIteratorNext, IOObjectRelease,
        IORegistryEntryCreateCFProperty, IOServiceGetMatchingService, IOServiceGetMatchingServices,
        IOServiceMatching,
    };

    /// Most services visited for one class.
    const MAX_SERVICES: usize = 64;

    /// Runs `read` against every matching service (at most `MAX_SERVICES`), releasing each.
    pub fn each_service<T>(class: &str, mut read: impl FnMut(io_registry_entry_t) -> T) -> Vec<T> {
        let mut found = Vec::new();
        let Ok(class) = CString::new(class) else {
            return found;
        };
        // SAFETY: `class` is a valid NUL-terminated C string for the duration of the call.
        let Some(matching) = (unsafe { IOServiceMatching(class.as_ptr()) }) else {
            return found;
        };
        let matching: CFRetained<CFDictionary> = (&matching).into();
        let mut iterator: io_iterator_t = 0;
        // SAFETY: the default port, a matching dictionary the call consumes, and a writable
        // iterator.
        let status = unsafe {
            IOServiceGetMatchingServices(kIOMainPortDefault, Some(matching), &mut iterator)
        };
        if status != 0 || iterator == 0 {
            return found;
        }
        loop {
            let service = IOIteratorNext(iterator);
            if service == 0 {
                break;
            }
            if found.len() < MAX_SERVICES {
                found.push(read(service));
            }
            let _ = IOObjectRelease(service);
        }
        let _ = IOObjectRelease(iterator);
        found
    }

    pub fn string(service: io_registry_entry_t, key: &str) -> Option<String> {
        property(service, key)?
            .downcast_ref::<CFString>()
            .map(ToString::to_string)
            .filter(|text| !text.is_empty() && text.len() <= 200)
    }

    /// Runs `read` against the first matching service and releases it afterwards.
    pub fn with_service<T>(class: &str, read: impl FnOnce(io_registry_entry_t) -> T) -> Option<T> {
        let class = CString::new(class).ok()?;
        // SAFETY: `class` is a valid NUL-terminated C string for the duration of the call.
        let matching = unsafe { IOServiceMatching(class.as_ptr()) }?;
        // Upcast to the immutable type the binding takes. This retains once more; the call
        // consumes that reference and `matching` releases its own when it goes out of scope.
        let matching: CFRetained<CFDictionary> = (&matching).into();
        // SAFETY: `kIOMainPortDefault` is the documented default port; the matching dictionary
        // reference is consumed by the call, as the binding documents.
        let service = unsafe { IOServiceGetMatchingService(kIOMainPortDefault, Some(matching)) };
        if service == 0 {
            return None;
        }
        let value = read(service);
        // Releasing our reference cannot fail meaningfully; the result carries no information.
        let _ = IOObjectRelease(service);
        Some(value)
    }

    fn property(service: io_registry_entry_t, key: &str) -> Option<CFRetained<CFType>> {
        let key = CFString::from_str(key);
        // SAFETY: `service` is a live registry entry held by `with_service`; the key is a valid
        // CFString; a null allocator selects the default; no options are defined.
        unsafe { IORegistryEntryCreateCFProperty(service, Some(&key), None, 0) }
    }

    pub fn integer(service: io_registry_entry_t, key: &str) -> Option<i64> {
        property(service, key)?.downcast_ref::<CFNumber>()?.as_i64()
    }

    pub fn dictionary_integers(
        service: io_registry_entry_t,
        property_key: &str,
        keys: &[&str],
    ) -> Vec<Option<i64>> {
        let Some(value) = property(service, property_key) else {
            return vec![None; keys.len()];
        };
        let Some(dictionary) = value.downcast_ref::<CFDictionary>() else {
            return vec![None; keys.len()];
        };
        // SAFETY: IORegistry dictionaries have CFString keys; values are checked with
        // `downcast_ref` before use.
        let dictionary: &CFDictionary<CFString, CFType> = unsafe { dictionary.cast_unchecked() };
        keys.iter()
            .map(|key| {
                dictionary
                    .get(&CFString::from_str(key))
                    .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
            })
            .collect()
    }
}
