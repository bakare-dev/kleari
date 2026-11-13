//! Read-only `sysctlbyname` lookups for hardware and kernel values that `sysinfo` does not
//! expose (chip name, core clusters, memory pressure). No privileges are needed.

/// Reads a string value such as `machdep.cpu.brand_string`.
#[cfg(target_os = "macos")]
pub fn string(name: &str) -> Option<String> {
    let bytes = raw(name)?;
    // Values are NUL-terminated C strings; take the text before the first NUL.
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let text = String::from_utf8(bytes[..end].to_vec()).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Reads an integer value of 4 or 8 bytes, such as `hw.memsize` or `hw.physicalcpu`.
#[cfg(target_os = "macos")]
pub fn integer(name: &str) -> Option<u64> {
    let bytes = raw(name)?;
    match bytes.len() {
        4 => Some(u64::from(u32::from_ne_bytes(bytes.try_into().ok()?))),
        8 => Some(u64::from_ne_bytes(bytes.try_into().ok()?)),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn raw(name: &str) -> Option<Vec<u8>> {
    use std::ffi::CString;
    use std::ptr;

    let name = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;
    // SAFETY: `name` is a valid NUL-terminated string; a null `oldp` asks only for the size.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            ptr::null_mut(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    // A size is never huge for these keys; refuse anything unexpected rather than allocate it.
    if status != 0 || size == 0 || size > 4096 {
        return None;
    }
    let mut buffer = vec![0_u8; size];
    // SAFETY: `buffer` has `size` writable bytes and `size` tells sysctl that capacity; sysctl
    // writes at most that many bytes and updates `size` to the number written.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    buffer.truncate(size);
    Some(buffer)
}

#[cfg(not(target_os = "macos"))]
pub fn string(_name: &str) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn integer(_name: &str) -> Option<u64> {
    None
}
