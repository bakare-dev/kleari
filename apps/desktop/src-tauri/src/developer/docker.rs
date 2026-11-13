//! Docker's disk use for the Developer page, read-only (ADR-005, ADR-011). While Docker is
//! running, Kleari asks its engine one fixed question over the local socket,
//! `GET /system/df`, which only reports usage. When it isn't running, only the size of Docker
//! Desktop's disk image is shown. Kleari never sends any other request and never runs a
//! Docker command; the response is untrusted and bounded.
//!
//! The exchange runs on the scan's own thread under one absolute deadline for both sockets.
//! The connect is non-blocking and reads use a short timeout, so the deadline and
//! cancellation are checked every 100 ms and nothing is left waiting once the scan moves on.

use std::ffi::CString;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::measure::{open_absolute, open_beneath};
use crate::system_monitor::non_local_mounts;

const REQUEST: &[u8] =
    b"GET /system/df HTTP/1.0\r\nHost: docker\r\nAccept: application/json\r\n\r\n";
/// The whole exchange, over every socket tried. `system/df` measures volumes, which can
/// take a few seconds.
const LIMIT: Duration = Duration::from_secs(10);
const WRITE_LIMIT: Duration = Duration::from_secs(1);
/// How often the deadline and cancellation are checked while waiting.
const POLL: Duration = Duration::from_millis(100);
const MAX_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerUsage {
    pub images_bytes: u64,
    pub build_cache_bytes: u64,
    pub volumes_bytes: u64,
    pub containers_bytes: u64,
    /// Every figure was reported; otherwise the totals are a minimum (a section missing,
    /// or sizes Docker didn't compute).
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DockerState {
    /// Neither Docker Desktop nor its data was found.
    NotInstalled,
    /// Installed but its engine didn't answer; the disk image's size, when found.
    #[serde(rename_all = "camelCase")]
    NotRunning { disk_image_bytes: Option<u64> },
    #[serde(rename_all = "camelCase")]
    Running {
        usage: DockerUsage,
        disk_image_bytes: Option<u64>,
    },
    /// The scan was cancelled before Docker was asked or answered.
    NotChecked,
}

/// Sums one `system/df` section. Docker writes an empty section as `null`. Returns the total
/// and whether every entry had a size; a missing section is `None`.
fn sum(
    section: Option<&serde_json::Value>,
    size: impl Fn(&serde_json::Value) -> Option<Option<u64>>,
) -> Option<(u64, bool)> {
    let section = section?;
    if section.is_null() {
        return Some((0, true));
    }
    let mut total = 0_u64;
    let mut complete = true;
    for item in section.as_array()? {
        match size(item) {
            // Not counted by design (shared build-cache records).
            None => {}
            Some(Some(bytes)) => total = total.saturating_add(bytes),
            Some(None) => complete = false,
        }
    }
    Some((total, complete))
}

fn non_negative(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(serde_json::Value::as_u64)
}

/// The usage in a `system/df` body. Image layers are counted once (`LayersSize`); a volume
/// whose size Docker didn't compute (`-1`) or a missing section makes the usage incomplete
/// rather than zero; shared build-cache records aren't counted twice. A body with none of
/// the expected fields isn't a `system/df` answer and gives `None`.
pub fn parse_usage(body: &[u8]) -> Option<DockerUsage> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let layers = object
        .get("LayersSize")
        .map(|value| non_negative(Some(value)));
    let build_cache = sum(object.get("BuildCache"), |record| {
        (record.get("Shared").and_then(serde_json::Value::as_bool) != Some(true))
            .then(|| non_negative(record.get("Size")))
    });
    let volumes = sum(object.get("Volumes"), |volume| {
        Some(non_negative(
            volume.get("UsageData").and_then(|usage| usage.get("Size")),
        ))
    });
    let containers = sum(object.get("Containers"), |container| {
        Some(non_negative(container.get("SizeRw")))
    });
    if layers.is_none() && build_cache.is_none() && volumes.is_none() && containers.is_none() {
        return None;
    }
    let part = |section: Option<(u64, bool)>| section.unwrap_or((0, false));
    let (build_cache_bytes, build_cache_complete) = part(build_cache);
    let (volumes_bytes, volumes_complete) = part(volumes);
    let (containers_bytes, containers_complete) = part(containers);
    let images_bytes = layers.flatten();
    Some(DockerUsage {
        images_bytes: images_bytes.unwrap_or(0),
        build_cache_bytes,
        volumes_bytes,
        containers_bytes,
        complete: images_bytes.is_some()
            && build_cache_complete
            && volumes_complete
            && containers_complete,
    })
}

/// The body of a `200 OK` HTTP/1.x response; anything else is `None`.
pub fn ok_body(response: &[u8]) -> Option<&[u8]> {
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&response[..end]).ok()?;
    let status = head.lines().next()?;
    let mut parts = status.split_whitespace();
    let version = parts.next()?;
    if !version.starts_with("HTTP/1.") || parts.next()? != "200" {
        return None;
    }
    // A chunked body isn't expected for an HTTP/1.0 request; refuse rather than misparse.
    if head.lines().any(|line| {
        line.to_ascii_lowercase().starts_with("transfer-encoding:")
            && line.to_ascii_lowercase().contains("chunked")
    }) {
        return None;
    }
    Some(&response[end + 4..])
}

/// Time left before `deadline`, or `None` once it has passed or the scan is cancelled.
fn remaining(deadline: Instant, cancel: &AtomicBool) -> Option<Duration> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
}

/// Waits until a non-blocking connect on `fd` finishes, in `POLL` steps.
fn wait_connected(fd: libc::c_int, deadline: Instant, cancel: &AtomicBool) -> Option<()> {
    loop {
        let wait = remaining(deadline, cancel)?.min(POLL);
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let millis = libc::c_int::try_from(wait.as_millis())
            .unwrap_or(libc::c_int::MAX)
            .max(1);
        // SAFETY: `poll` is one valid, writable pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&raw mut poll, 1, millis) };
        if ready > 0 {
            return Some(());
        }
        if ready < 0 && std::io::Error::last_os_error().kind() != ErrorKind::Interrupted {
            return None;
        }
    }
}

/// What a failed non-blocking `connect` means for the next step.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// The connection is under way: wait until the socket is writable, then read `SO_ERROR`.
    Wait,
    /// Linux: the listener's backlog is full (`EAGAIN`). Nothing is pending, so try again.
    Retry,
    Fail,
}

fn after_connect_error(errno: Option<libc::c_int>) -> Attempt {
    match errno {
        Some(libc::EINPROGRESS | libc::EINTR) => Attempt::Wait,
        Some(libc::EAGAIN) => Attempt::Retry,
        _ => Attempt::Fail,
    }
}

/// The socket's pending error (`SO_ERROR`), 0 once a connection has completed.
fn connect_error(fd: libc::c_int) -> Option<libc::c_int> {
    let mut error: libc::c_int = 0;
    let mut size = libc::socklen_t::try_from(std::mem::size_of::<libc::c_int>()).ok()?;
    // SAFETY: `error` and `size` are valid, writable and sized for SO_ERROR.
    let read = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&raw mut error).cast::<libc::c_void>(),
            &raw mut size,
        )
    };
    (read == 0).then_some(error)
}

/// Connects to the Unix socket at `path` without ever blocking: the socket is non-blocking
/// while connecting, so an engine that never accepts (a full backlog) fails or is waited on
/// only until `deadline` or cancellation. `std` has no connect timeout for Unix sockets.
fn connect(path: &Path, deadline: Instant, cancel: &AtomicBool) -> Option<UnixStream> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `sockaddr_un` is plain data; all zeroes is a valid, empty address.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    // The zeroed storage supplies the terminating NUL, so the path must leave room for it.
    if bytes.is_empty() || bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return None;
    }
    address.sun_family = libc::sa_family_t::try_from(libc::AF_UNIX).ok()?;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = u8::try_from(std::mem::size_of::<libc::sockaddr_un>()).ok()?;
    }
    for (slot, &byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = libc::c_char::from_ne_bytes([byte]);
    }
    let length = libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_un>()).ok()?;
    // SAFETY: a plain socket(2) call with constant arguments.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return None;
    }
    // SAFETY: `raw` was just returned by socket(2) and nothing else owns it.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: `raw` is an open descriptor owned by `fd` for these calls.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    // SAFETY: as above.
    if flags < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
    {
        return None;
    }
    loop {
        // SAFETY: `address` is an initialized `sockaddr_un` and `length` is its size.
        let result =
            unsafe { libc::connect(raw, (&raw const address).cast::<libc::sockaddr>(), length) };
        if result == 0 {
            break;
        }
        match after_connect_error(std::io::Error::last_os_error().raw_os_error()) {
            Attempt::Wait => {
                wait_connected(raw, deadline, cancel)?;
                connect_error(raw).filter(|error| *error == 0)?;
                break;
            }
            Attempt::Retry => std::thread::sleep(remaining(deadline, cancel)?.min(POLL)),
            Attempt::Fail => return None,
        }
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false).ok()?;
    Some(stream)
}

/// Sends the request and reads the whole response, giving up at `deadline` or as soon as the
/// scan is cancelled. A response over the size cap is refused.
fn exchange(socket: &Path, deadline: Instant, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let mut stream = connect(socket, deadline, cancel)?;
    let left = remaining(deadline, cancel)?;
    stream.set_write_timeout(Some(left.min(WRITE_LIMIT))).ok()?;
    stream.set_read_timeout(Some(POLL)).ok()?;
    stream.write_all(REQUEST).ok()?;
    let mut response = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        remaining(deadline, cancel)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Some(response),
            Ok(read) => {
                if response.len() + read > MAX_RESPONSE {
                    return None;
                }
                response.extend_from_slice(&buffer[..read]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
    }
}

/// Asks one socket, waiting at most until `deadline` and no longer than the scan runs.
fn ask(socket: &Path, deadline: Instant, cancel: &AtomicBool) -> Option<DockerUsage> {
    parse_usage(ok_body(&exchange(socket, deadline, cancel)?)?)
}

/// `path` is outside every mount the mount table reports as non-local; `false` if the table
/// can't be read. Sound on the lexical path because every open below refuses symlinks.
fn local(path: &Path) -> bool {
    non_local_mounts().is_some_and(|mounts| !mounts.iter().any(|mount| path.starts_with(mount)))
}

/// Whether an entry exists at `path`, looked up through directory handles from `/` with no
/// symlink on the way (the entry itself isn't followed) and never under a non-local mount.
fn present(path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    if !local(path) {
        return false;
    }
    let Some((dir, _)) = open_absolute(parent) else {
        return false;
    };
    CString::new(name.as_bytes())
        .ok()
        .is_some_and(|name| dir.stat_child(&name).is_ok())
}

/// Docker Desktop's disk image, measured by its allocated blocks without following symlinks
/// anywhere on the path, on the home folder's volume, never under a non-local mount.
fn disk_image_bytes(home: &Path) -> Option<u64> {
    if !local(home) {
        return None;
    }
    let (home_dir, dev) = open_absolute(home)?;
    let dir = open_beneath(
        &home_dir,
        dev,
        Path::new("Library/Containers/com.docker.docker/Data/vms/0/data"),
    )?;
    let stat = dir.stat_child(c"Docker.raw").ok()?;
    (!stat.is_dir() && !stat.is_symlink() && stat.dev == dev)
        .then(|| stat.blocks.saturating_mul(512))
}

/// The engine sockets Docker Desktop creates, in the order tried.
fn sockets(home: &Path) -> [PathBuf; 2] {
    [
        home.join(".docker/run/docker.sock"),
        PathBuf::from("/var/run/docker.sock"),
    ]
}

pub fn read(home: Option<&Path>, cancel: &AtomicBool) -> DockerState {
    let Some(home) = home else {
        return DockerState::NotInstalled;
    };
    state(
        home,
        &sockets(home),
        Path::new("/Applications/Docker.app"),
        LIMIT,
        cancel,
    )
}

fn state(
    home: &Path,
    sockets: &[PathBuf],
    app: &Path,
    limit: Duration,
    cancel: &AtomicBool,
) -> DockerState {
    if cancel.load(Ordering::Relaxed) {
        return DockerState::NotChecked;
    }
    let disk_image = disk_image_bytes(home);
    let deadline = Instant::now() + limit;
    let usage = sockets
        .iter()
        .filter(|socket| local(socket))
        .find_map(|socket| ask(socket, deadline, cancel));
    // A cancelled exchange says nothing about whether Docker is running.
    if cancel.load(Ordering::Relaxed) {
        return DockerState::NotChecked;
    }
    if let Some(usage) = usage {
        return DockerState::Running {
            usage,
            disk_image_bytes: disk_image,
        };
    }
    let installed = disk_image.is_some()
        || present(app)
        || present(&home.join("Library/Containers/com.docker.docker"));
    if installed {
        DockerState::NotRunning {
            disk_image_bytes: disk_image,
        }
    } else {
        DockerState::NotInstalled
    }
}
