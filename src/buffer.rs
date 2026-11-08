use crate::Error;
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};
const MAX_BUFFER: usize = 512 * 1024 * 1024;
pub struct Buffer {
    ptr: NonNull<u8>,
    len: usize,
    allocated: usize,
}

impl Buffer {
    pub fn new(len: usize) -> Result<Self, Error> {
        if len > MAX_BUFFER || len > isize::MAX as usize {
            return Err("memory allocation exceeds limit");
        }
        let ptr = if len == 0 {
            // A zero-length slice still requires a non-null aligned pointer.
            // No allocation or deallocation is performed for this sentinel.
            NonNull::<u128>::dangling().cast()
        } else {
            let layout = Layout::from_size_align(len, 16).map_err(|_| "invalid memory layout")?;
            // Nonzero, bounded layout. A null result is handled as a normal
            // error, and Drop uses the identical layout for deallocation.
            NonNull::new(unsafe { alloc_zeroed(layout) }).ok_or("memory allocation failed")?
        };
        Ok(Self {
            ptr,
            len,
            allocated: len,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn as_slice(&self) -> &[u8] {
        // All len bytes are initialized. The empty sentinel is never read.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Shorten the visible initialized bytes without changing allocation
    /// ownership or alignment. Drop still uses the original allocation size.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if self.allocated != 0 {
            // The successful constructor established these layout invariants.
            let layout = unsafe { Layout::from_size_align_unchecked(self.allocated, 16) };
            unsafe { dealloc(self.ptr.as_ptr(), layout) };
        }
    }
}

impl Deref for Buffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}
