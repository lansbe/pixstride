use super::*;
pub(crate) fn dynamic(
    tokens: &[u32],
    fixed_len: usize,
    checksum: u32,
) -> Result<Option<Vec<u8>>, Error> {
    // Header cost makes dynamic coding unsuitable for very short streams.
    if tokens.len() < 256 {
        return Ok(None);
    }
    let mut lit_freq = [0u64; 286];
    let mut dist_freq = [0u64; 30];
    lit_freq[256] = 1;
    let mut extra_bits = 0u64;
    for &token in tokens {
        if token & 0x8000_0000 == 0 {
            lit_freq[token as usize] += 1;
        } else {
            let length = ((token >> 15) & 255) as usize + 3;
            let distance = (token & 0x7fff) as usize + 1;
            let (li, di) = match_codes(length, distance)?;
            lit_freq[257 + li] += 1;
            dist_freq[di] += 1;
            extra_bits += u64::from(LENGTH_EXTRA[li]) + u64::from(DIST_EXTRA[di]);
        }
    }
    if dist_freq.iter().all(|&f| f == 0) {
        dist_freq[0] = 1;
    }
    let Some(lit_lengths) = huffman_lengths(&lit_freq) else {
        return Ok(None);
    };
    let Some(dist_lengths) = huffman_lengths(&dist_freq) else {
        return Ok(None);
    };
    let hlit = lit_lengths
        .iter()
        .rposition(|&n| n != 0)
        .unwrap_or(256)
        .max(256)
        + 1;
    let hdist = dist_lengths[..30]
        .iter()
        .rposition(|&n| n != 0)
        .unwrap_or(0)
        + 1;
    // Send the length sequence directly, using 16 four-bit code-length codes.
    // This avoids a second compressor and keeps the header simple and bounded.
    let mut bit_cost = 3u64 + 5 + 5 + 4 + 19 * 3 + 4 * (hlit + hdist) as u64 + extra_bits;
    for i in 0..286 {
        bit_cost += lit_freq[i] * u64::from(lit_lengths[i]);
    }
    for i in 0..30 {
        bit_cost += dist_freq[i] * u64::from(dist_lengths[i]);
    }
    let byte_cost = 2 + bit_cost.div_ceil(8) + 4;
    if byte_cost >= fixed_len as u64 {
        return Ok(None);
    }
    let lit_codes = huffman_codes(&lit_lengths[..286]).ok_or("DEFLATE: invalid tree")?;
    let dist_codes = huffman_codes(&dist_lengths[..30]).ok_or("DEFLATE: invalid tree")?;
    let mut out = Bits::new();
    out.byte(0x78)?;
    out.byte(0x01)?;
    out.put(0b101, 3)?; // BFINAL=1, BTYPE=10: dynamic Huffman.
    out.put((hlit - 257) as u32, 5)?;
    out.put((hdist - 1) as u32, 5)?;
    out.put(15, 4)?; // All 19 code-length code lengths follow.
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    for &symbol in &ORDER {
        out.put(if symbol < 16 { 4 } else { 0 }, 3)?;
    }
    for &length in lit_lengths[..hlit].iter().chain(&dist_lengths[..hdist]) {
        out.put(reverse(u16::from(length), 4), 4)?;
    }
    for &token in tokens {
        if token & 0x8000_0000 == 0 {
            let symbol = token as usize;
            out.put(lit_codes[symbol], lit_lengths[symbol])?;
        } else {
            let length = ((token >> 15) & 255) as usize + 3;
            let distance = (token & 0x7fff) as usize + 1;
            let (li, di) = match_codes(length, distance)?;
            let symbol = 257 + li;
            out.put(lit_codes[symbol], lit_lengths[symbol])?;
            out.put(
                (length - usize::from(LENGTH_BASE[li])) as u32,
                LENGTH_EXTRA[li],
            )?;
            out.put(dist_codes[di], dist_lengths[di])?;
            out.put(
                (distance - usize::from(DIST_BASE[di])) as u32,
                DIST_EXTRA[di],
            )?;
        }
    }
    out.put(lit_codes[256], lit_lengths[256])?;
    out.align()?;
    for byte in checksum.to_be_bytes() {
        out.byte(byte)?;
    }
    if out.bytes.len() >= fixed_len {
        Ok(None)
    } else {
        Ok(Some(out.bytes))
    }
}
pub(crate) fn stored(input: &[u8], len: usize) -> Result<Vec<u8>, Error> {
    if len > MAX_ZLIB {
        return Err("PNG: compressed output too large");
    }
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| "PNG: allocation failed")?;
    out.extend_from_slice(&[0x78, 0x01]);
    if input.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 255, 255]);
    } else {
        let mut at = 0;
        while at < input.len() {
            let n = (input.len() - at).min(65_535);
            out.push(u8::from(at + n == input.len()));
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.extend_from_slice(&(!(n as u16)).to_le_bytes());
            out.extend_from_slice(&input[at..at + n]);
            at += n;
        }
    }
    out.extend_from_slice(&adler32(input).to_be_bytes());
    debug_assert_eq!(out.len(), len);
    Ok(out)
}
// Share the already-qualified PNG decoder Adler implementation. The
// integration must expose both cfg-specific variants as pub(crate).
pub(crate) fn adler32(input: &[u8]) -> u32 {
    crate::png::adler32(input)
}
/// Return an entire zlib stream with a 32 KiB window and Adler checksum.
pub(crate) fn compress(input: &[u8]) -> Result<Vec<u8>, Error> {
    if input.len() > MAX_FILE || input.len() >= u32::MAX as usize {
        return Err("PNG: data too large");
    }
    let blocks = input
        .len()
        .checked_add(65_534)
        .ok_or("PNG: size too large")?
        / 65_535;
    let stored_len = input
        .len()
        .checked_add(blocks.max(1).checked_mul(5).ok_or("PNG: size too large")?)
        .and_then(|n| n.checked_add(6))
        .ok_or("PNG: size too large")?;
    let fits = (stored_len <= MAX_ZLIB).then_some(stored_len);
    if let Some(output) = fixed(input, fits)? {
        return Ok(output);
    }
    stored(input, stored_len)
}
