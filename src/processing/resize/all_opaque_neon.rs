use super::*;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn all_opaque_neon(src: &[u8]) -> bool {
    use core::arch::aarch64::*;
    let mut chunks = src.chunks_exact(16 * CHANNELS);
    for chunk in &mut chunks {
        // SAFETY: each chunk contains exactly 16 complete RGBA pixels.
        let alpha = vld4q_u8(chunk.as_ptr()).3;
        if vminvq_u8(alpha) != 255 {
            return false;
        }
    }
    chunks
        .remainder()
        .as_chunks::<CHANNELS>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255)
}
#[inline]
pub(crate) fn all_opaque(src: &[u8]) -> bool {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: the aarch64-apple-darwin target supports NEON, and the
        // vector reader only accesses complete blocks.
        unsafe { all_opaque_neon(src) }
    }
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = src;
        false
    }
}
pub(crate) fn nearest(image: Image, width: usize, height: usize) -> Result<Image, Error> {
    let mut pixels = Buffer::new(width * height * CHANNELS)?;
    let src = image.pixels.as_slice();
    let dst = pixels.as_mut_slice();
    for y in 0..height {
        let sy = (((2 * y + 1) * image.height) / (2 * height)).min(image.height - 1);
        for x in 0..width {
            let sx = (((2 * x + 1) * image.width) / (2 * width)).min(image.width - 1);
            let src_i = (sy * image.width + sx) * CHANNELS;
            let dst_i = (y * width + x) * CHANNELS;
            dst[dst_i..dst_i + CHANNELS].copy_from_slice(&src[src_i..src_i + CHANNELS]);
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
/// Compute one horizontally filtered source row. RGB is premultiplied by alpha
/// before filtering, avoiding colored halos at transparent edges.
pub(crate) fn horizontal_scalar(
    src: &[u8],
    src_width: usize,
    src_y: usize,
    plan: &Plan,
    out: &mut [f32],
) {
    let spans = typed::<Span>(&plan.spans);
    for (dx, span) in spans.iter().enumerate() {
        let mut r = 0.0f32;
        let mut g = 0.0f32;
        let mut b = 0.0f32;
        let mut a = 0.0f32;
        for n in 0..span.count {
            let i = (src_y * src_width + span.first + n) * CHANNELS;
            let weight = plan.weight(span, n);
            let alpha = src[i + 3] as f32;
            let alpha_weight = alpha * weight / 255.0;
            r += src[i] as f32 * alpha_weight;
            g += src[i + 1] as f32 * alpha_weight;
            b += src[i + 2] as f32 * alpha_weight;
            a += alpha * weight;
        }
        let base = dx * CHANNELS;
        out[base] = r;
        out[base + 1] = g;
        out[base + 2] = b;
        out[base + 3] = a;
    }
}
/// Process one cached-weight row using four vector lanes for RGBA. For opaque
/// input, the cached coefficients include the *same* f32 alpha/255 operation
/// as the scalar path, and the alpha sum is computed once per destination x.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn horizontal_neon<const OPAQUE: bool>(
    src: &[u8],
    src_width: usize,
    src_y: usize,
    plan: &Plan,
    weights: &[f32],
    out: &mut [f32],
) {
    use core::arch::aarch64::*;

    let spans = typed::<Span>(&plan.spans);
    let mut dx = 0usize;
    while dx < spans.len() {
        let span = &spans[dx];
        let start = (src_y * src_width + span.first) * CHANNELS;
        if OPAQUE && span.fast_stride != 0 && dx + 4 <= spans.len() {
            let coefficients = &weights[span.weight_offset..span.weight_offset + span.count];
            let mut red = vdupq_n_f32(0.0);
            let mut green = vdupq_n_f32(0.0);
            let mut blue = vdupq_n_f32(0.0);
            let mask = vdupq_n_u32(255);
            macro_rules! accumulate {
                ($packed:expr, $weight:expr) => {{
                    let packed = $packed;
                    let weight = $weight;
                    let r = vcvtq_f32_u32(vandq_u32(packed, mask));
                    let g = vcvtq_f32_u32(vandq_u32(vshrq_n_u32::<8>(packed), mask));
                    let b = vcvtq_f32_u32(vandq_u32(vshrq_n_u32::<16>(packed), mask));
                    red = vaddq_f32(red, vmulq_n_f32(r, weight));
                    green = vaddq_f32(green, vmulq_n_f32(g, weight));
                    blue = vaddq_f32(blue, vmulq_n_f32(b, weight));
                }};
            }
            let mut n = 0usize;
            if span.fast_stride == 2 {
                while n + 2 <= coefficients.len() {
                    // SAFETY: the plan checked every interleaved load stays
                    // inside this source row, including any unused lane.
                    let p = src.as_ptr().add(start + n * CHANNELS).cast::<u32>();
                    let data = vld2q_u32(p);
                    accumulate!(data.0, coefficients[n]);
                    accumulate!(data.1, coefficients[n + 1]);
                    n += 2;
                }
            } else {
                while n + 4 <= coefficients.len() {
                    let p = src.as_ptr().add(start + n * CHANNELS).cast::<u32>();
                    let data = vld4q_u32(p);
                    accumulate!(data.0, coefficients[n]);
                    accumulate!(data.1, coefficients[n + 1]);
                    accumulate!(data.2, coefficients[n + 2]);
                    accumulate!(data.3, coefficients[n + 3]);
                    n += 4;
                }
            }
            while n < coefficients.len() {
                let p = src.as_ptr().add(start + n * CHANNELS).cast::<u32>();
                let packed = if span.fast_stride == 2 {
                    vld2q_u32(p).0
                } else {
                    vld4q_u32(p).0
                };
                accumulate!(packed, coefficients[n]);
                n += 1;
            }
            let alpha = vdupq_n_f32(span.opaque_alpha);
            // SAFETY: the branch requires four complete destination pixels.
            vst4q_f32(
                out.as_mut_ptr().add(dx * CHANNELS),
                float32x4x4_t(red, green, blue, alpha),
            );
            dx += 4;
            continue;
        }
        let pixels = &src[start..start + span.count * CHANNELS];
        let weights = &weights[span.weight_offset..span.weight_offset + span.count];
        let mut accum = vdupq_n_f32(0.0);
        for (n, &weight) in weights.iter().enumerate() {
            // SAFETY: pixels is a checked slice with exactly count RGBA
            // pixels; n is bounded by the equally long weights slice.
            let packed = u32::from_le(core::ptr::read_unaligned(
                pixels.as_ptr().add(n * CHANNELS).cast::<u32>(),
            ));
            let channels8 = vcreate_u8(packed as u64);
            let channels16 = vget_low_u16(vmovl_u8(channels8));
            let channels = vcvtq_f32_u32(vmovl_u16(channels16));
            let factors = if OPAQUE {
                vdupq_n_f32(weight)
            } else {
                let alpha = (packed >> 24) as f32;
                let alpha_weight = alpha * weight / 255.0;
                vsetq_lane_f32::<3>(weight, vdupq_n_f32(alpha_weight))
            };
            accum = vaddq_f32(accum, vmulq_f32(channels, factors));
        }
        if OPAQUE {
            accum = vsetq_lane_f32::<3>(span.opaque_alpha, accum);
        }
        // SAFETY: the caller allocated one RGBA f32 vector per span.
        vst1q_f32(out.as_mut_ptr().add(dx * CHANNELS), accum);
        dx += 1;
    }
}
pub(crate) fn horizontal(src: &[u8], src_width: usize, src_y: usize, plan: &Plan, out: &mut [f32]) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    if let Some(weights) = &plan.weights {
        // SAFETY: aarch64-apple-darwin supports NEON; all source and
        // destination slices are bounded and valid for their lengths.
        unsafe {
            if plan.opaque_weights {
                horizontal_neon::<true>(src, src_width, src_y, plan, typed::<f32>(weights), out);
            } else {
                horizontal_neon::<false>(src, src_width, src_y, plan, typed::<f32>(weights), out);
            }
        }
        return;
    }
    horizontal_scalar(src, src_width, src_y, plan, out);
}
/// For exact x2/x4 reductions, store each color's source pixels by phase:
/// phase[p][i] is pixel p + i * step. u8 -> f32 is exact; taps retain their
/// previous order and the same rounded coefficients.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rgb_to_phase_planes(rgb: &[u8], planes: &mut [f32], step: usize) {
    use core::arch::aarch64::*;
    let width = rgb.len() / 3;
    debug_assert!(step == 2 || step == 4);
    debug_assert_eq!(width % step, 0);
    let pitch = width / step;
    let (red, rest) = planes.split_at_mut(width);
    let (green, blue) = rest.split_at_mut(width);
    unsafe fn store4(v: uint8x16_t, p: *mut f32) {
        let wide = vmovl_u8(vget_low_u8(v));
        vst1q_f32(p, vcvtq_f32_u32(vmovl_u16(vget_low_u16(wide))));
    }
    unsafe fn store8(v: uint8x16_t, p: *mut f32) {
        let wide = vmovl_u8(vget_low_u8(v));
        vst1q_f32(p, vcvtq_f32_u32(vmovl_u16(vget_low_u16(wide))));
        vst1q_f32(p.add(4), vcvtq_f32_u32(vmovl_u16(vget_high_u16(wide))));
    }
    unsafe fn store_phases(v: uint8x16_t, out: *mut f32, pitch: usize, offset: usize, step: usize) {
        let even_odd = vuzpq_u8(v, v);
        if step == 2 {
            store8(even_odd.0, out.add(offset));
            store8(even_odd.1, out.add(pitch + offset));
        } else {
            let even = vuzpq_u8(even_odd.0, even_odd.0);
            let odd = vuzpq_u8(even_odd.1, even_odd.1);
            store4(even.0, out.add(offset));
            store4(odd.0, out.add(pitch + offset));
            store4(even.1, out.add(2 * pitch + offset));
            store4(odd.1, out.add(3 * pitch + offset));
        }
    }
    let mut x = 0;
    while x + 16 <= width {
        // SAFETY: the complete input block and all phase stores cover
        // exactly these 16 in-bounds pixels in three distinct planes.
        let pixels = vld3q_u8(rgb.as_ptr().add(x * 3));
        store_phases(pixels.0, red.as_mut_ptr(), pitch, x / step, step);
        store_phases(pixels.1, green.as_mut_ptr(), pitch, x / step, step);
        store_phases(pixels.2, blue.as_mut_ptr(), pitch, x / step, step);
        x += 16;
    }
    while x < width {
        let offset = (x % step) * pitch + x / step;
        red[offset] = f32::from(rgb[x * 3]);
        green[offset] = f32::from(rgb[x * 3 + 1]);
        blue[offset] = f32::from(rgb[x * 3 + 2]);
        x += 1;
    }
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn horizontal_phase_planes<const STEP: usize>(
    planes: &[f32],
    width: usize,
    plan: &Plan,
    out: &mut [f32],
) {
    use core::arch::aarch64::*;
    let pitch = width / STEP;
    let (red, rest) = planes.split_at(width);
    let (green, blue) = rest.split_at(width);
    let weights = typed::<f32>(plan.weights.as_ref().unwrap());
    let spans = typed::<Span>(&plan.spans);
    let mut dx = 0usize;
    while dx < spans.len() {
        let span = &spans[dx];
        let coefficients = &weights[span.weight_offset..span.weight_offset + span.count];
        if span.fast_eight != 0 && usize::from(span.fast_stride) == STEP && dx + 8 <= spans.len() {
            let mut r0 = vdupq_n_f32(0.0);
            let mut g0 = vdupq_n_f32(0.0);
            let mut b0 = vdupq_n_f32(0.0);
            let mut r1 = vdupq_n_f32(0.0);
            let mut g1 = vdupq_n_f32(0.0);
            let mut b1 = vdupq_n_f32(0.0);
            for (n, &weight) in coefficients.iter().enumerate() {
                let x = span.first + n;
                let offset = (x % STEP) * pitch + x / STEP;
                // SAFETY: mark_eight requires both checked four-lane
                // groups; all eight positions are within this phase.
                r0 = vaddq_f32(r0, vmulq_n_f32(vld1q_f32(red.as_ptr().add(offset)), weight));
                g0 = vaddq_f32(
                    g0,
                    vmulq_n_f32(vld1q_f32(green.as_ptr().add(offset)), weight),
                );
                b0 = vaddq_f32(
                    b0,
                    vmulq_n_f32(vld1q_f32(blue.as_ptr().add(offset)), weight),
                );
                r1 = vaddq_f32(
                    r1,
                    vmulq_n_f32(vld1q_f32(red.as_ptr().add(offset + 4)), weight),
                );
                g1 = vaddq_f32(
                    g1,
                    vmulq_n_f32(vld1q_f32(green.as_ptr().add(offset + 4)), weight),
                );
                b1 = vaddq_f32(
                    b1,
                    vmulq_n_f32(vld1q_f32(blue.as_ptr().add(offset + 4)), weight),
                );
            }
            let alpha = vdupq_n_f32(span.opaque_alpha);
            vst4q_f32(
                out.as_mut_ptr().add(dx * CHANNELS),
                float32x4x4_t(r0, g0, b0, alpha),
            );
            vst4q_f32(
                out.as_mut_ptr().add((dx + 4) * CHANNELS),
                float32x4x4_t(r1, g1, b1, alpha),
            );
            dx += 8;
            continue;
        }
        if usize::from(span.fast_stride) == STEP && dx + 4 <= spans.len() {
            let mut r = vdupq_n_f32(0.0);
            let mut g = vdupq_n_f32(0.0);
            let mut b = vdupq_n_f32(0.0);
            for (n, &weight) in coefficients.iter().enumerate() {
                let x = span.first + n;
                let offset = (x % STEP) * pitch + x / STEP;
                // SAFETY: fast_stride was granted only when all four
                // source positions exist. Their values are contiguous in
                // the phase plane; x and the next three positions fit.
                r = vaddq_f32(r, vmulq_n_f32(vld1q_f32(red.as_ptr().add(offset)), weight));
                g = vaddq_f32(
                    g,
                    vmulq_n_f32(vld1q_f32(green.as_ptr().add(offset)), weight),
                );
                b = vaddq_f32(b, vmulq_n_f32(vld1q_f32(blue.as_ptr().add(offset)), weight));
            }
            vst4q_f32(
                out.as_mut_ptr().add(dx * CHANNELS),
                float32x4x4_t(r, g, b, vdupq_n_f32(span.opaque_alpha)),
            );
            dx += 4;
        } else {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            for (n, &weight) in coefficients.iter().enumerate() {
                let x = span.first + n;
                let offset = (x % STEP) * pitch + x / STEP;
                r += red[offset] * weight;
                g += green[offset] * weight;
                b += blue[offset] * weight;
            }
            out[dx * CHANNELS..dx * CHANNELS + CHANNELS].copy_from_slice(&[
                r,
                g,
                b,
                span.opaque_alpha,
            ]);
            dx += 1;
        }
    }
}
