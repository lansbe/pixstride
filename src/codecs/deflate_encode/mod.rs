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
    compress_framed(input, MAX_ZLIB, 0, 0)
}

pub(crate) fn compress_framed(
    input: &[u8],
    limit: usize,
    prefix: usize,
    suffix: usize,
) -> Result<Vec<u8>, Error> {
    if input.len() > MAX_FILE || limit < 8 {
        return Err("PNG: data or output budget exceeds limit");
    }
    // SAFETY: level one is valid; a failed C allocation is handled explicitly.
    let compressor = NonNull::new(unsafe { ffi::libdeflate_alloc_compressor(1) })
        .map(Compressor)
        .ok_or("PNG: compressor allocation failed")?;
    // SAFETY: the compressor is live; input length is bounded above.
    let capacity =
        unsafe { ffi::libdeflate_zlib_compress_bound(compressor.0.as_ptr(), input.len()) }
            .min(limit)
            .min(MAX_ZLIB);
    let framed = capacity
        .checked_add(prefix)
        .and_then(|n| n.checked_add(suffix))
        .filter(|&n| n <= MAX_FILE)
        .ok_or("PNG: framed output exceeds limit")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(framed)
        .map_err(|_| "PNG: allocation failed")?;
    output.resize(prefix, 0u8);
    // SAFETY: the reserved suffix of the allocation has at least capacity bytes.
    // libdeflate writes at most capacity bytes and reports the initialized length.
    // Unwritten spare capacity is neither exposed nor read.
    let written = unsafe {
        ffi::libdeflate_zlib_compress(
            compressor.0.as_ptr(),
            input.as_ptr().cast(),
            input.len(),
            output.spare_capacity_mut().as_mut_ptr().cast(),
            capacity,
        )
    };
    if written == 0 || written > capacity {
        return Err("PNG: compressed output exceeds limit");
    }
    // SAFETY: the prefix was initialized above and the compressor wrote written bytes.
    unsafe { output.set_len(prefix + written) };
    output.resize(prefix + written + suffix, 0);

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compression_respects_output_ceiling_and_roundtrips_with_independent_decoder() {
        let input = vec![b'a'; 4096];
        let encoded = compress_framed(&input, 128, 0, 0).unwrap();
        assert!(encoded.len() <= 128);
        let mut decoded = vec![0; input.len()];
        crate::png::inflate(&encoded, &mut decoded).unwrap();
        assert_eq!(decoded, input);
        assert!(compress_framed(&input, 4, 0, 0).is_err());
        let empty = compress_framed(&[], 128, 0, 0).unwrap();
        crate::png::inflate(&empty, &mut []).unwrap();
    }
}
