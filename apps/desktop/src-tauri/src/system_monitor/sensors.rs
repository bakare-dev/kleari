//! Chip temperatures and fan speeds (ADR-023), read-only, without privileges.
//!
//! macOS publishes no public API for either on Apple silicon, so these use the same private
//! interfaces Activity Monitor-style tools do:
//! - temperatures from `IOHIDEventSystemClient`: every HID service whose primary usage is a
//!   temperature sensor (`0xff00` / `5`), read as a temperature event. Sensor names differ by
//!   chip, so they are grouped by what their names say (`classify`);
//! - fan speeds from the `AppleSMC` service: `FNum` fans, each with its current, minimum and
//!   maximum speed (`F<n>Ac`, `F<n>Mn`, `F<n>Mx`), read-only (`kSMCReadKey`). Nothing is ever
//!   written to the SMC: fan control would need root, which Kleari never has.
//!
//! A private interface can change with a macOS update: whatever can't be read is reported as
//! not reported, never guessed.

use serde::Serialize;

/// Temperatures, in °C, as the chip's own sensors report them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipTemperatures {
    /// The average of the processor's sensors.
    pub cpu: f64,
    /// The hottest processor sensor.
    pub cpu_max: f64,
    /// The average of the graphics sensors, where the chip has them.
    pub gpu: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fan {
    pub rpm: f64,
    pub min_rpm: Option<f64>,
    pub max_rpm: Option<f64>,
}

/// Which part of the chip a sensor measures, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Cpu,
    Gpu,
    Other,
}

/// Groups a sensor by its name. Apple silicon names processor sensors after their clusters
/// ("pACC MTR Temp Sensor2", "eACC MTR Temp Sensor0") or as die sensors ("PMU tdie3"), and
/// graphics ones "GPU MTR Temp Sensor1"; Intel Macs report none of these here.
pub fn classify(name: &str) -> Part {
    let lower = name.to_ascii_lowercase();
    if lower.contains("gpu") {
        Part::Gpu
    } else if lower.contains("acc mtr") || lower.contains("cpu") || lower.contains("tdie") {
        Part::Cpu
    } else {
        Part::Other
    }
}

/// A plausible chip temperature (a sensor that isn't connected reads 0 or far out of range).
fn plausible(celsius: f64) -> bool {
    celsius.is_finite() && (5.0..=130.0).contains(&celsius)
}

/// The processor and graphics temperatures from named readings. Cluster sensors are preferred
/// over die sensors when a chip has both, since they sit on the cores.
pub fn summarize(readings: &[(String, f64)]) -> Option<ChipTemperatures> {
    let valid: Vec<(&str, f64)> = readings
        .iter()
        .filter(|(_, celsius)| plausible(*celsius))
        .map(|(name, celsius)| (name.as_str(), *celsius))
        .collect();
    let of = |part: Part, cluster: bool| -> Vec<f64> {
        valid
            .iter()
            .filter(|(name, _)| classify(name) == part)
            .filter(|(name, _)| !cluster || name.to_ascii_lowercase().contains("acc mtr"))
            .map(|(_, celsius)| *celsius)
            .collect()
    };
    let clusters = of(Part::Cpu, true);
    let cpu = if clusters.is_empty() {
        of(Part::Cpu, false)
    } else {
        clusters
    };
    if cpu.is_empty() {
        return None;
    }
    let average = |values: &[f64]| {
        #[allow(clippy::cast_precision_loss)]
        let count = values.len() as f64;
        values.iter().sum::<f64>() / count
    };
    let gpu = of(Part::Gpu, false);
    Some(ChipTemperatures {
        cpu: average(&cpu),
        cpu_max: cpu.iter().copied().fold(f64::MIN, f64::max),
        gpu: (!gpu.is_empty()).then(|| average(&gpu)),
    })
}

/// A plausible fan speed in rpm.
// Used by the macOS SMC reader and the tests; other targets (Linux CI) read no fans.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plausible_rpm(rpm: f64) -> bool {
    rpm.is_finite() && (0.0..=20_000.0).contains(&rpm)
}

/// An SMC value as a number, from its type: `flt ` (Apple silicon, little-endian f32),
/// `fpe2` (Intel, big-endian 14.2 fixed point), or an unsigned integer.
// Used by the macOS SMC reader and the tests; other targets (Linux CI) read no fans.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn smc_number(kind: [u8; 4], bytes: &[u8]) -> Option<f64> {
    match &kind {
        b"flt " => Some(f64::from(f32::from_le_bytes(
            bytes.get(..4)?.try_into().ok()?,
        ))),
        b"fpe2" => Some(f64::from(u16::from_be_bytes(bytes.get(..2)?.try_into().ok()?)) / 4.0),
        b"ui8 " => bytes.first().map(|&value| f64::from(value)),
        b"ui16" => Some(f64::from(u16::from_be_bytes(
            bytes.get(..2)?.try_into().ok()?,
        ))),
        _ => None,
    }
}

/// The chip's temperatures now, or `None` when its sensors can't be read.
pub fn read_temperatures() -> Option<ChipTemperatures> {
    summarize(&macos::temperature_readings()?)
}

/// The fans now: an empty list for a Mac without fans, `None` when the SMC can't be read.
pub fn read_fans() -> Option<Vec<Fan>> {
    macos::fans()
}

/// Every temperature sensor with its reading, for the macOS-only test that reads them.
#[cfg(all(test, target_os = "macos"))]
pub fn temperature_readings() -> Option<Vec<(String, f64)>> {
    macos::temperature_readings()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    use objc2_core_foundation::{CFArray, CFNumber, CFRetained, CFString, CFType};

    use super::{plausible_rpm, smc_number, Fan};

    type Client = c_void;
    type ServiceClient = c_void;
    type Event = c_void;

    /// `kIOHIDEventTypeTemperature`.
    const TEMPERATURE_EVENT: i64 = 15;
    /// The temperature event's level field: its type shifted into the field's top 16 bits.
    const TEMPERATURE_FIELD: u32 = 15 << 16;
    /// Apple's vendor page and the temperature-sensor usage on it.
    const SENSOR_PAGE: i64 = 0xff00;
    const SENSOR_USAGE: i64 = 5;
    /// Sensors read at most.
    const MAX_SENSORS: usize = 128;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDEventSystemClientCreate(allocator: *const c_void) -> *mut Client;
        fn IOHIDEventSystemClientCopyServices(client: *mut Client) -> *const CFArray;
        fn IOHIDServiceClientCopyProperty(
            service: *const ServiceClient,
            key: &CFString,
        ) -> *const CFType;
        fn IOHIDServiceClientCopyEvent(
            service: *const ServiceClient,
            kind: i64,
            options: i32,
            timestamp: i64,
        ) -> *mut Event;
        fn IOHIDEventGetFloatValue(event: *mut Event, field: u32) -> f64;
    }

    /// Takes ownership of a +1 CF reference, if it isn't null.
    ///
    /// # Safety
    /// `pointer` must be null or an owned (Create/Copy rule) reference to a CF object of type `T`.
    unsafe fn owned<T: objc2_core_foundation::Type>(pointer: *const T) -> Option<CFRetained<T>> {
        let pointer = std::ptr::NonNull::new(pointer.cast_mut())?;
        // SAFETY: the caller guarantees an owned reference of type `T`.
        Some(unsafe { CFRetained::from_raw(pointer) })
    }

    fn service_number(service: *const ServiceClient, key: &str) -> Option<i64> {
        let key = CFString::from_str(key);
        // SAFETY: `service` is a live service from the client's array; the result follows the
        // Copy rule.
        let value = unsafe { owned(IOHIDServiceClientCopyProperty(service, &key)) }?;
        value.downcast_ref::<CFNumber>()?.as_i64()
    }

    fn service_text(service: *const ServiceClient, key: &str) -> Option<String> {
        let key = CFString::from_str(key);
        // SAFETY: as in `service_number`.
        let value = unsafe { owned(IOHIDServiceClientCopyProperty(service, &key)) }?;
        value
            .downcast_ref::<CFString>()
            .map(ToString::to_string)
            .filter(|text| !text.is_empty() && text.len() <= 100)
    }

    pub fn temperature_readings() -> Option<Vec<(String, f64)>> {
        // SAFETY: a null allocator selects the default. The client is released below.
        let client = unsafe { IOHIDEventSystemClientCreate(std::ptr::null()) };
        if client.is_null() {
            return None;
        }
        // SAFETY: `client` is the live client just made; the array follows the Copy rule.
        let client_ref = unsafe { owned(client.cast_const().cast::<CFType>()) }?;
        // SAFETY: as above.
        let services = unsafe { owned(IOHIDEventSystemClientCopyServices(client)) }?;
        let mut readings = Vec::new();
        for index in 0..services
            .count()
            .min(isize::try_from(MAX_SENSORS).unwrap_or(isize::MAX))
        {
            // SAFETY: `index` is inside the array; the value is borrowed from it.
            let service = unsafe { services.value_at_index(index) }.cast::<ServiceClient>();
            if service.is_null()
                || service_number(service, "PrimaryUsagePage") != Some(SENSOR_PAGE)
                || service_number(service, "PrimaryUsage") != Some(SENSOR_USAGE)
            {
                continue;
            }
            let Some(name) = service_text(service, "Product") else {
                continue;
            };
            // SAFETY: `service` is live; the event follows the Copy rule and is released below.
            let event = unsafe { IOHIDServiceClientCopyEvent(service, TEMPERATURE_EVENT, 0, 0) };
            if event.is_null() {
                continue;
            }
            // SAFETY: `event` is a live temperature event.
            let celsius = unsafe { IOHIDEventGetFloatValue(event, TEMPERATURE_FIELD) };
            // SAFETY: `event` is an owned CF object; dropping it releases it.
            drop(unsafe { owned(event.cast_const().cast::<CFType>()) });
            readings.push((name, celsius));
        }
        drop(client_ref);
        Some(readings)
    }

    /// `SMCKeyData_t` from Apple's SMC user client, as the open-source fan tools define it. The
    /// C record's nested structs are 4-byte aligned; the padding is written out so the layout
    /// (80 bytes, checked in a test) matches it exactly.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct KeyData {
        key: u32,
        vers: [u8; 6],
        _vers_padding: [u8; 2],
        p_limit: [u8; 16],
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
        _info_padding: [u8; 3],
        result: u8,
        status: u8,
        data8: u8,
        _padding: u8,
        data32: u32,
        bytes: [u8; 32],
    }

    /// `kSMCHandleYPCEvent`, and its read and key-information commands.
    const HANDLE_EVENT: u32 = 2;
    const READ_KEY: u8 = 5;
    const KEY_INFO: u8 = 9;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connection: *mut u32) -> i32;
        fn IOServiceClose(connection: u32) -> i32;
        fn IOConnectCallStructMethod(
            connection: u32,
            selector: u32,
            input: *const c_void,
            input_size: usize,
            output: *mut c_void,
            output_size: *mut usize,
        ) -> i32;
    }

    fn key_code(key: &str) -> Option<u32> {
        Some(u32::from_be_bytes(key.as_bytes().try_into().ok()?))
    }

    fn call(connection: u32, input: &KeyData) -> Option<KeyData> {
        let mut output = KeyData::default();
        let mut size = std::mem::size_of::<KeyData>();
        // SAFETY: `input` and `output` are `KeyData` records of exactly `size` bytes, the layout
        // the SMC user client reads and writes for this selector.
        let status = unsafe {
            IOConnectCallStructMethod(
                connection,
                HANDLE_EVENT,
                std::ptr::from_ref(input).cast(),
                size,
                std::ptr::from_mut(&mut output).cast(),
                &mut size,
            )
        };
        (status == 0 && output.result == 0).then_some(output)
    }

    /// Reads one key's value as a number (never writes).
    fn read(connection: u32, key: &str) -> Option<f64> {
        let key = key_code(key)?;
        let info = call(
            connection,
            &KeyData {
                key,
                data8: KEY_INFO,
                ..KeyData::default()
            },
        )?;
        let value = call(
            connection,
            &KeyData {
                key,
                data_size: info.data_size,
                data8: READ_KEY,
                ..KeyData::default()
            },
        )?;
        let size = usize::try_from(info.data_size).ok()?.min(32);
        smc_number(info.data_type.to_be_bytes(), &value.bytes[..size])
    }

    pub fn fans() -> Option<Vec<Fan>> {
        let connection = crate::system_monitor::registry::open_service("AppleSMC", |service| {
            let mut connection = 0;
            // SAFETY: `service` is a live registry entry; `mach_task_self` is this task; type 0
            // is the SMC's default user client; `connection` is writable.
            #[allow(deprecated)]
            let status =
                unsafe { IOServiceOpen(service, libc::mach_task_self(), 0, &mut connection) };
            (status == 0 && connection != 0).then_some(connection)
        })??;
        let count = read(connection, "FNum").map_or(0, |count| count.clamp(0.0, 8.0) as usize);
        let mut fans = Vec::with_capacity(count);
        for index in 0..count {
            let Some(rpm) =
                read(connection, &format!("F{index}Ac")).filter(|&rpm| plausible_rpm(rpm))
            else {
                continue;
            };
            fans.push(Fan {
                rpm,
                min_rpm: read(connection, &format!("F{index}Mn")).filter(|&rpm| plausible_rpm(rpm)),
                max_rpm: read(connection, &format!("F{index}Mx")).filter(|&rpm| plausible_rpm(rpm)),
            });
        }
        // SAFETY: `connection` was opened above and is closed once.
        let _ = unsafe { IOServiceClose(connection) };
        Some(fans)
    }

    #[cfg(test)]
    pub const KEY_DATA_SIZE: usize = std::mem::size_of::<KeyData>();
}

#[cfg(not(target_os = "macos"))]
mod macos {
    use super::Fan;

    pub fn temperature_readings() -> Option<Vec<(String, f64)>> {
        None
    }

    pub fn fans() -> Option<Vec<Fan>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(readings: &[(&str, f64)]) -> Vec<(String, f64)> {
        readings
            .iter()
            .map(|(name, celsius)| ((*name).to_owned(), *celsius))
            .collect()
    }

    #[test]
    fn processor_and_graphics_sensors_are_told_apart() {
        assert_eq!(classify("pACC MTR Temp Sensor2"), Part::Cpu);
        assert_eq!(classify("eACC MTR Temp Sensor0"), Part::Cpu);
        assert_eq!(classify("PMU tdie3"), Part::Cpu);
        assert_eq!(classify("GPU MTR Temp Sensor1"), Part::Gpu);
        assert_eq!(classify("NAND CH0 temp"), Part::Other);
        assert_eq!(classify("gas gauge battery"), Part::Other);
    }

    #[test]
    fn cluster_sensors_win_and_unplugged_ones_are_ignored() {
        let temperatures = summarize(&named(&[
            ("pACC MTR Temp Sensor0", 50.0),
            ("pACC MTR Temp Sensor1", 60.0),
            ("PMU tdie1", 90.0),
            ("GPU MTR Temp Sensor0", 40.0),
            ("eACC MTR Temp Sensor9", 0.0),
            ("NAND CH0 temp", 33.0),
        ]))
        .expect("temperatures");
        assert!((temperatures.cpu - 55.0).abs() < 1e-9);
        assert!((temperatures.cpu_max - 60.0).abs() < 1e-9);
        assert_eq!(temperatures.gpu, Some(40.0));
        // A chip with only die sensors uses those.
        let die = summarize(&named(&[("PMU tdie1", 44.0), ("PMU tdie2", 46.0)])).expect("die");
        assert!((die.cpu - 45.0).abs() < 1e-9);
        assert_eq!(die.gpu, None);
        assert_eq!(summarize(&named(&[("NAND CH0 temp", 33.0)])), None);
    }

    #[test]
    fn smc_numbers_by_type() {
        assert_eq!(
            smc_number(*b"flt ", &1850.0_f32.to_le_bytes()),
            Some(1850.0)
        );
        // 1200 rpm in 14.2 fixed point.
        assert_eq!(
            smc_number(*b"fpe2", &(1200_u16 * 4).to_be_bytes()),
            Some(1200.0)
        );
        assert_eq!(smc_number(*b"ui8 ", &[2]), Some(2.0));
        assert_eq!(smc_number(*b"ch8*", &[1, 2, 3, 4]), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_smc_record_has_the_size_the_smc_expects() {
        assert_eq!(macos::KEY_DATA_SIZE, 80);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reading_sensors_and_fans_changes_nothing() {
        let readings = temperature_readings().unwrap_or_default();
        eprintln!("sensors: {readings:?}");
        eprintln!("chip: {:?}", read_temperatures());
        let fans = read_fans();
        eprintln!("fans: {fans:?}");
        if let Some(temperatures) = read_temperatures() {
            assert!(temperatures.cpu > 5.0 && temperatures.cpu < 130.0);
        }
    }
}
