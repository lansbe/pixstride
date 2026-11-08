//! Bounded reads of administrator-selected regular files.
use crate::{Buffer, Error};
use std::{
    fs::{File, OpenOptions},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
};
pub struct InputFile {
    file: File,
    len: usize,
}
impl InputFile {
    pub fn open(path: &[u8]) -> Result<Self, Error> {
        let path = Path::new(std::ffi::OsStr::from_bytes(path));
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(crate::local::O_NOFOLLOW | crate::local::O_NONBLOCK)
            .open(path)
            .map_err(|_| "cannot open source file")?;
        let meta = file.metadata().map_err(|_| "cannot inspect source file")?;
        if !meta.is_file() || meta.len() > crate::MAX_FILE as u64 {
            return Err("invalid or oversized source file");
        }
        Ok(Self {
            file,
            len: meta.len() as usize,
        })
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn read_all(&self) -> Result<Buffer, Error> {
        crate::local::read_checked(
            self.file
                .try_clone()
                .map_err(|_| "cannot clone source handle")?,
            || {},
        )
        .map_err(|_| "cannot read source safely")
    }
}
