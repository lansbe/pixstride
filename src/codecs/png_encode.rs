//! Lossless RGB/RGBA PNG encoding for , with adaptive PNG filters and the
//! in-tree zlib/DEFLATE encoder. No external image or compression library.
//!
//! Input pixels are RGBA8. An entirely opaque image is encoded as RGB8;
//! otherwise all four channels are preserved. Color metadata is not part of
//! `Image` and therefore is not invented here.

use crate::{deflate_encode, Buffer, Error, Image, MAX_FILE, MAX_PIXELS};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_DIMENSION: usize = 100_000;

fn write_chunk(output: &mut [u8], pos: usize, kind: &[u8; 4], data: &[u8]) -> usize {
    output[pos..pos + 4].copy_from_slice(&(data.len() as u32).to_be_bytes());
    output[pos + 4..pos + 8].copy_from_slice(kind);
    output[pos + 8..pos + 8 + data.len()].copy_from_slice(data);
    let crc = crate::png::crc32(&output[pos + 4..pos + 8 + data.len()]);
    output[pos + 8 + data.len()..pos + 12 + data.len()].copy_from_slice(&crc.to_be_bytes());
    pos + 12 + data.len()
}

#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
    let p = a + b - c;
    let da = (p - a).abs();
    let db = (p - b).abs();
    let dc = (p - c).abs();
    if da <= db && da <= dc {
        a as u8
    } else if db <= dc {
        b as u8
    } else {
        c as u8
    }
}

#[inline]
fn filtered(kind: usize, x: u8, a: u8, b: u8, c: u8) -> u8 {
    match kind {
        0 => x,
        1 => x.wrapping_sub(a),
        2 => x.wrapping_sub(b),
        3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
        4 => x.wrapping_sub(paeth(a, b, c)),
        _ => unreachable!(),
    }
}

#[cfg(any(test, not(all(target_arch = "aarch64", target_feature = "neon"))))]
fn score_filters_scalar(current: &[u8], previous: &[u8], channels: usize) -> [u64; 5] {
    let mut scores = [0u64; 5];
    for i in 0..current.len() {
        let a = if i >= channels {
            current[i - channels]
        } else {
            0
        };
        let b = previous[i];
        let c = if i >= channels {
            previous[i - channels]
        } else {
            0
        };
        for (kind, score) in scores.iter_mut().enumerate() {
            let value = u16::from(filtered(kind, current[i], a, b, c));
            *score += u64::from(value.min(256 - value));
        }
    }
    scores
}

// The 8-bit inputs give p in -255..510 and differences in -510..510,
// which fit exactly in signed 16-bit lanes. Tie order matches scalar Paeth.
#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
#[inline(always)]
unsafe fn paeth_lanes(
    a: core::arch::aarch64::uint8x8_t,
    b: core::arch::aarch64::uint8x8_t,
    c: core::arch::aarch64::uint8x8_t,
) -> core::arch::aarch64::uint8x8_t {
    use core::arch::aarch64::*;
    let a = vreinterpretq_s16_u16(vmovl_u8(a));
    let b = vreinterpretq_s16_u16(vmovl_u8(b));
    let c = vreinterpretq_s16_u16(vmovl_u8(c));
    let p = vsubq_s16(vaddq_s16(a, b), c);
    let da = vabdq_s16(p, a);
    let db = vabdq_s16(p, b);
    let dc = vabdq_s16(p, c);
    let choose_a = vandq_u16(vcleq_s16(da, db), vcleq_s16(da, dc));
    let choose_b = vcleq_s16(db, dc);
    vmovn_u16(vreinterpretq_u16_s16(vbslq_s16(
        choose_a,
        a,
        vbslq_s16(choose_b, b, c),
    )))
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
#[inline(always)]
unsafe fn paeth16(
    a: core::arch::aarch64::uint8x16_t,
    b: core::arch::aarch64::uint8x16_t,
    c: core::arch::aarch64::uint8x16_t,
) -> core::arch::aarch64::uint8x16_t {
    use core::arch::aarch64::*;
    vcombine_u8(
        paeth_lanes(vget_low_u8(a), vget_low_u8(b), vget_low_u8(c)),
        paeth_lanes(vget_high_u8(a), vget_high_u8(b), vget_high_u8(c)),
    )
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
fn score_filters(current: &[u8], previous: &[u8], channels: usize) -> [u64; 5] {
    use core::arch::aarch64::*;
    debug_assert_eq!(current.len(), previous.len());
    let mut scores = [0u64; 5];
    let mut i = 0usize;
    // The first pixel has no left or upper-left neighbour.
    while i < channels.min(current.len()) {
        for (kind, score) in scores.iter_mut().enumerate() {
            let v = u16::from(filtered(kind, current[i], 0, previous[i], 0));
            *score += u64::from(v.min(256 - v));
        }
        i += 1;
    }
    // SAFETY: every vector load is inside both equally-sized slices; the
    // shifted neighbours start at i - channels and are in bounds as well.
    unsafe {
        let mut sums = [vdupq_n_u16(0); 5];
        let mut blocks = 0usize;
        while i + 16 <= current.len() {
            let x = vld1q_u8(current.as_ptr().add(i));
            let a = vld1q_u8(current.as_ptr().add(i - channels));
            let b = vld1q_u8(previous.as_ptr().add(i));
            let c = vld1q_u8(previous.as_ptr().add(i - channels));
            let residuals = [
                x,
                vsubq_u8(x, a),
                vsubq_u8(x, b),
                vsubq_u8(x, vhaddq_u8(a, b)),
                vsubq_u8(x, paeth16(a, b, c)),
            ];
            for (kind, value) in residuals.into_iter().enumerate() {
                // -128 remains 0x80 and is interpreted as unsigned 128.
                let magnitude = vreinterpretq_u8_s8(vabsq_s8(vreinterpretq_s8_u8(value)));
                sums[kind] = vpadalq_u8(sums[kind], magnitude);
            }
            i += 16;
            blocks += 1;
            if blocks == 255 {
                // Each lane receives at most 256 per block, so 255 blocks
                // are bounded by 65 280, below the u16 overflow boundary.
                for kind in 0..5 {
                    scores[kind] += u64::from(vaddlvq_u16(sums[kind]));
                    sums[kind] = vdupq_n_u16(0);
                }
                blocks = 0;
            }
        }
        for kind in 0..5 {
            scores[kind] += u64::from(vaddlvq_u16(sums[kind]));
        }
    }
    while i < current.len() {
        let (a, b, c) = (current[i - channels], previous[i], previous[i - channels]);
        for (kind, score) in scores.iter_mut().enumerate() {
            let v = u16::from(filtered(kind, current[i], a, b, c));
            *score += u64::from(v.min(256 - v));
        }
        i += 1;
    }
    scores
}

#[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
fn score_filters(current: &[u8], previous: &[u8], channels: usize) -> [u64; 5] {
    score_filters_scalar(current, previous, channels)
}

fn append_filtered(
    raw: &mut Vec<u8>,
    current: &[u8],
    previous: &[u8],
    channels: usize,
    kind: usize,
) {
    if kind == 0 {
        raw.extend_from_slice(current);
        return;
    }
    let mut i = 0usize;
    while i < channels.min(current.len()) {
        raw.push(filtered(kind, current[i], 0, previous[i], 0));
        i += 1;
    }
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe {
        use core::arch::aarch64::*;
        while i + 16 <= current.len() {
            let x = vld1q_u8(current.as_ptr().add(i));
            let a = vld1q_u8(current.as_ptr().add(i - channels));
            let b = vld1q_u8(previous.as_ptr().add(i));
            let c = vld1q_u8(previous.as_ptr().add(i - channels));
            let prediction = match kind {
                1 => a,
                2 => b,
                3 => vhaddq_u8(a, b),
                4 => paeth16(a, b, c),
                _ => unreachable!(),
            };
            let mut bytes = [0u8; 16];
            vst1q_u8(bytes.as_mut_ptr(), vsubq_u8(x, prediction));
            raw.extend_from_slice(&bytes);
            i += 16;
        }
    }
    while i < current.len() {
        raw.push(filtered(
            kind,
            current[i],
            current[i - channels],
            previous[i],
            previous[i - channels],
        ));
        i += 1;
    }
}

fn zeros(size: usize) -> Result<Vec<u8>, Error> {
    let mut row = Vec::new();
    row.try_reserve_exact(size)
        .map_err(|_| "PNG: allocation failed")?;
    row.resize(size, 0);
    Ok(row)
}

/// Inspect alpha in bounded groups without changing the first non-opaque
/// decision. Input length was checked against complete RGBA pixels.
fn all_opaque(input: &[u8]) -> bool {
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut at = 0usize;
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe {
        use core::arch::aarch64::{vld4q_u8, vminvq_u8};
        while at + 64 <= input.len() {
            // SAFETY: the guard covers sixteen complete RGBA pixels.
            if vminvq_u8(vld4q_u8(input.as_ptr().add(at)).3) != 255 {
                return false;
            }
            at += 64;
        }
    }
    input[at..]
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255)
}

/// Produce the same packed RGB bytes from a validated RGBA row.
fn opaque_rgb_row(row: &[u8], rgb: &mut [u8]) {
    debug_assert_eq!(row.len() / 4 * 3, rgb.len());
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut src = 0usize;
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut dst = 0usize;
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe {
        use core::arch::aarch64::{uint8x16x3_t, vld4q_u8, vst3q_u8};
        while src + 64 <= row.len() {
            // SAFETY: each load covers sixteen complete input pixels and
            // each store covers the corresponding sixteen RGB pixels.
            let rgba = vld4q_u8(row.as_ptr().add(src));
            vst3q_u8(
                rgb.as_mut_ptr().add(dst),
                uint8x16x3_t(rgba.0, rgba.1, rgba.2),
            );
            src += 64;
            dst += 48;
        }
    }
    for (pixel, dest) in row[src..]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(rgb[dst..].chunks_exact_mut(3))
    {
        dest.copy_from_slice(&pixel[..3]);
    }
}

#[inline]
fn append_best_row(raw: &mut Vec<u8>, current: &[u8], previous: &[u8], channels: usize) {
    // Keep the original strict comparison and therefore the same tie order.
    let scores = score_filters(current, previous, channels);
    let mut best = 0;
    for kind in 1..5 {
        if scores[kind] < scores[best] {
            best = kind;
        }
    }
    raw.push(best as u8);
    append_filtered(raw, current, previous, channels, best);
}

/// Produce a valid, lossless PNG with a bounded in-tree compressor.
pub fn encode(image: &Image) -> Result<Buffer, Error> {
    let (width, height) = (image.width, image.height);
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || width.checked_mul(height).ok_or("PNG: size too large")? > MAX_PIXELS
    {
        return Err("PNG: dimensions out of bounds");
    }
    let input_stride = width.checked_mul(4).ok_or("PNG: size too large")?;
    let input_len = input_stride
        .checked_mul(height)
        .ok_or("PNG: size too large")?;
    let input = image.pixels.as_slice();
    if input.len() != input_len {
        return Err("PNG: inconsistent pixels");
    }
    let opaque = all_opaque(input);
    let channels = if opaque { 3 } else { 4 };
    let stride = width.checked_mul(channels).ok_or("PNG: size too large")?;
    let raw_len = stride
        .checked_add(1)
        .and_then(|n| n.checked_mul(height))
        .ok_or("PNG: size too large")?;
    if raw_len > MAX_FILE {
        return Err("PNG: data too large");
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(raw_len)
        .map_err(|_| "PNG: allocation failed")?;
    let mut previous = zeros(stride)?;
    if opaque {
        let mut current = zeros(stride)?;
        for row in input.chunks_exact(input_stride) {
            opaque_rgb_row(row, &mut current);
            append_best_row(&mut raw, &current, &previous, channels);
            std::mem::swap(&mut current, &mut previous);
        }
    } else {
        for y in 0..height {
            let row = &input[y * input_stride..(y + 1) * input_stride];
            let above = if y == 0 {
                previous.as_slice()
            } else {
                &input[(y - 1) * input_stride..y * input_stride]
            };
            append_best_row(&mut raw, row, above, channels);
        }
    }
    debug_assert_eq!(raw.len(), raw_len);
    drop(previous);
    let compressed = deflate_encode::compress(&raw)?;
    drop(raw);
    let total = compressed
        .len()
        .checked_add(57)
        .ok_or("PNG: size too large")?;
    if total > MAX_FILE || compressed.len() > u32::MAX as usize {
        return Err("PNG: output too large");
    }
    let mut result = Buffer::new(total)?;
    let out = result.as_mut_slice();
    out[..8].copy_from_slice(SIGNATURE);
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&(width as u32).to_be_bytes());
    header[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    header[8] = 8;
    header[9] = if opaque { 2 } else { 6 };
    let pos = write_chunk(out, 8, b"IHDR", &header);
    let pos = write_chunk(out, pos, b"IDAT", &compressed);
    let pos = write_chunk(out, pos, b"IEND", &[]);
    if pos != total {
        return Err("PNG: inconsistent encoded size");
    }
    Ok(result)
}

#[cfg(test)]
mod lane_tests {
    use super::*;
    #[test]
    fn vector_filter_scores_match_the_scalar_reference_at_lane_boundaries() {
        for channels in [3, 4] {
            for width in [1, 3, 4, 5, 15, 16, 17, 31, 32, 33, 257] {
                let current = (0..width * channels)
                    .map(|i| (i * 73 + 19) as u8)
                    .collect::<Vec<_>>();
                let previous = (0..width * channels)
                    .map(|i| (i * 137 + 251) as u8)
                    .collect::<Vec<_>>();
                assert_eq!(
                    score_filters(&current, &previous, channels),
                    score_filters_scalar(&current, &previous, channels),
                    "{width} pixels, {channels} channels"
                );
            }
        }
    }
}
