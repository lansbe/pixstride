//! Bounded overload response for the local compatibility server.
//!
//! Sending a response and immediately dropping a socket with unread request
//! bytes can replace a clean EOF with a TCP reset. Reject off the accept loop,
//! half-close the response, and drain a bounded amount of input for a bounded
//! time. Only a small fixed number of reject threads may exist concurrently;
//! additional connections are closed without promising an HTTP response.

use std::{
    io::Read,
    net::Shutdown,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const MAX_REJECTIONS: usize = 4;
const MAX_DRAIN: usize = 16 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::Release);
    }
}

/// Respond to a rejected connection without blocking the listener. Saturation
/// beyond the rejection budget is handled by closing the extra connection.
pub fn reject(socket: Box<dyn super::socket::Socket>) {
    if ACTIVE
        .try_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n < MAX_REJECTIONS {
                Some(n + 1)
            } else {
                None
            }
        })
        .is_err()
    {
        return;
    }
    let permit = Permit;
    // If thread creation fails, dropping the closure also releases the permit.
    let _ = std::thread::Builder::new()
        .name("image-busy".into())
        .spawn(move || {
            let _permit = permit;
            respond(socket);
        });
}

fn respond(mut socket: Box<dyn super::socket::Socket>) {
    let _ = socket.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = crate::protocol::text(socket.as_mut(), 503, "Request admission is full");
    let _ = socket.shutdown(Shutdown::Write);
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    let mut remaining = MAX_DRAIN;
    let mut buffer = [0u8; 2048];
    while remaining > 0 {
        let time_left = deadline.saturating_duration_since(Instant::now());
        if time_left.is_zero() {
            break;
        }
        if socket.set_read_timeout(Some(time_left)).is_err() {
            break;
        }
        let limit = buffer.len().min(remaining);
        match socket.read(&mut buffer[..limit]) {
            Ok(0) => break,
            Ok(n) => remaining -= n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}
