//! Bounded lossless zlib compression using libdeflate's optimized implementation.
use crate::{Error, MAX_FILE};
use libdeflate_sys as ffi;
use std::ptr::NonNull;

const MAX_ZLIB: usize = MAX_FILE - 57;
struct Compressor(NonNull<ffi::libdeflate_compressor>);
impl Drop for Compressor {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns a live allocation from libdeflate.
        unsafe { ffi::libdeflate_free_compressor(self.0.as_ptr()) };
    }
}

pub(crate) fn compress(input: &[u8]) -> Result<Vec<u8>, Error> {
    compress_bounded(input, MAX_ZLIB)
}

fn compress_bounded(input: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    if input.len() > MAX_FILE || limit < 8 {
        return Err("PNG: data or output budget exceeds limit");
    }
    // SAFETY: level six is valid; a failed C allocation is handled explicitly.
    let compressor = NonNull::new(unsafe { ffi::libdeflate_alloc_compressor(6) })
        .map(Compressor)
        .ok_or("PNG: compressor allocation failed")?;
    // SAFETY: the compressor is live; input length is bounded above.
    let capacity =
        unsafe { ffi::libdeflate_zlib_compress_bound(compressor.0.as_ptr(), input.len()) }
            .min(limit)
            .min(MAX_ZLIB);
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| "PNG: allocation failed")?;
    output.resize(capacity, 0);
    // SAFETY: input/output are disjoint live byte slices of the supplied sizes.
    // libdeflate returns zero without exceeding the destination on insufficient space.
    let written = unsafe {
        ffi::libdeflate_zlib_compress(
            compressor.0.as_ptr(),
            input.as_ptr().cast(),
            input.len(),
            output.as_mut_ptr().cast(),
            output.len(),
        )
    };
    if written == 0 || written > output.len() {
        return Err("PNG: compressed output exceeds limit");
    }
    output.truncate(written);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compression_respects_output_ceiling_and_roundtrips_with_independent_decoder() {
        let input = vec![b'a'; 4096];
        let encoded = compress_bounded(&input, 128).unwrap();
        assert!(encoded.len() <= 128);
        let mut decoded = vec![0; input.len()];
        crate::png::inflate(&encoded, &mut decoded).unwrap();
        assert_eq!(decoded, input);
        assert!(compress_bounded(&input, 4).is_err());
        let empty = compress_bounded(&[], 128).unwrap();
        crate::png::inflate(&empty, &mut []).unwrap();
    }
}
