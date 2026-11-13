//! TCP/Unix listeners share the same deadlines and bounded HTTP path.
use std::{
    io::{self, Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::{MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    time::Duration,
};
pub(crate) trait Socket: Read + Write + Send {
    fn set_read_timeout(&self, value: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&self, value: Option<Duration>) -> io::Result<()>;
    fn write_timeout(&self) -> io::Result<Option<Duration>>;
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    fn set_nodelay(&self, value: bool) -> io::Result<()>;
}
macro_rules! socket_impl {
    ($ty:ty,$nodelay:expr) => {
        impl Socket for $ty {
            fn set_read_timeout(&self, v: Option<Duration>) -> io::Result<()> {
                <$ty>::set_read_timeout(self, v)
            }
            fn set_write_timeout(&self, v: Option<Duration>) -> io::Result<()> {
                <$ty>::set_write_timeout(self, v)
            }
            fn write_timeout(&self) -> io::Result<Option<Duration>> {
                <$ty>::write_timeout(self)
            }
            fn shutdown(&self, v: Shutdown) -> io::Result<()> {
                <$ty>::shutdown(self, v)
            }
            fn set_nodelay(&self, v: bool) -> io::Result<()> {
                ($nodelay)(self, v)
            }
        }
    };
}
socket_impl!(TcpStream, |s: &TcpStream, v| s.set_nodelay(v));
socket_impl!(UnixStream, |_: &UnixStream, _| Ok(()));
pub(crate) enum Listener {
    Tcp(TcpListener),
    Unix(UnixListener, PathBuf, u64, u64),
}
impl Listener {
    pub fn bind(ip: IpAddr, port: u16) -> io::Result<Self> {
        if let Some(path) = std::env::var_os("PIXSTRIDE_UNIX_SOCKET") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(io::Error::other("Unix socket must be absolute"));
            }
            let parent = std::fs::metadata(
                path.parent()
                    .ok_or_else(|| io::Error::other("socket parent required"))?,
            )?;
            // SAFETY: geteuid takes no pointers and returns this process's uid.
            if parent.uid() != unsafe { libc::geteuid() } || parent.mode() & 0o022 != 0 {
                return Err(io::Error::other(
                    "socket parent must be owned by this user and not writable by group or others",
                ));
            }
            let listener = UnixListener::bind(&path)?; // Never unlink an existing socket.
            let meta = std::fs::symlink_metadata(&path)?;
            let result = Self::Unix(listener, path, meta.dev(), meta.ino());
            if let Self::Unix(_, p, _, _) = &result {
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(result)
        } else {
            TcpListener::bind((ip, port)).map(Self::Tcp)
        }
    }
    pub fn set_nonblocking(&self, value: bool) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_nonblocking(value),
            Self::Unix(s, ..) => s.set_nonblocking(value),
        }
    }
    pub fn accept(&self) -> io::Result<Box<dyn Socket>> {
        match self {
            Self::Tcp(s) => {
                let (socket, _) = s.accept()?;
                socket.set_nonblocking(false)?;
                Ok(Box::new(socket))
            }
            Self::Unix(s, ..) => {
                let (socket, _) = s.accept()?;
                socket.set_nonblocking(false)?;
                Ok(Box::new(socket))
            }
        }
    }
}
impl AsRawFd for Listener {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            Self::Tcp(s) => s.as_raw_fd(),
            Self::Unix(s, ..) => s.as_raw_fd(),
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Self::Unix(_, p, dev, ino) = self {
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.dev() == *dev && m.ino() == *ino) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_connections_wait_for_fragmented_input_on_nonblocking_listeners() {
        let tcp = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(tcp.local_addr().unwrap()).unwrap();
        let listener = Listener::Tcp(tcp);
        listener.set_nonblocking(true).unwrap();
        let mut socket = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            client.write_all(b"GET").unwrap();
        });
        let mut input = [0; 3];
        socket.read_exact(&mut input).unwrap();
        assert_eq!(&input, b"GET");
        sender.join().unwrap();
    }
}
