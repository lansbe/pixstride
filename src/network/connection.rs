//! Preserve pipelined bytes between bounded HTTP requests on one socket.
use super::socket::Socket;
use std::{
    io::{self, IoSlice, Read, Write},
    net::Shutdown,
    time::{Duration, Instant},
};
pub(super) struct Connection {
    socket: Box<dyn Socket>,
    bytes: [u8; 2048],
    start: usize,
    end: usize,
    matched: usize,
    complete: bool,
}
impl Connection {
    pub fn new(socket: Box<dyn Socket>) -> Self {
        Self {
            socket,
            bytes: [0; 2048],
            start: 0,
            end: 0,
            matched: 0,
            complete: false,
        }
    }
    pub fn next(&mut self, idle: Duration) -> bool {
        if super::lifecycle::stopping() {
            return false;
        }
        self.matched = 0;
        self.complete = false;
        let deadline = Instant::now() + idle;
        while self.start == self.end {
            if super::lifecycle::stopping() || Instant::now() >= deadline {
                return false;
            }
            if self
                .socket
                .set_read_timeout(Some(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(100)),
                ))
                .is_err()
            {
                return false;
            }
            match self.socket.read(&mut self.bytes) {
                Ok(0) => return false,
                Ok(n) => {
                    self.start = 0;
                    self.end = n;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(_) => return false,
            }
        }
        true
    }
}
impl Read for Connection {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.complete {
            return Ok(0);
        }
        if self.start == self.end {
            self.end = self.socket.read(&mut self.bytes)?;
            self.start = 0;
        }
        let mut count = 0;
        while count < out.len() && self.start < self.end {
            let byte = self.bytes[self.start];
            self.start += 1;
            out[count] = byte;
            count += 1;
            self.matched = if byte == b"\r\n\r\n"[self.matched] {
                self.matched + 1
            } else {
                usize::from(byte == b'\r')
            };
            if self.matched == 4 {
                self.complete = true;
                break;
            }
        }
        Ok(count)
    }
}
impl Write for Connection {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.socket.write(b)
    }
    fn write_vectored(&mut self, b: &[IoSlice<'_>]) -> io::Result<usize> {
        self.socket.write_vectored(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
impl Socket for Connection {
    fn set_read_timeout(&self, v: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(v)
    }
    fn set_write_timeout(&self, v: Option<Duration>) -> io::Result<()> {
        self.socket.set_write_timeout(v)
    }
    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        self.socket.write_timeout()
    }
    fn shutdown(&self, v: Shutdown) -> io::Result<()> {
        self.socket.shutdown(v)
    }
    fn set_nodelay(&self, v: bool) -> io::Result<()> {
        self.socket.set_nodelay(v)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipelined_headers_and_fragmented_delimiters_stay_in_their_own_requests() {
        let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            client
                .write_all(b"GET /one HTTP/1.1\r\nHost: localhost:80\r\n\r")
                .unwrap();
            std::thread::sleep(Duration::from_millis(5));
            client
                .write_all(b"\nHEAD /two HTTP/1.1\r\nHost: localhost:80\r\n\r\n")
                .unwrap();
            client // Keep the receiving endpoint alive while both requests are parsed.
        });
        let mut connection = Connection::new(Box::new(server));
        let first = super::super::protocol::request_with(&mut connection, 80, true, |target, _| {
            target.to_owned()
        })
        .unwrap();
        assert_eq!(first, "/one");
        assert!(connection.next(Duration::from_secs(1)));
        let second =
            super::super::protocol::request_with(&mut connection, 80, true, |target, c| {
                (target.to_owned(), c.head)
            })
            .unwrap();
        assert_eq!(second, ("/two".into(), true));
        let _client = worker.join().unwrap();
    }
}
