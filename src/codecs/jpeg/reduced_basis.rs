use super::*;
pub(crate) const fn reduced_basis<const N: usize, const FACTOR: usize>() -> ReducedBasis<N> {
    let mut result = [[0.0; 8]; N];
    let mut x = 0;
    while x < N {
        let mut u = 0;
        while u < 8 {
            let mut i = 0;
            let mut sum = 0.0;
            while i < FACTOR {
                sum += BASIS[x * FACTOR + i][u];
                i += 1;
            }
            result[x][u] = sum / FACTOR as f64;
            u += 1;
        }
        x += 1;
    }
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    let pairs = {
        let mut pairs = [[[0.0; 2]; 8]; 2];
        let mut x = 0;
        while x < N / 2 && x < 2 {
            let mut u = 0;
            while u < 8 {
                pairs[x][u] = [result[x * 2][u], result[x * 2 + 1][u]];
                u += 1;
            }
            x += 1;
        }
        pairs
    };
    ReducedBasis {
        values: result,
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        pairs,
    }
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn inverse_dct_reduced_neon<const N: usize, const ROWS: usize>(
    coeff: &[f64; 64],
    row_mask: u8,
    basis: &ReducedBasis<N>,
    out: &mut [u8; 64],
) {
    use core::arch::aarch64::{vaddq_f64, vdupq_n_f64, vld1q_f64, vmulq_n_f64, vst1q_f64};
    let mut work = [[0.0f64; N]; ROWS];
    for v in 0..ROWS {
        if row_mask & (1 << v) == 0 {
            continue;
        }
        for x in 0..N / 2 {
            let mut sum = vdupq_n_f64(0.0);
            for u in 0..8 {
                let pair = vld1q_f64(basis.pairs[x][u].as_ptr());
                sum = vaddq_f64(sum, vmulq_n_f64(pair, coeff[v * 8 + u]));
            }
            vst1q_f64(work[v].as_mut_ptr().add(x * 2), sum);
        }
    }
    for y in 0..N {
        for x in 0..N / 2 {
            let mut sum = vdupq_n_f64(0.0);
            for v in 0..ROWS {
                let pair = vld1q_f64(work[v].as_ptr().add(x * 2));
                sum = vaddq_f64(sum, vmulq_n_f64(pair, basis.values[y][v]));
            }
            // Preserve every multiply and addition from the eight-row
            // reference, including the sign of an inactive zero term.
            let zero = vdupq_n_f64(0.0);
            for v in ROWS..8 {
                sum = vaddq_f64(sum, vmulq_n_f64(zero, basis.values[y][v]));
            }
            let value = vaddq_f64(vmulq_n_f64(sum, 0.25), vdupq_n_f64(128.0));
            let value = vaddq_f64(value, vdupq_n_f64(0.5));
            let pair = core::arch::aarch64::vcvtq_u64_f64(value);
            out[y * 8 + x * 2] = core::arch::aarch64::vgetq_lane_u64::<0>(pair).min(255) as u8;
            out[y * 8 + x * 2 + 1] = core::arch::aarch64::vgetq_lane_u64::<1>(pair).min(255) as u8;
        }
    }
}
pub(crate) fn inverse_dct_reduced<const N: usize>(
    coeff: &[f64; 64],
    row_mask: u8,
    has_ac: bool,
    basis: &ReducedBasis<N>,
    out: &mut [u8; 64],
) {
    if !has_ac {
        let dc = clamp_round(coeff[0] * 0.125 + 128.0);
        for row in out.as_chunks_mut::<8>().0.iter_mut().take(N) {
            row[..N].fill(dc);
        }
        return;
    }
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    if N == 2 || N == 4 {
        // SAFETY: on AArch64 NEON is available; N is even and at most four,
        // so every loaded/stored pair lies within the fixed-size rows.
        // The decoder already records the nonzero coefficient rows. Select
        // an upper bound of 2, 4 or 8 rows: remaining rows are known zero.
        // The vertical pass still sums all eight terms in reference order.
        unsafe {
            if row_mask & 0xfc == 0 {
                inverse_dct_reduced_neon::<N, 2>(coeff, row_mask, basis, out);
            } else if row_mask & 0xf0 == 0 {
                inverse_dct_reduced_neon::<N, 4>(coeff, row_mask, basis, out);
            } else {
                inverse_dct_reduced_neon::<N, 8>(coeff, row_mask, basis, out);
            }
        };
        return;
    }
    let mut work = [[0.0f64; N]; 8];
    for v in 0..8 {
        if row_mask & (1 << v) == 0 {
            continue;
        }
        for x in 0..N {
            let mut sum = 0.0;
            for u in 0..8 {
                sum += basis.values[x][u] * coeff[v * 8 + u];
            }
            work[v][x] = sum;
        }
    }
    for y in 0..N {
        for x in 0..N {
            let mut sum = 0.0;
            for v in 0..8 {
                sum += basis.values[y][v] * work[v][x];
            }
            out[y * 8 + x] = clamp_round(sum * 0.25 + 128.0);
        }
    }
}
pub(crate) fn block(
    bits: &mut Entropy<'_>,
    component: &mut Component,
    quant: &[u16; 64],
    dc: &Huffman,
    ac: &Huffman,
    out: &mut [u8; 64],
    shrink: usize,
) -> Result<(), Error> {
    let (_, difference) = dc.token::<false>(bits)?;
    component.predictor = component
        .predictor
        .checked_add(difference)
        .ok_or("JPEG: invalid DC predictor")?;
    if component.predictor < -(1 << 20) || component.predictor > (1 << 20) {
        return Err("JPEG: DC predictor out of bounds");
    }
    let mut coeff = [0.0f64; 64];
    coeff[0] = component.predictor as f64 * quant[0] as f64;
    let mut k = 1usize;
    let mut has_ac = false;
    let mut row_mask = 1u8;
    while k < 64 {
        let (symbol, amplitude) = ac.token::<true>(bits)?;
        let run = (symbol >> 4) as usize;
        let size = symbol & 15;
        if size == 0 {
            if run == 0 {
                break;
            }
            if run != 15 || k + 16 > 64 {
                return Err("JPEG: invalid AC sequence");
            }
            k += 16;
            continue;
        }
        k += run;
        if k >= 64 {
            return Err("JPEG: AC sequence out of bounds");
        }
        let j = ZIGZAG[k];
        coeff[j] = amplitude as f64 * quant[j] as f64;
        row_mask |= 1 << (j / 8);
        has_ac = true;
        k += 1;
    }
    match shrink {
        1 => inverse_dct(&coeff, row_mask, has_ac, out),
        2 => inverse_dct_reduced(&coeff, row_mask, has_ac, &REDUCED_2, out),
        4 => inverse_dct_reduced(&coeff, row_mask, has_ac, &REDUCED_4, out),
        // The average of an 8x8 IDCT block contains only its DC term.
        8 => out[0] = clamp_round(coeff[0] * 0.125 + 128.0),
        _ => return Err("JPEG: invalid reduction factor"),
    }
    Ok(())
}
// Compute an axis's component factor that reaches the requested resolution
// when its sampling ratio is a power of two. The actual IDCT factor is the
// maximum required by either axis, so in 4:2:2 the horizontal chroma can remain
// subsampled and must still be interpolated. Unusual ratios use the full factor.
pub(crate) fn component_shrink(shrink: usize, sampling: usize, maximum: usize) -> usize {
    if maximum.is_multiple_of(sampling) {
        let ratio = maximum / sampling;
        if ratio.is_power_of_two() {
            return (shrink / ratio).max(1);
        }
    }
    shrink
}
pub(crate) fn parse_frame(
    bytes: &[u8],
    components: &mut [Component; 3],
) -> Result<(usize, usize, usize, usize, usize), Error> {
    if bytes.len() < 6 || bytes[0] != 8 {
        return Err("JPEG: only 8-bit precision is supported");
    }
    let height = read_u16(bytes, 1)?;
    let width = read_u16(bytes, 3)?;
    let count = bytes[5] as usize;
    if count != 1 && count != 3 {
        return Err("JPEG: unsupported component count");
    }
    if bytes.len() != 6 + count * 3 {
        return Err("JPEG: invalid frame header");
    }
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .filter(|&v| v <= MAX_PIXELS)
            .is_none()
    {
        return Err("JPEG: dimensions out of bounds");
    }
    let mut max_h = 0usize;
    let mut max_v = 0usize;
    let mut blocks = 0usize;
    for i in 0..count {
        let p = 6 + i * 3;
        let h = (bytes[p + 1] >> 4) as usize;
        let v = (bytes[p + 1] & 15) as usize;
        let quant = bytes[p + 2] as usize;
        if h == 0 || h > 4 || v == 0 || v > 4 || quant >= 4 {
            return Err("JPEG: invalid sampling");
        }
        for c in &components[..i] {
            if c.id == bytes[p] {
                return Err("JPEG: duplicate component");
            }
        }
        components[i] = Component {
            id: bytes[p],
            h,
            v,
            quant,
            ..Component::EMPTY
        };
        max_h = max_h.max(h);
        max_v = max_v.max(v);
        blocks += h * v;
    }
    if blocks > 10 || (count == 1 && (max_h != 1 || max_v != 1)) {
        return Err("JPEG: unsupported sampling");
    }
    for c in &components[..count] {
        if !max_h.is_multiple_of(c.h) || !max_v.is_multiple_of(c.v) {
            return Err("JPEG: fractional sampling unsupported");
        }
    }
    Ok((width, height, count, max_h, max_v))
}
pub(crate) fn parse_scan(
    bytes: &[u8],
    components: &mut [Component; 3],
    count: usize,
) -> Result<[usize; 3], Error> {
    if bytes.len() != 1 + count * 2 + 3 || bytes[0] as usize != count {
        return Err("JPEG: separate or incomplete scans unsupported");
    }
    if bytes[1 + count * 2] != 0 || bytes[2 + count * 2] != 63 || bytes[3 + count * 2] != 0 {
        return Err("JPEG: unsupported scan mode");
    }
    let mut order = [0usize; 3];
    let mut used = [false; 3];
    for i in 0..count {
        let id = bytes[1 + i * 2];
        let table = bytes[2 + i * 2];
        let mut found = None;
        for j in 0..count {
            if components[j].id == id {
                found = Some(j);
                break;
            }
        }
        let j = found.ok_or("JPEG: unknown scan component")?;
        if used[j] || table >> 4 >= 4 || table & 15 >= 4 {
            return Err("JPEG: invalid scan");
        }
        used[j] = true;
        components[j].dc = (table >> 4) as usize;
        components[j].ac = (table & 15) as usize;
        order[i] = j;
    }
    Ok(order)
}
// JPEG sample positions are centred in their source pixel regions. Use fixed
// point so chroma interpolates across MCU boundaries without a math library.
pub(crate) fn sample_axis(
    pos: usize,
    factor: usize,
    maximum: usize,
    limit: usize,
) -> (usize, usize, u32) {
    let coord = (((pos * 2 + 1) * factor * 128) / maximum) as isize - 128;
    if coord <= 0 {
        return (0, 0, 0);
    }
    let lower = (coord as usize) >> 8;
    if lower >= limit - 1 {
        return (limit - 1, limit - 1, 0);
    }
    (lower, lower + 1, (coord as u32) & 255)
}
pub(crate) fn sample_plane(
    data: &[u8],
    offset: usize,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    component: Component,
    max_h: usize,
    max_v: usize,
) -> u8 {
    let (x0, x1, wx) = sample_axis(x, component.h, max_h, width);
    let (y0, y1, wy) = sample_axis(y, component.v, max_v, height);
    let top = data[offset + y0 * width + x0] as u32 * (256 - wx)
        + data[offset + y0 * width + x1] as u32 * wx;
    let bottom = data[offset + y1 * width + x0] as u32 * (256 - wx)
        + data[offset + y1 * width + x1] as u32 * wx;
    ((top * (256 - wy) + bottom * wy + 32768) >> 16) as u8
}
// Specialized form of sample_axis for 2:1 chroma. All weights are integer
// quarters, so the interpolation below is mathematically identical to the
// generic 8-bit fixed-point path, without per-pixel integer divisions.
#[inline]
pub(crate) fn half_axis(pos: usize, limit: usize) -> (usize, usize, u32) {
    if pos == 0 {
        return (0, 0, 0);
    }
    let lower = (pos - 1) / 2;
    if lower >= limit - 1 {
        return (limit - 1, limit - 1, 0);
    }
    (lower, lower + 1, if pos & 1 == 0 { 3 } else { 1 })
}
#[inline]
pub(crate) fn vertical_quarters(
    plane: &[u8],
    start: usize,
    stride: usize,
    x: usize,
    y0: usize,
    y1: usize,
    wy: u32,
) -> u32 {
    plane[start + y0 * stride + x] as u32 * (4 - wy) + plane[start + y1 * stride + x] as u32 * wy
}
#[inline]
pub(crate) fn store_rgb(target: &mut [u8], offset: usize, y: u8, cb: u8, cr: u8) {
    let luminance = (y as i32) << 22;
    let cb = cb as i32 - 128;
    let cr = cr as i32 - 128;
    // These fixed-point constants are checked exhaustively against the prior
    // f64 conversion over all 256^3 inputs by jpeg_v5_color_math.py. The two
    // green half-way cases need the original evaluation order; the single
    // blue half-way case has an exact integer result after rounding.
    let red = (luminance + 5_880_414 * cr + (1 << 21)) >> 22;
    let green = if (cb == -50 && cr == 50) || (cb == 50 && cr == -50) {
        clamp_round(y as f64 - 0.344136 * cb as f64 - 0.714136 * cr as f64)
    } else {
        ((luminance - 1_443_411 * cb - 2_995_303 * cr + (1 << 21)) >> 22).clamp(0, 255) as u8
    };
    let blue = if cb == -125 {
        y.saturating_sub(221)
    } else {
        ((luminance + 7_432_307 * cb + (1 << 21)) >> 22).clamp(0, 255) as u8
    };
    target[offset] = red.clamp(0, 255) as u8;
    target[offset + 1] = green;
    target[offset + 2] = blue;
    target[offset + 3] = 255;
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rgb_four(
    y: core::arch::aarch64::uint16x4_t,
    cb: core::arch::aarch64::uint16x4_t,
    cr: core::arch::aarch64::uint16x4_t,
) -> (
    core::arch::aarch64::uint16x4_t,
    core::arch::aarch64::uint16x4_t,
    core::arch::aarch64::uint16x4_t,
) {
    use core::arch::aarch64::*;
    let y = vreinterpretq_s32_u32(vmovl_u16(y));
    let cb = vsubq_s32(vreinterpretq_s32_u32(vmovl_u16(cb)), vdupq_n_s32(128));
    let cr = vsubq_s32(vreinterpretq_s32_u32(vmovl_u16(cr)), vdupq_n_s32(128));
    let base = vaddq_s32(vshlq_n_s32::<22>(y), vdupq_n_s32(1 << 21));
    let red = vaddq_s32(base, vmulq_n_s32(cr, 5_880_414));
    let green = vsubq_s32(
        vsubq_s32(base, vmulq_n_s32(cb, 1_443_411)),
        vmulq_n_s32(cr, 2_995_303),
    );
    let blue = vaddq_s32(base, vmulq_n_s32(cb, 7_432_307));
    (
        vqmovun_s32(vshrq_n_s32::<22>(red)),
        vqmovun_s32(vshrq_n_s32::<22>(green)),
        vqmovun_s32(vshrq_n_s32::<22>(blue)),
    )
}
