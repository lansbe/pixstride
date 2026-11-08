use super::*;
pub(crate) fn huffman(counts: &[u8; 16], values: &[u8]) -> Result<[Code; 256], Error> {
    let mut table = [Code::default(); 256];
    let mut code = 0u32;
    let mut at = 0usize;
    for (i, &count) in counts.iter().enumerate() {
        let len = i + 1;
        for _ in 0..count {
            let symbol = *values.get(at).ok_or("JPEG: invalid Huffman table")? as usize;
            // JPEG reserves the all-ones code for entropy padding.
            if code >= (1u32 << len) - 1 || table[symbol].len != 0 {
                return Err("JPEG: invalid Huffman table");
            }
            table[symbol] = Code {
                bits: code as u16,
                len: len as u8,
            };
            at += 1;
            code += 1;
        }
        code <<= 1;
    }
    if at != values.len() {
        return Err("JPEG: invalid Huffman table");
    }
    Ok(table)
}
// Faults apply only to this test thread and only to byte-at-a-time output.
// Production builds do not contain this hook or its state.

impl Writer {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bits: 0,
            count: 0,
        }
    }

    pub(crate) fn bytes(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.len() > MAX_FILE.saturating_sub(self.bytes.len()) {
            return Err("JPEG: encoded output too large");
        }
        self.bytes
            .try_reserve(data.len())
            .map_err(|_| "memory allocation failed")?;
        self.bytes.extend_from_slice(data);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn byte(&mut self, byte: u8) -> Result<(), Error> {
        if self.bytes.len() >= MAX_FILE {
            return Err("JPEG: encoded output too large");
        }
        if self.bytes.len() == self.bytes.capacity() {
            self.bytes
                .try_reserve(1)
                .map_err(|_| "memory allocation failed")?;
        }
        self.bytes.push(byte);
        Ok(())
    }
    pub(crate) fn word(&mut self, value: u16) -> Result<(), Error> {
        self.bytes(&value.to_be_bytes())
    }
    pub(crate) fn marker(&mut self, marker: u8) -> Result<(), Error> {
        self.bytes(&[0xff, marker])
    }

    #[inline(always)]
    pub(crate) fn entropy(&mut self, code: u32, len: u8) -> Result<(), Error> {
        if len == 0 || len > 16 || code >= (1u32 << len) {
            return Err("JPEG: invalid entropy code");
        }
        self.append_valid_bits(code, len)
    }

    /// Append previously validated bits, most significant first. The fused
    /// Huffman/amplitude path can supply up to 32 bits; at most seven prior
    /// bits are pending, so the u64 reservoir never approaches overflow.
    #[inline(always)]
    pub(crate) fn append_valid_bits(&mut self, code: u32, len: u8) -> Result<(), Error> {
        debug_assert!(len > 0 && len <= 32);
        debug_assert!(u64::from(code) < (1u64 << len));
        debug_assert!(self.count < 8);
        self.bits = (self.bits << len) | u64::from(code);
        self.count += len;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.bits >> self.count) as u8;
            self.byte(byte)?;
            if byte == 0xff {
                self.byte(0)?;
            }
        }
        self.bits &= (1u64 << self.count) - 1;
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn huff(&mut self, table: &[Code; 256], symbol: u8) -> Result<(), Error> {
        let code = table[usize::from(symbol)];
        self.entropy(u32::from(code.bits), code.len)
    }

    /// A nonzero coefficient emits a Huffman symbol immediately followed by
    /// its amplitude. Combining those adjacent bit strings preserves their
    /// order while draining and masking the reservoir only once.
    #[inline(always)]
    pub(crate) fn huff_amplitude(
        &mut self,
        table: &[Code; 256],
        symbol: u8,
        bits: u32,
        len: u8,
    ) -> Result<(), Error> {
        let code = table[usize::from(symbol)];
        if code.len == 0
            || code.len > 16
            || u32::from(code.bits) >= (1u32 << code.len)
            || len == 0
            || len > 16
            || bits >= (1u32 << len)
        {
            // Preserve validation and error order for malformed inputs.
            self.entropy(u32::from(code.bits), code.len)?;
            return self.entropy(bits, len);
        }
        let joined = (u32::from(code.bits) << len) | bits;
        self.append_valid_bits(joined, code.len + len)
    }

    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        if self.count != 0 {
            let padding = 8 - self.count;
            self.entropy((1u32 << padding) - 1, padding)?;
        }
        Ok(())
    }
}
pub(crate) fn quantization(quality: u8, base: &[u8; 64]) -> Result<[u8; 64], Error> {
    if !(1..=100).contains(&quality) {
        return Err("JPEG: quality must be 1..100");
    }
    // Common quality convention, not a normative JPEG quality scale.
    let q = u32::from(quality);
    let scale = if q < 50 { 5000 / q } else { 200 - q * 2 };
    let mut result = [0; 64];
    for (value, &entry) in result.iter_mut().zip(base) {
        *value = ((u32::from(entry) * scale + 50) / 100).clamp(1, 255) as u8;
    }
    Ok(result)
}
pub(crate) fn dht(
    w: &mut Writer,
    selector: u8,
    counts: &[u8; 16],
    values: &[u8],
) -> Result<(), Error> {
    w.marker(0xc4)?;
    w.word((2 + 1 + counts.len() + values.len()) as u16)?;
    w.byte(selector)?;
    w.bytes(counts)?;
    w.bytes(values)
}
pub(crate) fn headers(
    w: &mut Writer,
    width: u16,
    height: u16,
    qy: &[u8; 64],
    qc: &[u8; 64],
) -> Result<(), Error> {
    w.marker(0xd8)?;
    w.marker(0xe0)?; // JFIF APP0
    w.word(16)?;
    w.bytes(b"JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00")?;
    w.marker(0xdb)?;
    w.word(132)?;
    w.byte(0)?;
    for &index in &ZIGZAG {
        w.byte(qy[index])?;
    }
    w.byte(1)?;
    for &index in &ZIGZAG {
        w.byte(qc[index])?;
    }
    w.marker(0xc0)?;
    w.word(17)?;
    w.byte(8)?;
    w.word(height)?;
    w.word(width)?;
    w.bytes(&[3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1])?;
    dht(w, 0x00, &DC_Y_COUNTS, &DC_VALUES)?;
    dht(w, 0x10, &AC_Y_COUNTS, &AC_Y_VALUES)?;
    dht(w, 0x01, &DC_C_COUNTS, &DC_VALUES)?;
    dht(w, 0x11, &AC_C_COUNTS, &AC_C_VALUES)?;
    w.marker(0xda)?;
    w.word(12)?;
    w.bytes(&[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0])
}
pub(crate) fn level(pixel: &[u8]) -> [i16; 3] {
    let (r, g, b) = if pixel[3] == 255 {
        // For any byte x, floor((255*x + 127)/255) equals x exactly.
        (
            i32::from(pixel[0]),
            i32::from(pixel[1]),
            i32::from(pixel[2]),
        )
    } else {
        let alpha = u32::from(pixel[3]);
        let flatten =
            |x: u8| -> i32 { ((u32::from(x) * alpha + 255 * (255 - alpha) + 127) / 255) as i32 };
        (flatten(pixel[0]), flatten(pixel[1]), flatten(pixel[2]))
    };
    let y = (19595 * r + 38470 * g + 7471 * b + 32768) >> 16;
    let cb = ((-11059 * r - 21709 * g + 32768 * b + 32768) >> 16) + 128;
    let cr = ((32768 * r - 27439 * g - 5329 * b + 32768) >> 16) + 128;
    [
        (y.clamp(0, 255) - 128) as i16,
        (cb.clamp(0, 255) - 128) as i16,
        (cr.clamp(0, 255) - 128) as i16,
    ]
}
pub(crate) fn quantized_dct(samples: &[i16; 64], quant: &[u8; 64]) -> [i16; 64] {
    // For any i16 sample, |left +/- right| <= 65536 and the largest
    // sum of absolute coefficients in a row is 23172. Consequently every
    // horizontal partial and final sum fits i32 (at most 1_518_600_192).
    let mut horizontal = [0i32; 64];
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe {
        use core::arch::aarch64::{
            vdupq_n_s32, vld1q_s32, vmlaq_n_s32, vst1q_s32, vzip1q_s32, vzip2q_s32,
        };
        for y in 0..8 {
            let mut even = vdupq_n_s32(0);
            let mut odd = vdupq_n_s32(0);
            for x in 0..4 {
                let left = i32::from(samples[y * 8 + x]);
                let right = i32::from(samples[y * 8 + 7 - x]);
                // SAFETY: the four coefficient lanes and output lanes are
                // fixed-size and within their arrays; NEON is enabled by cfg.
                even = vmlaq_n_s32(even, vld1q_s32(DCT_EVEN[x].as_ptr()), left + right);
                odd = vmlaq_n_s32(odd, vld1q_s32(DCT_ODD[x].as_ptr()), left - right);
            }
            let dst = horizontal.as_mut_ptr().add(y * 8);
            vst1q_s32(dst, vzip1q_s32(even, odd));
            vst1q_s32(dst.add(4), vzip2q_s32(even, odd));
        }
    }
    #[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
    for y in 0..8 {
        for u in 0..8 {
            let mut sum = 0i32;
            for x in 0..4 {
                let left = i32::from(samples[y * 8 + x]);
                let right = i32::from(samples[y * 8 + 7 - x]);
                sum += (if u & 1 == 0 {
                    left + right
                } else {
                    left - right
                }) * DCT[u][x];
            }
            horizontal[y * 8 + u] = sum;
        }
    }
    let mut result = [0i16; 64];
    for v in 0..8 {
        for u in 0..8 {
            let mut sum = 0i64;
            for y in 0..4 {
                let top = i64::from(horizontal[y * 8 + u]);
                let bottom = i64::from(horizontal[(7 - y) * 8 + u]);
                sum += (if v & 1 == 0 {
                    top + bottom
                } else {
                    top - bottom
                }) * i64::from(DCT[v][y]);
            }
            let q = quant[v * 8 + u];
            let divisor = (1i64 << 28) * i64::from(q);
            // JPEG quality 100 has unit quantizers. Preserve ties away from
            // zero, but use the exact power-of-two shift instead of division.
            let absolute = sum.unsigned_abs() + (divisor / 2) as u64;
            let magnitude = if q == 1 {
                absolute >> 28
            } else {
                absolute / divisor as u64
            };
            let rounded = if sum < 0 {
                -(magnitude as i64)
            } else {
                magnitude as i64
            };
            result[v * 8 + u] = rounded as i16;
        }
    }
    result
}
pub(crate) fn magnitude(value: i32) -> (u8, u32) {
    let abs = value.unsigned_abs();
    if abs == 0 {
        return (0, 0);
    }
    let len = (32 - abs.leading_zeros()) as u8;
    let bits = if value < 0 {
        ((1i64 << len) - 1 + i64::from(value)) as u32
    } else {
        abs
    };
    (len, bits)
}
pub(crate) fn block(
    w: &mut Writer,
    samples: &[i16; 64],
    quant: &[u8; 64],
    prev: &mut i32,
    dc: &[Code; 256],
    ac: &[Code; 256],
) -> Result<(), Error> {
    let coefficients = quantized_dct(samples, quant);
    let current = i32::from(coefficients[0]);
    let (size, bits) = magnitude(current - *prev);
    if size > 11 {
        return Err("JPEG: DC coefficient outside baseline range");
    }
    *prev = current;
    if size == 0 {
        w.huff(dc, size)?;
    } else {
        w.huff_amplitude(dc, size, bits, size)?;
    }
    let mut zeroes = 0u8;
    for &index in &ZIGZAG[1..] {
        let value = i32::from(coefficients[index]);
        if value == 0 {
            zeroes += 1;
            continue;
        }
        while zeroes >= 16 {
            w.huff(ac, 0xf0)?;
            zeroes -= 16;
        }
        let (size, bits) = magnitude(value);
        if size > 10 {
            return Err("JPEG: AC coefficient outside baseline range");
        }
        w.huff_amplitude(ac, (zeroes << 4) | size, bits, size)?;
        zeroes = 0;
    }
    if zeroes != 0 {
        w.huff(ac, 0)?;
    }
    Ok(())
}
/// Encode an RGBA image to sequential 8-bit baseline JFIF JPEG.
///
/// `quality` uses the common 1..=100 quantization convention; it does not
/// imply losslessness or byte identity with any other JPEG implementation.
pub(crate) fn encode(image: &Image, quality: u8) -> Result<Buffer, Error> {
    let width = image.width;
    let height = image.height;
    let pixels = width
        .checked_mul(height)
        .ok_or("JPEG: dimensions overflow")?;
    let bytes = pixels.checked_mul(4).ok_or("JPEG: dimensions overflow")?;
    if width == 0
        || height == 0
        || width > u16::MAX as usize
        || height > u16::MAX as usize
        || pixels > MAX_PIXELS
        || bytes > MAX_FILE
        || image.pixels.len() != bytes
    {
        return Err("JPEG: dimensions or pixels outside limits");
    }
    let qy = quantization(quality, &QUANT_Y)?;
    let qc = quantization(quality, &QUANT_C)?;
    let dc_y = huffman(&DC_Y_COUNTS, &DC_VALUES)?;
    let dc_c = huffman(&DC_C_COUNTS, &DC_VALUES)?;
    let ac_y = huffman(&AC_Y_COUNTS, &AC_Y_VALUES)?;
    let ac_c = huffman(&AC_C_COUNTS, &AC_C_VALUES)?;
    let mut w = Writer::new();
    headers(&mut w, width as u16, height as u16, &qy, &qc)?;
    let source = image.pixels.as_slice();
    let mut previous = [0i32; 3];
    for by in (0..height).step_by(8) {
        for bx in (0..width).step_by(8) {
            let mut planes = [[0i16; 64]; 3];
            for y in 0..8 {
                let sy = (by + y).min(height - 1);
                for x in 0..8 {
                    let sx = (bx + x).min(width - 1);
                    let at = (sy * width + sx) * 4;
                    let values = level(&source[at..at + 4]);
                    for c in 0..3 {
                        planes[c][y * 8 + x] = values[c];
                    }
                }
            }
            block(&mut w, &planes[0], &qy, &mut previous[0], &dc_y, &ac_y)?;
            block(&mut w, &planes[1], &qc, &mut previous[1], &dc_c, &ac_c)?;
            block(&mut w, &planes[2], &qc, &mut previous[2], &dc_c, &ac_c)?;
        }
    }
    w.flush()?;
    w.marker(0xd9)?;
    let mut output = Buffer::new(w.bytes.len())?;
    output.as_mut_slice().copy_from_slice(&w.bytes);
    Ok(output)
}
