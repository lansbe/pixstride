//! Local source root pinned by directory descriptor on supported Unix platforms.
//!
//! Names are flat and the final component is opened with `O_NOFOLLOW`. The
//! directory descriptor remains authoritative if the configured path is
//! renamed or replaced. Sources are read into one bounded private buffer.

use crate::Buffer;
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
};
use std::{
    ffi::{c_char, c_int, CString},
    fs::{File, Metadata, OpenOptions},
    io,
    path::Path,
};

#[cfg(target_os = "macos")]
pub(crate) const O_NONBLOCK: i32 = 0x4;
#[cfg(target_os = "macos")]
pub(crate) const O_NOFOLLOW: i32 = 0x100;
#[cfg(target_os = "macos")]
const O_DIRECTORY: i32 = 0x100000;
#[cfg(target_os = "macos")]
const O_CLOEXEC: i32 = 0x1000000;
#[cfg(target_os = "linux")]
pub(crate) const O_NONBLOCK: i32 = 0x800;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) const O_NOFOLLOW: i32 = 0x20000;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) const O_NOFOLLOW: i32 = 0x8000;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const O_DIRECTORY: i32 = 0x10000;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const O_DIRECTORY: i32 = 0x4000;
#[cfg(target_os = "linux")]
const O_CLOEXEC: i32 = 0x80000;
const O_RDONLY: i32 = 0;
const MAX_SOURCE: u64 = 64 * 1024 * 1024;
const MAX_SOURCE_NAME: usize = 12_000;

unsafe extern "C" {
    fn openat(fd: c_int, name: *const c_char, flags: c_int, ...) -> c_int;
}

pub struct LocalRoot {
    directory: File,
}

impl LocalRoot {
    pub fn open(path: &Path) -> Result<Self, String> {
        // Preserve configured-root symlink resolution at startup, then pin the
        // chosen directory. O_NOFOLLOW rejects a final-component substitution
        // occurring between canonicalization and this open.
        let canonical = std::fs::canonicalize(path).map_err(|_| "invalid local source root")?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(canonical)
            .map_err(|_| "cannot open local source root safely")?;
        if !directory
            .metadata()
            .map_err(|_| "cannot inspect local source root")?
            .is_dir()
        {
            return Err("local source root must be a directory".into());
        }
        Ok(Self { directory })
    }

    fn open_name(&self, source: &str) -> Result<File, String> {
        let name = source
            .strip_prefix("local:///")
            .ok_or("local source must start with local:///")?;
        if name.is_empty()
            || name.len() > MAX_SOURCE_NAME
            || name == "."
            || name == ".."
            || name
                .bytes()
                .any(|b| b <= 32 || b >= 127 || matches!(b, b'/' | b'\\' | b'?' | b'#'))
        {
            return Err("local source must be a flat filename".into());
        }
        let name = CString::new(name).map_err(|_| "invalid local source name")?;
        loop {
            // SAFETY: the directory remains open for the call, `name` is a
            // live NUL-terminated string with no embedded slash, and no mode
            // argument is needed because no creation flag is used.
            let fd = unsafe {
                openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC,
                )
            };
            if fd >= 0 {
                // SAFETY: a successful openat returns a new owned descriptor.
                return Ok(unsafe { File::from_raw_fd(fd) });
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err("file not found or cannot be opened safely".into());
            }
        }
    }

    pub fn read(&self, source: &str) -> Result<Buffer, String> {
        read_checked(self.open_name(source)?, || {})
    }
}

fn same_identity(before: &Metadata, after: &Metadata) -> bool {
    after.file_type().is_file()
        && after.dev() == before.dev()
        && after.ino() == before.ino()
        && after.len() == before.len()
        && after.mtime() == before.mtime()
        && after.mtime_nsec() == before.mtime_nsec()
        && after.ctime() == before.ctime()
        && after.ctime_nsec() == before.ctime_nsec()
}

// The hook is a zero-cost no-op in the normal call. Tests use it to create a
// deterministic mutation exactly after the initial metadata snapshot.
pub(crate) fn read_checked<F: FnOnce()>(file: File, after_snapshot: F) -> Result<Buffer, String> {
    let before = file.metadata().map_err(|_| "cannot inspect source file")?;
    if !before.file_type().is_file() {
        return Err("source is not a regular file".into());
    }
    if before.len() > MAX_SOURCE {
        return Err("source exceeds 64 MiB".into());
    }
    let len = usize::try_from(before.len()).map_err(|_| "source exceeds address space")?;
    after_snapshot();
    let mut result = Buffer::new(len)?;
    let mut used = 0usize;
    while used < len {
        match file.read_at(&mut result.as_mut_slice()[used..], used as u64) {
            Ok(0) => return Err("source file changed while reading".into()),
            Ok(n) => used = used.checked_add(n).ok_or("source read overflow")?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("source read failed".into()),
        }
    }
    let mut extra = [0u8; 1];
    loop {
        match file.read_at(&mut extra, len as u64) {
            Ok(0) => break,
            Ok(_) => return Err("source file changed while reading".into()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("source read failed".into()),
        }
    }
    let after = file.metadata().map_err(|_| "cannot inspect source file")?;
    if !same_identity(&before, &after) {
        return Err("source file changed while reading".into());
    }
    Ok(result)
}
