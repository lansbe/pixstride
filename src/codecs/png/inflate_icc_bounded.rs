use super::*;
/// Decompress one zlib ICC payload with an explicit upper bound. The maximum
/// accepted bound is 4 MiB; the returned allocation is the exact decoded size.
/// A counting pass is followed by the ordinary decoder's length, DEFLATE and
/// Adler checks. No partial profile is exposed if either pass fails.
pub(crate) fn inflate_icc_bounded(zlib: &[u8], max: usize) -> Result<Buffer, Error> {
    const MAX_ICC_BYTES: usize = 4 * 1024 * 1024;
    if max > MAX_ICC_BYTES {
        return Err("ICC: limit too large");
    }
    let mut probe = InflateSizeProbe {
        used: 0,
        capacity: max + 1,
    };
    let status = inflate_to(zlib, &mut probe);
    if probe.used > max {
        return Err("ICC: profile too large");
    }
    match status {
        Err("DEFLATE: unexpected stream size") => (),
        Err(error) => return Err(error),
        Ok(()) => return Err("ICC: inconsistent profile size"),
    }
    let mut output = Buffer::new(probe.used)?;
    inflate(zlib, output.as_mut_slice())?;
    Ok(output)
}
pub(crate) fn inflate_to<T: InflateOutput>(zlib: &[u8], output: &mut T) -> Result<(), Error> {
    if zlib.len() < 6 {
        return Err("zlib: truncated stream");
    }
    let cmf = zlib[0];
    let flg = zlib[1];
    validate_zlib_header(cmf, flg)?;
    let payload = &zlib[2..zlib.len() - 4];
    let mut bits = Bits::new(payload);
    inflate_payload(&mut bits, output)?;
    let expected = be32(&zlib[zlib.len() - 4..]);
    finish_inflate(output, expected)
}
pub(crate) fn validate_zlib_header(cmf: u8, flg: u8) -> Result<(), Error> {
    if cmf & 15 != 8 || cmf >> 4 > 7 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err("zlib: invalid header");
    }
    if flg & 32 != 0 {
        return Err("zlib: dictionary unsupported");
    }
    Ok(())
}
#[inline(always)]
pub(crate) fn inflate_payload<B: DeflateBits, T: InflateOutput>(
    bits: &mut B,
    output: &mut T,
) -> Result<(), Error> {
    let mut used = 0usize;
    let mut blocks = 0usize;
    loop {
        // Decoded byte limits do not bound empty blocks or dynamic-tree work.
        // This applies equally to buffered, streamed, ICC and TIFF inflation.
        blocks += 1;
        if blocks > 16_384 {
            return Err("DEFLATE: block work limit");
        }
        let final_block = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => {
                bits.align_byte();
                if bits.remaining() < 4 {
                    return Err("DEFLATE: truncated block");
                }
                let (n, inv) = bits.stored_header()?;
                if n != !inv {
                    return Err("DEFLATE: invalid block length");
                }
                let n = n as usize;
                if n > output.capacity() - used || n > bits.remaining() {
                    return Err("DEFLATE: block out of bounds");
                }
                bits.copy_stored(output, n)?;
                used += n;
            }
            block @ (1 | 2) => {
                let dynamic;
                let (literal, distance) = if block == 1 {
                    fixed_trees()?
                } else {
                    dynamic = dynamic_trees(bits)?;
                    &dynamic
                };
                loop {
                    let symbol = literal.decode(bits)?;
                    match symbol {
                        0..=255 => {
                            if used >= output.capacity() {
                                return Err("DEFLATE: output too large");
                            }
                            output.literal(symbol as u8);
                            used += 1;
                        }
                        256 => break,
                        257..=285 => {
                            let code = symbol - 257;
                            let length =
                                LENGTH_BASE[code] + bits.read(LENGTH_EXTRA[code])? as usize;
                            let dcode = distance.decode(bits)?;
                            if dcode >= 30 {
                                return Err("DEFLATE: distance reservee");
                            }
                            let dist = DIST_BASE[dcode] + bits.read(DIST_EXTRA[dcode])? as usize;
                            if dist > used || length > output.capacity() - used {
                                return Err("DEFLATE: copy out of bounds");
                            }
                            output.backref(dist, length);
                            used += length;
                        }
                        _ => return Err("DEFLATE: symbole reserve"),
                    }
                }
            }
            _ => return Err("DEFLATE: type de bloc reserve"),
        }
        if final_block {
            break;
        }
    }
    if used != output.capacity() || bits.remaining() != 0 {
        return Err("DEFLATE: unexpected stream size");
    }
    Ok(())
}
pub(crate) fn finish_inflate<T: InflateOutput>(output: &mut T, expected: u32) -> Result<(), Error> {
    if output.checksum() != expected {
        return Err("zlib: checksum Adler incorrect");
    }
    if let Some(error) = output.row_error() {
        return Err(error);
    }
    Ok(())
}
#[cfg(not(all(
    target_arch = "aarch64",
    target_feature = "neon",
    target_feature = "dotprod"
)))]
pub(crate) fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(5552) {
        // Four independent prefix sums shorten the dependency chain. For each
        // lane j, its accumulated prefix values carry weight 4*(n-k)-j in the
        // normal Adler sum. Recombine in u64, then reduce as usual. The limit
        // of 5552 bytes also keeps each individual u32 lane bounded.
        let mut sums = [0u32; 4];
        let mut prefixes = [0u32; 4];
        let mut groups = chunk.chunks_exact(4);
        for group in &mut groups {
            for lane in 0..4 {
                sums[lane] += u32::from(group[lane]);
                prefixes[lane] += sums[lane];
            }
        }
        let total: u64 = sums.iter().map(|&v| u64::from(v)).sum();
        let prefix_total: u64 = prefixes.iter().map(|&v| u64::from(v)).sum();
        let correction = u64::from(sums[1]) + 2 * u64::from(sums[2]) + 3 * u64::from(sums[3]);
        let mut b64 = u64::from(b) + (chunk.len() / 4) as u64 * 4 * u64::from(a) + 4 * prefix_total
            - correction;
        let mut a64 = u64::from(a) + total;
        for &byte in groups.remainder() {
            a64 += u64::from(byte);
            b64 += a64;
        }
        a = (a64 % 65521) as u32;
        b = (b64 % 65521) as u32;
    }
    (b << 16) | a
}
// The same Adler recurrence, computed over 16-byte blocks with ARM's integer
// dot product. For block j, the 16 bytes contribute sum S[j] to A and a
// within-block weighted sum W[j] to B. Keeping prefix += old sum yields
// B += 16*N*A_start + 16*sum(prefix) + sum(W). The 5552-byte reduction
// bounds all u32 lanes; the final recombination uses u64. Targets without the
// declared dotprod feature use the four-lane scalar implementation above.
#[cfg(all(
    target_arch = "aarch64",
    target_feature = "neon",
    target_feature = "dotprod"
))]
pub(crate) fn adler32(data: &[u8]) -> u32 {
    use core::arch::aarch64::{
        vaddq_u32, vaddvq_u32, vdotq_u32, vdupq_n_u32, vdupq_n_u8, vld1q_u8,
    };
    let mut a = 1u32;
    let mut b = 0u32;
    let weights = [16u8, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    for chunk in data.chunks(5552) {
        let mut groups = chunk.chunks_exact(16);
        let count = groups.len();
        // SAFETY: target features are explicitly required by cfg. Each vld1q
        // reads exactly 16 checked bytes from either weights or a full chunk.
        let (sum, prefixes, weighted) = unsafe {
            let one = vdupq_n_u8(1);
            let weight = vld1q_u8(weights.as_ptr());
            let mut sums = vdupq_n_u32(0);
            let mut prefix = vdupq_n_u32(0);
            let mut weighted = vdupq_n_u32(0);
            for group in &mut groups {
                let bytes = vld1q_u8(group.as_ptr());
                prefix = vaddq_u32(prefix, sums);
                sums = vdotq_u32(sums, bytes, one);
                weighted = vdotq_u32(weighted, bytes, weight);
            }
            (vaddvq_u32(sums), vaddvq_u32(prefix), vaddvq_u32(weighted))
        };
        let mut b64 = u64::from(b)
            + (count as u64) * 16 * u64::from(a)
            + 16 * u64::from(prefixes)
            + u64::from(weighted);
        let mut a64 = u64::from(a) + u64::from(sum);
        for &byte in groups.remainder() {
            a64 += u64::from(byte);
            b64 += a64;
        }
        a = (a64 % 65521) as u32;
        b = (b64 % 65521) as u32;
    }
    (b << 16) | a
}
pub(crate) fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}
// The left dependency runs between pixels, but the color channels within a
// pixel are independent. Evaluate three or four predictors together while
// preserving the exact Paeth tie order and wrapping byte addition.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn paeth_row_neon<const CHANNELS: usize>(
    row: &mut [u8],
    previous: &[u8],
) -> usize {
    use core::arch::aarch64::*;
    debug_assert_eq!(row.len(), previous.len());
    debug_assert!(CHANNELS == 3 || CHANNELS == 4);
    let mut left = vdupq_n_u16(0);
    let mut upper_left = vdupq_n_u16(0);
    let mut x = 0;
    while x + 4 <= row.len() {
        // SAFETY: the guard keeps both four-byte reads and the write inside
        // their distinct rows. For RGB, the fourth byte is preserved; the
        // fourth NEON lane never contributes to any of the three channels.
        let raw = core::ptr::read_unaligned(row.as_ptr().add(x).cast::<u32>());
        let above_word = core::ptr::read_unaligned(previous.as_ptr().add(x).cast::<u32>());
        let above = vmovl_u8(vcreate_u8(u64::from(above_word)));
        let pa = vabdq_u16(above, upper_left);
        let pb = vabdq_u16(left, upper_left);
        // All component sums are in [-510, 510] and fit in signed i16.
        let pc = vreinterpretq_u16_s16(vabsq_s16(vsubq_s16(
            vaddq_s16(vreinterpretq_s16_u16(left), vreinterpretq_s16_u16(above)),
            vshlq_n_s16::<1>(vreinterpretq_s16_u16(upper_left)),
        )));
        let choose_left = vandq_u16(vcleq_u16(pa, pb), vcleq_u16(pa, pc));
        let choose_above = vcleq_u16(pb, pc);
        let predictor = vbslq_u16(
            choose_left,
            left,
            vbslq_u16(choose_above, above, upper_left),
        );
        let result = vadd_u8(vcreate_u8(u64::from(raw)), vmovn_u16(predictor));
        let mut value = vget_lane_u32::<0>(vreinterpret_u32_u8(result));
        if CHANNELS == 3 {
            value = (value & 0x00ff_ffff) | (raw & 0xff00_0000);
        }
        core::ptr::write_unaligned(row.as_mut_ptr().add(x).cast::<u32>(), value);
        left = vmovl_u8(result);
        upper_left = above;
        x += CHANNELS;
    }
    x
}
/// Expand an opaque RGB row directly to RGBA. The caller supplies equal pixel
/// counts and has already validated the source and destination image sizes.
pub(crate) fn opaque_rgb_to_rgba(row: &[u8], out: &mut [u8]) {
    debug_assert_eq!(row.len() % 3, 0);
    debug_assert_eq!(out.len(), row.len() / 3 * 4);
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut source = 0usize;
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut dest = 0usize;
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        use core::arch::aarch64::{uint8x16x4_t, vdupq_n_u8, vld3q_u8, vst4q_u8};
        while row.len() - source >= 48 {
            // SAFETY: each iteration reads exactly 48 source bytes and writes
            // 64 destination bytes. Row lengths are derived from validated
            // image dimensions; the two buffers are separate mmap mappings.
            unsafe {
                let rgb = vld3q_u8(row.as_ptr().add(source));
                let rgba = uint8x16x4_t(rgb.0, rgb.1, rgb.2, vdupq_n_u8(255));
                vst4q_u8(out.as_mut_ptr().add(dest), rgba);
            }
            source += 48;
            dest += 64;
        }
    }
    for (rgb, rgba) in row[source..]
        .as_chunks::<3>()
        .0
        .iter()
        .zip(out[dest..].chunks_exact_mut(4))
    {
        rgba.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
}
