use super::*;
impl<'a, F: FnMut(usize, &[u8]), const VALIDATE_ONLY: bool> StreamRows<'a, F, VALIDATE_ONLY> {
    pub(crate) fn new(stride: usize, height: usize, on_row: &'a mut F) -> Result<Self, Error> {
        Ok(Self {
            filtered: Buffer::new(if VALIDATE_ONLY { 0 } else { stride + 1 })?,
            previous: Buffer::new(if VALIDATE_ONLY { 0 } else { stride + 1 })?,
            row_size: stride + 1,
            filled: 0,
            row: 0,
            height,
            adler_a: 1,
            adler_b: 0,
            invalid_filter: false,
            on_row,
        })
    }

    #[inline(always)]
    pub(crate) fn feed(&mut self, mut data: &[u8]) {
        if VALIDATE_ONLY {
            // The history still contains the original filtered bytes. Check
            // filter tags at row boundaries and checksum the whole contiguous
            // segment without materializing rows that will never be rendered.
            const MOD: u64 = 65521;
            let partial = adler32(data);
            let a = u64::from(self.adler_a);
            let b = u64::from(self.adler_b);
            self.adler_a = ((a + u64::from(partial & 0xffff) + MOD - 1) % MOD) as u32;
            self.adler_b =
                ((b + u64::from(partial >> 16) + data.len() as u64 * ((a + MOD - 1) % MOD)) % MOD)
                    as u32;
            let mut next = if self.filled == 0 {
                0
            } else {
                self.row_size - self.filled
            };
            while next < data.len() {
                self.invalid_filter |= data[next] > 4;
                next += self.row_size;
            }
            let total = self.filled + data.len();
            self.row += total / self.row_size;
            self.filled = total % self.row_size;
            return;
        }
        while !data.is_empty() {
            let take = core::cmp::min(data.len(), self.filtered.len() - self.filled);
            self.filtered.as_mut_slice()[self.filled..self.filled + take]
                .copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == self.filtered.len() {
                self.finish_row();
                self.filled = 0;
            }
        }
    }

    #[inline(never)]
    pub(crate) fn finish_row(&mut self) {
        debug_assert!(self.row < self.height);
        const MOD: u64 = 65521;
        let filtered = self.filtered.as_slice();
        let partial = adler32(filtered);
        let a = u64::from(self.adler_a);
        let b = u64::from(self.adler_b);
        self.adler_a = ((a + u64::from(partial & 0xffff) + MOD - 1) % MOD) as u32;
        self.adler_b =
            ((b + u64::from(partial >> 16) + filtered.len() as u64 * ((a + MOD - 1) % MOD)) % MOD)
                as u32;
        // A normalized RGB8 identity PNG reuses the encoded scanlines.
        // All filter values 0..=4 are valid for any byte values; if we do not
        // consume pixels, reconstructing the predictor output is unnecessary.
        // The DEFLATE history and Adler still cover every filtered byte.
        if VALIDATE_ONLY {
            self.invalid_filter |= filtered[0] > 4;
            self.row += 1;
            return;
        }
        if !self.invalid_filter {
            let filter = filtered[0];
            // Adler has consumed the original filtered bytes. Reconstruct
            // this row in place and retain it as the next row's predecessor;
            // neither reconstructed buffer aliases the DEFLATE history.
            let previous = &self.previous.as_slice()[1..];
            let current = &mut self.filtered.as_mut_slice()[1..];
            match filter {
                0 => {}
                1..=4 => {
                    let mut x = 0;
                    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
                    if filter == 4 {
                        // SAFETY: RGB rows are distinct and equally sized;
                        // the helper bounds each vector load and store.
                        x = unsafe { paeth_row_neon::<3>(current, previous) };
                    }
                    if filter == 2 {
                        #[cfg(target_arch = "aarch64")]
                        unsafe {
                            use core::arch::aarch64::{vaddq_u8, vld1q_u8, vst1q_u8};
                            while x + 16 <= current.len() {
                                // SAFETY: both slices cover this vector;
                                // previous and current are distinct buffers.
                                let row = vld1q_u8(current.as_ptr().add(x));
                                let above = vld1q_u8(previous.as_ptr().add(x));
                                vst1q_u8(current.as_mut_ptr().add(x), vaddq_u8(row, above));
                                x += 16;
                            }
                        }
                    }
                    while x < current.len() {
                        let left = if x >= 3 { current[x - 3] } else { 0 };
                        let above = previous[x];
                        let upper_left = if x >= 3 { previous[x - 3] } else { 0 };
                        let predictor = match filter {
                            1 => left,
                            2 => above,
                            3 => ((u16::from(left) + u16::from(above)) / 2) as u8,
                            4 => paeth(left, above, upper_left),
                            _ => unreachable!(),
                        };
                        current[x] = current[x].wrapping_add(predictor);
                        x += 1;
                    }
                }
                _ => self.invalid_filter = true,
            }
            if !self.invalid_filter {
                (self.on_row)(self.row, current);
                core::mem::swap(&mut self.previous, &mut self.filtered);
            }
        }
        self.row += 1;
    }

    pub(crate) fn checksum(&self) -> u32 {
        debug_assert_eq!(self.row, self.height);
        debug_assert_eq!(self.filled, 0);
        (self.adler_b << 16) | self.adler_a
    }
}
impl<'a, F: FnMut(usize, &[u8]), const VALIDATE_ONLY: bool> RingInflate<'a, F, VALIDATE_ONLY> {
    pub(crate) fn new(stride: usize, height: usize, on_row: &'a mut F) -> Result<Self, Error> {
        Ok(Self {
            history: Buffer::new(DEFLATE_WINDOW)?,
            rows: StreamRows::new(stride, height, on_row)?,
            pos: 0,
            expected: mul_checked(add_checked(stride, 1)?, height)?,
        })
    }

    /// Advance after a write confined to the remaining contiguous segment.
    /// At the boundary consume it before index zero can be overwritten.
    #[inline(always)]
    pub(crate) fn advance(&mut self, count: usize) {
        debug_assert!(count <= DEFLATE_WINDOW - self.pos);
        self.pos += count;
        if self.pos == DEFLATE_WINDOW {
            self.rows.feed(self.history.as_slice());
            self.pos = 0;
        }
    }

    /// Record bytes immediately for LZ77 references; drain before wrap.
    pub(crate) fn append_slice(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let take = core::cmp::min(bytes.len(), DEFLATE_WINDOW - self.pos);
            self.history.as_mut_slice()[self.pos..self.pos + take].copy_from_slice(&bytes[..take]);
            self.advance(take);
            bytes = &bytes[take..];
        }
    }
}
impl<F: FnMut(usize, &[u8]), const VALIDATE_ONLY: bool> InflateOutput
    for RingInflate<'_, F, VALIDATE_ONLY>
{
    fn capacity(&self) -> usize {
        self.expected
    }
    fn literal(&mut self, byte: u8) {
        self.history.as_mut_slice()[self.pos] = byte;
        self.advance(1);
    }
    fn stored(&mut self, bytes: &[u8]) {
        self.append_slice(bytes);
    }
    fn backref(&mut self, dist: usize, length: usize) {
        let source = (self.pos + DEFLATE_WINDOW - dist) & DEFLATE_MASK;
        if dist == 3 && length >= 24 && self.pos >= 3 && self.pos + length <= DEFLATE_WINDOW {
            copy_triplet_match(self.history.as_mut_slice(), self.pos, length);
            self.advance(length);
            return;
        }
        if dist >= length
            && source + length <= DEFLATE_WINDOW
            && self.pos + length <= DEFLATE_WINDOW
            && (source + length <= self.pos || self.pos + length <= source)
        {
            copy_disjoint_match(self.history.as_mut_slice(), source, self.pos, length);
            self.advance(length);
            return;
        }
        if dist < length && source + dist == self.pos && self.pos + length <= DEFLATE_WINDOW {
            // The match and its entire repeated expansion are contiguous in
            // history. Each doubling copy reads only initialized output from
            // the preceding step; the row consumer observes it afterwards.
            let history = self.history.as_mut_slice();
            history.copy_within(source..self.pos, self.pos);
            let mut copied = dist;
            while copied < length {
                let take = core::cmp::min(copied, length - copied);
                history.copy_within(self.pos..self.pos + take, self.pos + copied);
                copied += take;
            }
            self.advance(length);
            return;
        }
        // At most 258 bytes per DEFLATE match. Copy its available prefix out
        // of the ring first, then expand repeated bytes, including wraparound
        // and distance-one matches, before overwriting any history.
        let mut copy = [0u8; 258];
        let mut copied = core::cmp::min(dist, length);
        let first = core::cmp::min(copied, DEFLATE_WINDOW - source);
        copy[..first].copy_from_slice(&self.history.as_slice()[source..source + first]);
        if copied > first {
            copy[first..copied].copy_from_slice(&self.history.as_slice()[..copied - first]);
        }
        while copied < length {
            let take = core::cmp::min(copied, length - copied);
            copy.copy_within(..take, copied);
            copied += take;
        }
        self.append_slice(&copy[..length]);
    }
    fn checksum(&mut self) -> u32 {
        // inflate_to calls checksum exactly once after the final DEFLATE
        // write. Earlier segments were consumed before reuse, so only this
        // final prefix remains to be consumed.
        self.rows.feed(&self.history.as_slice()[..self.pos]);
        self.rows.checksum()
    }
    fn row_error(&self) -> Option<Error> {
        if self.rows.invalid_filter {
            Some("PNG: filtre inconnu")
        } else {
            None
        }
    }
}
impl Compressed<'_> {
    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Owned(buffer) => buffer.as_slice(),
        }
    }
}
/// Parse and CRC-check every chunk before any compressed bytes are decoded.
pub(crate) fn parse_for_transform(data: &[u8]) -> Result<ParsedPng<'_>, Error> {
    if data.len() > MAX_FILE {
        return Err("PNG: file too large");
    }
    if !data.starts_with(SIGNATURE) {
        return Err("PNG: invalid signature");
    }

    let mut pos = 8usize;
    let mut width = 0usize;
    let mut height = 0usize;
    let mut color = 0u8;
    let mut channels = 0usize;
    let mut ihdr = false;
    let mut plte = false;
    let mut trns = false;
    let mut end = false;
    let mut seen_idat = false;
    let mut idat_finished = false;
    let mut idat_size = 0usize;
    let mut single_idat: Option<&[u8]> = None;
    let mut palette = [0u8; 768];
    let mut pal_alpha = [255u8; 256];
    let mut palette_count = 0usize;
    let mut transparent_gray: Option<u8> = None;
    let mut transparent_rgb: Option<[u8; 3]> = None;

    while pos < data.len() {
        if data.len() - pos < 12 {
            return Err("PNG: truncated chunk");
        }
        let len = be32(&data[pos..]) as usize;
        let chunk_end = add_checked(pos, add_checked(len, 12)?)?;
        if chunk_end > data.len() {
            return Err("PNG: truncated chunk");
        }
        let kind = &data[pos + 4..pos + 8];
        let content = &data[pos + 8..pos + 8 + len];
        let expected_crc = be32(&data[pos + 8 + len..]);
        if crc32(&data[pos + 4..pos + 8 + len]) != expected_crc {
            return Err("PNG: CRC incorrect");
        }
        if !ihdr && kind != b"IHDR" {
            return Err("PNG: IHDR absent en debut");
        }
        if seen_idat && kind != b"IDAT" {
            idat_finished = true;
        }
        match kind {
            b"IHDR" => {
                if ihdr || len != 13 {
                    return Err("PNG: invalid IHDR");
                }
                ihdr = true;
                width = be32(content) as usize;
                height = be32(&content[4..]) as usize;
                color = content[9];
                channels = match color {
                    0 | 3 => 1,
                    2 => 3,
                    4 => 2,
                    6 => 4,
                    _ => return Err("PNG: unsupported color type"),
                };
                if width == 0
                    || height == 0
                    || width > MAX_DIMENSION
                    || height > MAX_DIMENSION
                    || mul_checked(width, height)? > MAX_PIXELS
                {
                    return Err("PNG: dimensions out of bounds");
                }
                if content[8] != 8 || content[10] != 0 || content[11] != 0 || content[12] != 0 {
                    return Err(
                        "PNG: seules les images 8 bits non entrelacees sont prises en charge",
                    );
                }
            }
            b"PLTE" => {
                if plte
                    || seen_idat
                    || len == 0
                    || len > 768
                    || !len.is_multiple_of(3)
                    || color == 0
                    || color == 4
                {
                    return Err("PNG: invalid palette");
                }
                plte = true;
                palette[..len].copy_from_slice(content);
                palette_count = len / 3;
            }
            b"tRNS" => {
                if trns || seen_idat {
                    return Err("PNG: transparence mal placee");
                }
                trns = true;
                match color {
                    // Only the low sample-depth bits are meaningful in tRNS.
                    0 if len == 2 => transparent_gray = Some(content[1]),
                    2 if len == 6 => transparent_rgb = Some([content[1], content[3], content[5]]),
                    3 if plte && len <= palette_count => pal_alpha[..len].copy_from_slice(content),
                    _ => return Err("PNG: invalid or unsupported transparency"),
                }
            }
            b"IDAT" => {
                if idat_finished || (color == 3 && !plte) {
                    return Err("PNG: IDAT mal place");
                }
                // The first pass already validates the chunk CRC and order.
                // A single IDAT can be decoded directly from the input bytes.
                single_idat = if seen_idat { None } else { Some(content) };
                seen_idat = true;
                idat_size = add_checked(idat_size, len)?;
                if idat_size > MAX_FILE {
                    return Err("PNG: compressed data too large");
                }
            }
            b"IEND" => {
                if len != 0 || !seen_idat || chunk_end != data.len() {
                    return Err("PNG: invalid IEND");
                }
                end = true;
                break;
            }
            b"acTL" | b"fcTL" | b"fdAT" => return Err("PNG: animation unsupported"),
            _ => {
                if kind[0] & 0x20 == 0 {
                    return Err("PNG: chunk critique inconnu");
                }
            }
        }
        pos = chunk_end;
    }
    if !end || idat_size < 6 {
        return Err("PNG: incomplete image");
    }

    let stride = mul_checked(width, channels)?;
    let raw_size = mul_checked(add_checked(stride, 1)?, height)?;
    let pixel_size = mul_checked(mul_checked(width, height)?, 4)?;
    if raw_size > MAX_FILE || pixel_size > MAX_FILE {
        return Err("PNG: decompressed image too large");
    }
    Ok(ParsedPng {
        data,
        width,
        height,
        color,
        channels,
        stride,
        raw_size,
        pixel_size,
        idat_size,
        single_idat,
        palette,
        pal_alpha,
        palette_count,
        transparent_gray,
        transparent_rgb,
    })
}
