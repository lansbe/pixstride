use super::*;
#[cfg(not(all(target_arch = "aarch64", target_feature = "crc")))]
pub(crate) const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
#[cfg(not(all(target_arch = "aarch64", target_feature = "crc")))]
pub(crate) fn crc32_update(mut c: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        c = CRC_TABLE[((c ^ byte as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c
}
// Arm CRC32 instructions use the same IEEE polynomial as PNG. The path is
// compiled only when the target explicitly guarantees the CRC feature; other
// targets keep the table-based implementation. No runtime library is called.
#[cfg(all(target_arch = "aarch64", target_feature = "crc"))]
pub(crate) fn crc32_update(mut c: u32, bytes: &[u8]) -> u32 {
    use core::arch::aarch64::{__crc32b, __crc32d};
    let mut groups = bytes.chunks_exact(8);
    for group in &mut groups {
        let word = u64::from_le_bytes([
            group[0], group[1], group[2], group[3], group[4], group[5], group[6], group[7],
        ]);
        // SAFETY: this function exists only when the target enables CRC.
        c = unsafe { __crc32d(c, word) };
    }
    for &byte in groups.remainder() {
        // SAFETY: as above; the data is read as an ordinary checked slice.
        c = unsafe { __crc32b(c, byte) };
    }
    c
}
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    !crc32_update(!0u32, bytes)
}
pub(crate) fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
pub(crate) fn add_checked(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or("PNG: size too large")
}
pub(crate) fn mul_checked(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or("PNG: size too large")
}
impl<'a> Bits<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bits: 0,
            available: 0,
        }
    }

    pub(crate) fn read(&mut self, n: u8) -> Result<u32, Error> {
        while self.available < n {
            if self.pos >= self.data.len() {
                return Err("DEFLATE: truncated stream");
            }
            self.bits |= (self.data[self.pos] as u64) << self.available;
            self.pos += 1;
            self.available += 8;
        }
        let mask = (1u64 << n) - 1;
        let value = (self.bits & mask) as u32;
        self.bits >>= n;
        self.available -= n;
        Ok(value)
    }

    /// Observe the next nine bits without consuming or prefetching them. This
    /// preserves byte alignment when the next block is an uncompressed block.
    pub(crate) fn peek_nine(&self) -> Option<usize> {
        let mut value = self.bits;
        let mut count = self.available;
        let mut position = self.pos;
        while count < 9 {
            let byte = *self.data.get(position)?;
            value |= u64::from(byte) << count;
            position += 1;
            count += 8;
        }
        Some((value & 511) as usize)
    }

    pub(crate) fn align_byte(&mut self) {
        // read() pulls no more bytes than required, so fewer than eight bits
        // remain here. They are padding before the next stored block.
        self.bits = 0;
        self.available = 0;
    }
}
impl DeflateBits for Bits<'_> {
    #[inline(always)]
    fn read(&mut self, n: u8) -> Result<u32, Error> {
        Bits::read(self, n)
    }

    #[inline(always)]
    fn peek_nine(&mut self) -> Result<Option<usize>, Error> {
        Ok(Bits::peek_nine(self))
    }

    #[inline(always)]
    fn consume_huffman(&mut self, n: u8) -> Result<(), Error> {
        Bits::read(self, n).map(|_| ())
    }

    #[inline(always)]
    fn align_byte(&mut self) {
        Bits::align_byte(self)
    }

    #[inline(always)]
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    #[inline(always)]
    fn stored_header(&mut self) -> Result<(u16, u16), Error> {
        let n = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        let inv = u16::from_le_bytes([self.data[self.pos + 2], self.data[self.pos + 3]]);
        self.pos += 4;
        Ok((n, inv))
    }

    #[inline(always)]
    fn copy_stored<T: InflateOutput>(&mut self, output: &mut T, len: usize) -> Result<(), Error> {
        output.stored(&self.data[self.pos..self.pos + len]);
        self.pos += len;
        Ok(())
    }
}
impl Huffman {
    pub(crate) fn build(lengths: &[u8]) -> Result<Self, Error> {
        if lengths.len() > 288 {
            return Err("DEFLATE: tree too large");
        }
        let mut h = Self {
            counts: [0; 16],
            first: [0; 16],
            offset: [0; 16],
            symbols: [0; 288],
            fast: [0; 512],
        };
        for &length in lengths {
            if length > 15 {
                return Err("DEFLATE: invalid code length");
            }
            if length != 0 {
                h.counts[length as usize] += 1;
            }
        }
        let mut code: u32 = 0;
        let mut total: u16 = 0;
        for len in 1..=15 {
            code = (code + h.counts[len - 1] as u32) << 1;
            if code + h.counts[len] as u32 > (1u32 << len) {
                return Err("DEFLATE: arbre sursouscrit");
            }
            h.first[len] = code as u16;
            h.offset[len] = total;
            total += h.counts[len];
        }
        let mut next = h.offset;
        let mut codes = h.first;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let pos = next[length as usize] as usize;
                h.symbols[pos] = symbol as u16;
                next[length as usize] += 1;
                let canonical = codes[length as usize];
                codes[length as usize] += 1;
                if length <= 9 {
                    // Huffman codes are transmitted MSB first, whereas the
                    // bit reservoir places the first received bit at bit 0.
                    let prefix = (canonical.reverse_bits() >> (16 - length)) as usize;
                    let mut index = prefix;
                    while index < h.fast.len() {
                        h.fast[index] = ((symbol as u16) << 4) | u16::from(length);
                        index += 1usize << length;
                    }
                }
            }
        }
        Ok(h)
    }

    #[inline(always)]
    pub(crate) fn decode<B: DeflateBits>(&self, bits: &mut B) -> Result<usize, Error> {
        if let Some(index) = bits.peek_nine()? {
            let entry = self.fast[index];
            if entry != 0 {
                let length = (entry & 15) as u8;
                bits.consume_huffman(length)?;
                return Ok((entry >> 4) as usize);
            }
        }
        let mut code: u32 = 0;
        for len in 1..=15 {
            code = (code << 1) | bits.read(1)?;
            let first = self.first[len] as u32;
            let count = self.counts[len] as u32;
            if code >= first && code - first < count {
                return Ok(
                    self.symbols[self.offset[len] as usize + (code - first) as usize] as usize,
                );
            }
        }
        Err("DEFLATE: invalid Huffman code")
    }
}
pub(crate) fn fixed_trees() -> Result<&'static (Huffman, Huffman), Error> {
    // The fixed alphabet belongs to the format, not to an image or block.
    // Initialize it once without retaining any source or decoded image bytes.
    static FIXED: std::sync::OnceLock<Result<(Huffman, Huffman), Error>> =
        std::sync::OnceLock::new();
    FIXED
        .get_or_init(|| {
            let mut lengths = [0u8; 288];
            lengths[..144].fill(8);
            lengths[144..256].fill(9);
            lengths[256..280].fill(7);
            lengths[280..].fill(8);
            Ok((Huffman::build(&lengths)?, Huffman::build(&[5; 32])?))
        })
        .as_ref()
        .map_err(|error| *error)
}
pub(crate) fn dynamic_trees<B: DeflateBits>(bits: &mut B) -> Result<(Huffman, Huffman), Error> {
    let nlit = bits.read(5)? as usize + 257;
    let ndist = bits.read(5)? as usize + 1;
    let ncode = bits.read(4)? as usize + 4;
    if nlit > 286 || ndist > 32 {
        return Err("DEFLATE: invalid code count");
    }
    let mut code_lengths = [0u8; 19];
    for &index in &CODE_ORDER[..ncode] {
        code_lengths[index] = bits.read(3)? as u8;
    }
    let code_tree = Huffman::build(&code_lengths)?;
    let mut lengths = [0u8; 318];
    let total = nlit + ndist;
    let mut i = 0;
    while i < total {
        let symbol = code_tree.decode(bits)?;
        let (value, count) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => {
                if i == 0 {
                    return Err("DEFLATE: repeat without previous code");
                }
                (lengths[i - 1], bits.read(2)? as usize + 3)
            }
            17 => (0, bits.read(3)? as usize + 3),
            18 => (0, bits.read(7)? as usize + 11),
            _ => return Err("DEFLATE: invalid length code"),
        };
        if count > total - i {
            return Err("DEFLATE: repeat too long");
        }
        lengths[i..i + count].fill(value);
        i += count;
    }
    if lengths[256] == 0 {
        return Err("DEFLATE: code fin absent");
    }
    Ok((
        Huffman::build(&lengths[..nlit])?,
        Huffman::build(&lengths[nlit..total])?,
    ))
}
/// Copy a non-overlapping DEFLATE match. Small matches are common, and bounded
/// unaligned loads/stores avoid a separate generic memmove for each match.
#[inline(always)]
pub(crate) fn copy_disjoint_match(output: &mut [u8], source: usize, dest: usize, length: usize) {
    debug_assert!(length >= 3 && (source + length <= dest || dest + length <= source));
    debug_assert!(dest + length <= output.len());
    // SAFETY: inflate validates both the distance and output size before
    // entering this function. Therefore the source and destination of every
    // load/store are within output and the two ranges are disjoint. The two
    // stores to the same destination range may overlap but write equal bytes.
    unsafe {
        let base = output.as_mut_ptr();
        let src = base.add(source).cast_const();
        let dst = base.add(dest);
        if length == 3 {
            dst.cast::<u16>()
                .write_unaligned(src.cast::<u16>().read_unaligned());
            *dst.add(2) = *src.add(2);
        } else if length < 8 {
            let first = src.cast::<u32>().read_unaligned();
            let last = src.add(length - 4).cast::<u32>().read_unaligned();
            dst.cast::<u32>().write_unaligned(first);
            dst.add(length - 4).cast::<u32>().write_unaligned(last);
        } else if length < 16 {
            let first = src.cast::<u64>().read_unaligned();
            let last = src.add(length - 8).cast::<u64>().read_unaligned();
            dst.cast::<u64>().write_unaligned(first);
            dst.add(length - 8).cast::<u64>().write_unaligned(last);
        } else {
            #[cfg(target_arch = "aarch64")]
            {
                use core::arch::aarch64::{vld1q_u8, vst1q_u8};
                if length < 32 {
                    let first = vld1q_u8(src);
                    let last = vld1q_u8(src.add(length - 16));
                    vst1q_u8(dst, first);
                    vst1q_u8(dst.add(length - 16), last);
                } else if length < 64 {
                    let a = vld1q_u8(src);
                    let b = vld1q_u8(src.add(16));
                    let c = vld1q_u8(src.add(length - 32));
                    let d = vld1q_u8(src.add(length - 16));
                    vst1q_u8(dst, a);
                    vst1q_u8(dst.add(16), b);
                    vst1q_u8(dst.add(length - 32), c);
                    vst1q_u8(dst.add(length - 16), d);
                } else {
                    core::ptr::copy_nonoverlapping(src, dst, length);
                }
            }
            #[cfg(not(target_arch = "aarch64"))]
            core::ptr::copy_nonoverlapping(src, dst, length);
        }
    }
}
/// Expand a validated, contiguous distance-three match. The first 24 bytes
/// are eight copies of the prior triplet. Subsequent disjoint doubling copies
/// preserve the same three-byte period as byte-by-byte DEFLATE expansion.
#[inline(always)]
pub(crate) fn copy_triplet_match(output: &mut [u8], dest: usize, length: usize) {
    debug_assert!(dest >= 3 && length >= 24 && dest + length <= output.len());
    let a = output[dest - 3];
    let b = output[dest - 2];
    let c = output[dest - 1];
    let seed = [
        a, b, c, a, b, c, a, b, c, a, b, c, a, b, c, a, b, c, a, b, c, a, b, c,
    ];
    output[dest..dest + 24].copy_from_slice(&seed);
    let mut copied = 24;
    while copied < length {
        let take = core::cmp::min(copied, length - copied);
        output.copy_within(dest..dest + take, dest + copied);
        copied += take;
    }
}
impl InflateOutput for WholeInflate<'_> {
    fn capacity(&self) -> usize {
        self.output.len()
    }
    fn literal(&mut self, byte: u8) {
        self.output[self.used] = byte;
        self.used += 1;
    }
    fn stored(&mut self, bytes: &[u8]) {
        let end = self.used + bytes.len();
        self.output[self.used..end].copy_from_slice(bytes);
        self.used = end;
    }
    fn backref(&mut self, dist: usize, length: usize) {
        if dist == 3 && length >= 24 {
            copy_triplet_match(self.output, self.used, length);
        } else if dist >= length {
            copy_disjoint_match(self.output, self.used - dist, self.used, length);
        } else {
            let mut copied = dist;
            self.output
                .copy_within(self.used - dist..self.used, self.used);
            while copied < length {
                let take = core::cmp::min(copied, length - copied);
                self.output
                    .copy_within(self.used..self.used + take, self.used + copied);
                copied += take;
            }
        }
        self.used += length;
    }
    fn checksum(&mut self) -> u32 {
        adler32(self.output)
    }
}
pub(crate) fn inflate(zlib: &[u8], output: &mut [u8]) -> Result<(), Error> {
    let mut whole = WholeInflate { output, used: 0 };
    inflate_to(zlib, &mut whole)
}
impl InflateOutput for InflateSizeProbe {
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn literal(&mut self, _: u8) {
        self.used += 1;
    }
    fn stored(&mut self, bytes: &[u8]) {
        self.used += bytes.len();
    }
    fn backref(&mut self, _: usize, length: usize) {
        self.used += length;
    }
    fn checksum(&mut self) -> u32 {
        0
    }
}
