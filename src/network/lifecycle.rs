//! Signal-aware socket admission without a polling delay on ready connections.
use std::{
    os::fd::AsRawFd,
    sync::atomic::{AtomicBool, Ordering},
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
#[cfg(target_os = "linux")]
type Count = std::ffi::c_ulong;
#[cfg(target_os = "macos")]
type Count = std::ffi::c_uint;
unsafe extern "C" {
    fn signal(number: i32, handler: extern "C" fn(i32)) -> usize;
    fn poll(fds: *mut PollFd, count: Count, timeout: i32) -> i32;
}
pub fn initialize() -> Result<(), crate::Error> {
    // Only lock-free atomic stores run in the signal handler.
    if unsafe { signal(15, stop) } == usize::MAX || unsafe { signal(2, stop) } == usize::MAX {
        return Err("cannot install shutdown handlers");
    }
    Ok(())
}
pub fn stopping() -> bool {
    STOP.load(Ordering::Relaxed)
}
pub fn wait(listener: &impl AsRawFd) -> Result<bool, crate::Error> {
    while !STOP.load(Ordering::Relaxed) {
        let mut fd = PollFd {
            fd: listener.as_raw_fd(),
            events: 1,
            revents: 0,
        };
        let ready = unsafe { poll(&mut fd, 1, 100) };
        if ready > 0 {
            if fd.revents & 1 != 0 {
                return Ok(true);
            }
            return Err("listener failed");
        }
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err("listener poll failed");
        }
    }
    Ok(false)
}
