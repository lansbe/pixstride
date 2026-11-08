use super::*;
// A DHT segment can redefine the same table repeatedly, including between
// progressive scans. Bound the expensive prefix-table construction per image.
pub(crate) const MAX_HUFFMAN_TABLE_DEFINITIONS: usize = 4096;
impl Huffman {
    pub(crate) const EMPTY: Self = Self {
        counts: [0; 16],
        values: [0; 256],
        short: [0; 1024],
        len: 0,
        present: false,
    };

    pub(crate) fn token<const AC: bool>(&self, bits: &mut Entropy<'_>) -> Result<(u8, i32), Error> {
        if !self.present {
            return Err("JPEG: table Huffman absente");
        }
        if let Some(prefix) = bits.peek_prefix10() {
            // Preserve the same speculative input and marker behavior as the
            // ten-bit decoder even though DC indexes only the first eight bits.
            let entry = self.short[if AC { prefix } else { prefix >> 2 }];
            let len = ((entry >> 8) & 31) as u8;
            if len != 0 {
                let symbol = entry as u8;
                let total = ((entry >> 13) & 15) as u8;
                // An inline entry contains its whole symbol and amplitude in
                // at most eight DC bits or ten AC bits, so its category is <= 9.
                // Larger or invalid categories still take the checked path.
                if total != 0 {
                    bits.discard(total);
                    return Ok((symbol, (entry as i32) >> 17));
                }
                let size = if AC { symbol & 15 } else { symbol };
                if size > if AC { 10 } else { 11 } {
                    return Err(if AC {
                        "JPEG: invalid AC category"
                    } else {
                        "JPEG: invalid DC category"
                    });
                }
                bits.discard(len);
                return Ok((symbol, bits.value(size)?));
            }
        }
        let mut code = 0u32;
        let mut first = 0u32;
        let mut offset = 0usize;
        for &count in &self.counts {
            code = (code << 1) | bits.bit()? as u32;
            let count = count as u32;
            if code >= first && code - first < count {
                let i = offset + (code - first) as usize;
                if i >= self.len {
                    return Err("JPEG: invalid Huffman table");
                }
                let symbol = self.values[i];
                let size = if AC { symbol & 15 } else { symbol };
                if size > if AC { 10 } else { 11 } {
                    return Err(if AC {
                        "JPEG: invalid AC category"
                    } else {
                        "JPEG: invalid DC category"
                    });
                }
                return Ok((symbol, bits.value(size)?));
            }
            offset += count as usize;
            first = (first + count) << 1;
        }
        Err("JPEG: invalid Huffman code")
    }
}
impl Component {
    pub(crate) const EMPTY: Self = Self {
        id: 0,
        h: 0,
        v: 0,
        quant: 0,
        dc: 0,
        ac: 0,
        predictor: 0,
    };
}
impl<'a> Entropy<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            bits: 0,
            remaining: 0,
        }
    }

    // A marker is never consumed speculatively: restart and EOI remain at
    // self.pos even when fewer than eight bits are available for lookahead.
    pub(crate) fn fill_byte(&mut self) -> bool {
        let b = match self.data.get(self.pos) {
            Some(&b) => b,
            None => return false,
        };
        if b == 0xff {
            if self.data.get(self.pos + 1) != Some(&0x00) {
                return false;
            }
            self.pos += 1;
        }
        self.pos += 1;
        self.bits = (self.bits << 8) | b as u32;
        self.remaining += 8;
        true
    }

    // This is called only when fewer than ten bits remain, so appending
    // two bytes leaves at most 25 live bits in the u32 reservoir. Never read
    // an FF here: stuffed FF00 and marker bytes use the existing byte reader.
    #[inline(always)]
    pub(crate) fn fill_pair(&mut self) -> bool {
        let Some(end) = self.pos.checked_add(2) else {
            return false;
        };
        let Some(pair) = self.data.get(self.pos..end) else {
            return false;
        };
        if pair[0] == 0xff || pair[1] == 0xff {
            return false;
        }
        self.bits = (self.bits << 16) | u16::from_be_bytes([pair[0], pair[1]]) as u32;
        self.pos = end;
        self.remaining += 16;
        true
    }

    pub(crate) fn peek_prefix10(&mut self) -> Option<usize> {
        if self.remaining < 10 && !self.fill_pair() && !self.fill_byte() {
            return None;
        }
        while self.remaining < 10 {
            if !self.fill_byte() {
                return None;
            }
        }
        // Consumed high bits remain in the reservoir; only the ten live bits
        // at the head may address the table after any amount of lookahead.
        Some(((self.bits >> (self.remaining - 10)) & 1023) as usize)
    }

    pub(crate) fn discard(&mut self, count: u8) {
        self.remaining -= count;
    }

    pub(crate) fn take(&mut self, count: u8) -> Result<u32, Error> {
        while self.remaining < count {
            if !self.fill_byte() {
                return Err("JPEG: truncated stream or unexpected data marker");
            }
        }
        // Higher discarded bits may remain in the reservoir. Mask only when
        // extracting a value; the common inline Huffman path discards without
        // an extraction. fill_byte never drops any of the remaining low bits.
        let value = (self.bits >> (self.remaining - count)) & ((1u32 << count) - 1);
        self.discard(count);
        Ok(value)
    }

    pub(crate) fn bit(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)? as u8)
    }

    pub(crate) fn value(&mut self, count: u8) -> Result<i32, Error> {
        if count == 0 {
            return Ok(0);
        }
        if count > 16 {
            return Err("JPEG: invalid amplitude");
        }
        let mut value = self.take(count)? as i32;
        if value < (1i32 << (count - 1)) {
            value -= (1i32 << count) - 1;
        }
        Ok(value)
    }

    pub(crate) fn marker(&mut self, expected: u8) -> Result<(), Error> {
        // Only padding bits from the last entropy byte can remain at a scan
        // or restart boundary. A complete unread byte means speculative
        // lookahead crossed an unexpected extra byte; reject it rather than
        // accidentally accepting a stream the original bit reader rejected.
        if self.remaining >= 8 {
            return Err("JPEG: extra byte before marker");
        }
        self.remaining = 0;
        self.bits = 0;
        let marker = read_marker(self.data, &mut self.pos)?;
        if marker != expected {
            return Err("JPEG: invalid restart or end marker");
        }
        Ok(())
    }
}
pub(crate) fn read_u16(bytes: &[u8], pos: usize) -> Result<usize, Error> {
    let high = *bytes.get(pos).ok_or("JPEG: truncated header")? as usize;
    let low = *bytes.get(pos + 1).ok_or("JPEG: truncated header")? as usize;
    Ok((high << 8) | low)
}
pub(crate) fn read_marker(data: &[u8], pos: &mut usize) -> Result<u8, Error> {
    if data.get(*pos) != Some(&0xff) {
        return Err("JPEG: marqueur attendu");
    }
    while data.get(*pos) == Some(&0xff) {
        *pos += 1;
    }
    let marker = *data.get(*pos).ok_or("JPEG: truncated marker")?;
    *pos += 1;
    if marker == 0 {
        return Err("JPEG: invalid marker");
    }
    Ok(marker)
}
pub(crate) fn parse_dqt(
    bytes: &[u8],
    tables: &mut [[u16; 64]; 4],
    present: &mut [bool; 4],
) -> Result<(), Error> {
    let mut pos = 0;
    while pos < bytes.len() {
        let tag = bytes[pos];
        pos += 1;
        let id = (tag & 15) as usize;
        if tag >> 4 != 0 || id >= 4 {
            return Err("JPEG: unsupported quantization table");
        }
        let end = pos.checked_add(64).ok_or("JPEG: invalid table")?;
        if end > bytes.len() {
            return Err("JPEG: truncated quantization table");
        }
        for i in 0..64 {
            let q = bytes[pos + i];
            if q == 0 {
                return Err("JPEG: zero quantization");
            }
            tables[id][ZIGZAG[i]] = q as u16;
        }
        present[id] = true;
        pos = end;
    }
    Ok(())
}
pub(crate) fn parse_dht(
    bytes: &[u8],
    dc: &mut [Huffman; 4],
    ac: &mut [Huffman; 4],
    remaining: &mut usize,
) -> Result<(), Error> {
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes.len() - pos < 17 {
            return Err("JPEG: truncated Huffman table");
        }
        let tag = bytes[pos];
        pos += 1;
        let class = tag >> 4;
        let id = (tag & 15) as usize;
        if class > 1 || id >= 4 {
            return Err("JPEG: unsupported Huffman table");
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or("JPEG: Huffman table budget exceeded")?;
        let mut table = Huffman::EMPTY;
        let mut space = 1i32;
        for i in 0..16 {
            table.counts[i] = bytes[pos + i];
            table.len += bytes[pos + i] as usize;
            space = (space << 1) - bytes[pos + i] as i32;
            if space < 0 {
                return Err("JPEG: oversubscribed Huffman table");
            }
        }
        pos += 16;
        if table.len == 0 || table.len > 256 || table.len > bytes.len() - pos {
            return Err("JPEG: invalid Huffman table size");
        }
        table.values[..table.len].copy_from_slice(&bytes[pos..pos + table.len]);
        let mut code = 0usize;
        let mut offset = 0usize;
        let prefix_bits = if class == 0 { 8usize } else { 10usize };
        for length in 1..=prefix_bits {
            let count = table.counts[length - 1] as usize;
            let repeats = 1usize << (prefix_bits - length);
            for i in 0..count {
                let start = (code + i) * repeats;
                let symbol = table.values[offset + i];
                let base = ((length as u32) << 8) | symbol as u32;
                let size = if class == 1 { symbol & 15 } else { symbol };
                if size as usize + length <= prefix_bits {
                    let size = size as usize;
                    for suffix in 0..repeats {
                        let mut amplitude =
                            ((suffix >> (prefix_bits - length - size)) & ((1 << size) - 1)) as i32;
                        if size != 0 && amplitude < (1 << (size - 1)) {
                            amplitude -= (1 << size) - 1;
                        }
                        table.short[start + suffix] = base
                            | (((length + size) as u32) << 13)
                            | (((amplitude as u32) & 0x7fff) << 17);
                    }
                } else {
                    table.short[start..start + repeats].fill(base);
                }
            }
            offset += count;
            code = (code + count) << 1;
        }
        table.present = true;
        pos += table.len;
        if class == 0 {
            dc[id] = table;
        } else {
            ac[id] = table;
        }
    }
    Ok(())
}
pub(crate) fn clamp_round(value: f64) -> u8 {
    if value <= 0.0 {
        0
    } else if value >= 255.0 {
        255
    } else {
        (value + 0.5) as u8
    }
}
#[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
pub(crate) fn inverse_dct(coeff: &[f64; 64], row_mask: u8, has_ac: bool, out: &mut [u8; 64]) {
    if !has_ac {
        out.fill(clamp_round(coeff[0] * 0.125 + 128.0));
        return;
    }
    let mut work = [0f64; 64];
    for v in 0..8 {
        // Quantized JPEG blocks are often sparse. A completely empty row
        // contributes exactly zero and the work array is already zeroed.
        if row_mask & (1 << v) == 0 {
            continue;
        }
        for x in 0..8 {
            let mut sum = 0.0;
            for u in 0..8 {
                sum += BASIS[x][u] * coeff[v * 8 + u];
            }
            work[v * 8 + x] = sum;
        }
    }
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0;
            for v in 0..8 {
                sum += BASIS[y][v] * work[v * 8 + x];
            }
            out[y * 8 + x] = clamp_round(sum * 0.25 + 128.0);
        }
    }
}
// Transpose pairs of basis rows at compile time. Each SIMD lane still follows
// the exact scalar order u=0..7 or v=0..7, without an FMA or reassociation.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
pub(crate) const fn basis_pairs() -> [[[f64; 2]; 8]; 4] {
    let mut pairs = [[[0.0; 2]; 8]; 4];
    let mut x = 0;
    while x < 4 {
        let mut u = 0;
        while u < 8 {
            pairs[x][u][0] = BASIS[2 * x][u];
            pairs[x][u][1] = BASIS[2 * x + 1][u];
            u += 1;
        }
        x += 1;
    }
    pairs
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn inverse_dct_neon(coeff: &[f64; 64], row_mask: u8, out: &mut [u8; 64]) {
    use core::arch::aarch64::{vaddq_f64, vdupq_n_f64, vld1q_f64, vmulq_n_f64, vst1q_f64};
    let mut work = [0.0f64; 64];
    for v in 0..8 {
        if row_mask & (1 << v) == 0 {
            continue;
        }
        for x in 0..4 {
            let mut sum = vdupq_n_f64(0.0);
            for u in 0..8 {
                // The two lanes are adjacent valid constants; every multiply
                // is followed by the same ordered addition as the scalar loop.
                let basis = vld1q_f64(BASIS_PAIRS[x][u].as_ptr());
                sum = vaddq_f64(sum, vmulq_n_f64(basis, coeff[v * 8 + u]));
            }
            vst1q_f64(work.as_mut_ptr().add(v * 8 + 2 * x), sum);
        }
    }
    for y in 0..8 {
        for x in 0..4 {
            let mut sum = vdupq_n_f64(0.0);
            for v in 0..8 {
                let row = vld1q_f64(work.as_ptr().add(v * 8 + 2 * x));
                sum = vaddq_f64(sum, vmulq_n_f64(row, BASIS[y][v]));
            }
            // Add the same two scalings in the same order as clamp_round.
            // FCVTZU saturates negative values to zero; clamp positive values
            // above 255 after truncation, as Rust's f64-to-u8 cast does.
            let value = vaddq_f64(vmulq_n_f64(sum, 0.25), vdupq_n_f64(128.0));
            let value = vaddq_f64(value, vdupq_n_f64(0.5));
            let pair = core::arch::aarch64::vcvtq_u64_f64(value);
            out[y * 8 + 2 * x] = core::arch::aarch64::vgetq_lane_u64::<0>(pair).min(255) as u8;
            out[y * 8 + 2 * x + 1] = core::arch::aarch64::vgetq_lane_u64::<1>(pair).min(255) as u8;
        }
    }
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
pub(crate) fn inverse_dct(coeff: &[f64; 64], row_mask: u8, has_ac: bool, out: &mut [u8; 64]) {
    if !has_ac {
        out.fill(clamp_round(coeff[0] * 0.125 + 128.0));
        return;
    }
    // SAFETY: aarch64-apple-darwin supports NEON. All vector loads and stores
    // address complete pairs within the fixed arrays.
    unsafe { inverse_dct_neon(coeff, row_mask, out) }
}
