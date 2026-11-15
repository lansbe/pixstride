//! Owned nonblocking native-worker pipes share one absolute exchange deadline.
use crate::Error;
use std::{
    io::{ErrorKind, Read, Write},
    os::fd::AsRawFd,
    time::Instant,
};

pub(super) fn nonblocking(pipe: &impl AsRawFd) -> Result<(), Error> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fd belongs to the live pipe; preserve all flags except O_NONBLOCK.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err("cannot configure native pipe");
    }
    Ok(())
}

fn remaining(deadline: Instant) -> Result<std::time::Duration, Error> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or("native processing deadline exceeded")
}

fn wait(pipe: &impl AsRawFd, events: i16, deadline: Instant) -> Result<(), Error> {
    loop {
        let milliseconds = remaining(deadline)?
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd: pipe.as_raw_fd(),
            events,
            revents: 0,
        };
        // SAFETY: one valid descriptor, bounded timeout, no shared mutable poll state.
        let ready = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
        if ready > 0 {
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err("invalid native pipe");
            }
            // HUP/ERR are handled by the next read/write; HUP may retain readable bytes.
            return Ok(());
        }
        if ready < 0 && std::io::Error::last_os_error().kind() != ErrorKind::Interrupted {
            return Err("native pipe wait failed");
        }
    }
}

pub(super) fn write_all(
    pipe: &mut (impl Write + AsRawFd),
    mut bytes: &[u8],
    deadline: Instant,
) -> Result<(), Error> {
    while !bytes.is_empty() {
        remaining(deadline)?;
        match pipe.write(bytes) {
            Ok(0) => return Err("native input rejected"),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => wait(pipe, libc::POLLOUT, deadline)?,
            Err(_) => return Err("native input rejected"),
        }
    }
    Ok(())
}

pub(super) fn read_exact(
    pipe: &mut (impl Read + AsRawFd),
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), Error> {
    while !bytes.is_empty() {
        remaining(deadline)?;
        match pipe.read(bytes) {
            Ok(0) => return Err("native worker closed output"),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => wait(pipe, libc::POLLIN, deadline)?,
            Err(_) => return Err("native frame read failed"),
        }
    }
    Ok(())
}

pub(super) fn frame(
    pipe: &mut (impl Read + AsRawFd),
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>, Error> {
    let mut size = [0; 8];
    read_exact(pipe, &mut size, deadline)?;
    let size = u64::from_le_bytes(size);
    if size > limit as u64 {
        return Err("native frame exceeds limit");
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size as usize)
        .map_err(|_| "native frame allocation failed")?;
    bytes.resize(size as usize, 0);
    read_exact(pipe, &mut bytes, deadline)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        net::Shutdown,
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };

    #[test]
    fn transfers_more_than_the_pipe_capacity_without_helper_io_threads() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        nonblocking(&sender).unwrap();
        nonblocking(&receiver).unwrap();
        assert_ne!(
            unsafe { libc::fcntl(sender.as_raw_fd(), libc::F_GETFL) } & libc::O_NONBLOCK,
            0
        );
        let expected: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
        let deadline = Instant::now() + Duration::from_secs(3);
        let reader = std::thread::spawn(move || {
            let mut actual = vec![0; 1024 * 1024];
            read_exact(&mut receiver, &mut actual, deadline).unwrap();
            actual
        });
        write_all(&mut sender, &expected, deadline).unwrap();
        assert_eq!(reader.join().unwrap(), expected);
    }

    #[test]
    fn a_silent_peer_and_a_blocked_writer_both_obey_the_deadline() {
        let (mut sender, _silent) = UnixStream::pair().unwrap();
        nonblocking(&sender).unwrap();
        let start = Instant::now();
        assert_eq!(
            read_exact(&mut sender, &mut [0; 1], start + Duration::from_millis(30)),
            Err("native processing deadline exceeded")
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        let start = Instant::now();
        assert_eq!(
            write_all(
                &mut sender,
                &vec![1; 4 * 1024 * 1024],
                start + Duration::from_millis(30)
            ),
            Err("native processing deadline exceeded")
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn eof_in_the_middle_of_a_frame_is_an_error() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        nonblocking(&receiver).unwrap();
        sender.write_all(&[1, 2]).unwrap();
        sender.shutdown(Shutdown::Write).unwrap();
        assert_eq!(
            read_exact(
                &mut receiver,
                &mut [0; 3],
                Instant::now() + Duration::from_secs(1)
            ),
            Err("native worker closed output")
        );
    }
}
