use super::*;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn horizontal_phase_planes_rgb<const STEP: usize>(
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
            vst3q_f32(out.as_mut_ptr().add(dx * 3), float32x4x3_t(r0, g0, b0));
            vst3q_f32(
                out.as_mut_ptr().add((dx + 4) * 3),
                float32x4x3_t(r1, g1, b1),
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
            vst3q_f32(out.as_mut_ptr().add(dx * 3), float32x4x3_t(r, g, b));
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
            out[dx * 3..dx * 3 + 3].copy_from_slice(&[r, g, b]);
            dx += 1;
        }
    }
}
pub(crate) fn horizontal_stream(
    planes: &Option<Buffer>,
    rgba: &Option<Buffer>,
    width: usize,
    step: usize,
    plan: &Plan,
    out: &mut [f32],
) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    if let Some(planes) = planes {
        // SAFETY: the buffer holds three complete, aligned phase planes;
        // the plan has checked opaque coefficients and source bounds.
        unsafe {
            match step {
                2 => horizontal_phase_planes::<2>(typed::<f32>(planes), width, plan, out),
                4 => horizontal_phase_planes::<4>(typed::<f32>(planes), width, plan, out),
                _ => unreachable!(),
            }
        }
        return;
    }
    let _ = (planes, step);
    horizontal(rgba.as_ref().unwrap().as_slice(), width, 0, plan, out);
}
/// The three-channel path is enabled only when the existing opaque RGB phase
/// planes were successfully created. It removes alpha from the row cache.
pub(crate) fn horizontal_stream_three(
    planes: &Option<Buffer>,
    width: usize,
    step: usize,
    plan: &Plan,
    out: &mut [f32],
) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    unsafe {
        let planes = typed::<f32>(planes.as_ref().unwrap());
        match step {
            2 => horizontal_phase_planes_rgb::<2>(planes, width, plan, out),
            4 => horizontal_phase_planes_rgb::<4>(planes, width, plan, out),
            _ => unreachable!(),
        }
    }
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        let _ = (planes, width, step, plan, out);
        unreachable!()
    }
}
/// Each destination accumulator receives source rows in the same order as
/// before. SIMD changes only the independent pixel/channel dimension.
#[inline]
pub(crate) fn accumulate_weighted(sums: &mut [f32], row: &[f32], weight: f32) {
    assert_eq!(sums.len(), row.len());
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: the lengths above are equal, and the NEON helper accesses
        // only complete groups of four plus an in-bounds scalar remainder.
        unsafe { accumulate_weighted_neon(sums, row, weight) };
    }
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    {
        for (dst, &value) in sums.iter_mut().zip(row) {
            *dst += value * weight;
        }
    }
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn accumulate_weighted_neon(sums: &mut [f32], row: &[f32], weight: f32) {
    use core::arch::aarch64::*;
    let mut i = 0usize;
    while i + 4 <= sums.len() {
        // Preserve two separately rounded operations. Do not use FMA.
        let source = vld1q_f32(row.as_ptr().add(i));
        let previous = vld1q_f32(sums.as_ptr().add(i));
        let product = vmulq_n_f32(source, weight);
        let result = vaddq_f32(previous, product);
        vst1q_f32(sums.as_mut_ptr().add(i), result);
        i += 4;
    }
    while i < sums.len() {
        sums[i] += row[i] * weight;
        i += 1;
    }
}
/// Preserve the existing premultiplied-alpha arithmetic and byte rounding.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rgba_from_sums_neon(sums: &[f32], row_pixels: &mut [u8]) -> usize {
    use core::arch::aarch64::*;
    // Saturating conversion followed by narrowing implements byte(x), including
    // NaN, infinities and negative values, without changing f32 arithmetic.
    unsafe fn bytes(x: float32x4_t, valid: uint32x4_t) -> uint8x8_t {
        let rounded = vaddq_f32(x, vdupq_n_f32(0.5));
        let value = vandq_u32(vcvtq_u32_f32(rounded), valid);
        vqmovn_u16(vcombine_u16(vqmovn_u32(value), vdup_n_u16(0)))
    }
    let mut i = 0;
    while i + 16 <= sums.len() {
        // Four complete interleaved RGBA float samples and four output pixels.
        let values = vld4q_f32(sums.as_ptr().add(i));
        let valid = vcgtq_f32(values.3, vdupq_n_f32(1e-6));
        let scale = vdivq_f32(vdupq_n_f32(255.0), values.3);
        let red = bytes(vmulq_f32(values.0, scale), valid);
        let green = bytes(vmulq_f32(values.1, scale), valid);
        let blue = bytes(vmulq_f32(values.2, scale), valid);
        let alpha = bytes(values.3, valid);
        let rg = vzip_u8(red, green).0;
        let ba = vzip_u8(blue, alpha).0;
        let rgba = vzip_u16(vreinterpret_u16_u8(rg), vreinterpret_u16_u8(ba));
        vst1q_u8(
            row_pixels.as_mut_ptr().add(i),
            vcombine_u8(vreinterpret_u8_u16(rgba.0), vreinterpret_u8_u16(rgba.1)),
        );
        i += 16;
    }
    i
}
pub(crate) fn rgba_from_sums(sums: &[f32], row_pixels: &mut [u8]) {
    debug_assert_eq!(sums.len(), row_pixels.len());
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    // SAFETY: both slices have equal lengths, with four values per pixel.
    let first = unsafe { rgba_from_sums_neon(sums, row_pixels) };
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    let first = 0;
    for (values, pixel) in sums[first..]
        .as_chunks::<CHANNELS>()
        .0
        .iter()
        .zip(row_pixels[first..].chunks_exact_mut(CHANNELS))
    {
        let alpha = values[3];
        if alpha > 1e-6 {
            let unpremultiply = 255.0 / alpha;
            pixel[0] = byte(values[0] * unpremultiply);
            pixel[1] = byte(values[1] * unpremultiply);
            pixel[2] = byte(values[2] * unpremultiply);
            pixel[3] = byte(alpha);
        } else {
            pixel.fill(0);
        }
    }
}
/// Reconstruct the alpha originally accumulated alongside every source pixel.
/// `alpha_table` contains the exact per-output-row alpha sums for each
/// horizontal pattern, and the map selects one pattern for each pixel.
pub(crate) fn rgba_from_three_sums(
    sums: &[f32],
    map: &[u8],
    alpha_table: &[f32],
    pixels: &mut [u8],
) {
    debug_assert_eq!(sums.len(), map.len() * 3);
    debug_assert_eq!(pixels.len(), map.len() * 4);
    let mut at = 0usize;
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    unsafe {
        use core::arch::aarch64::*;
        unsafe fn bytes(x: float32x4_t, valid: uint32x4_t) -> uint8x8_t {
            let rounded = vaddq_f32(x, vdupq_n_f32(0.5));
            let value = vandq_u32(vcvtq_u32_f32(rounded), valid);
            vqmovn_u16(vcombine_u16(vqmovn_u32(value), vdup_n_u16(0)))
        }
        while at + 4 <= map.len() {
            let alpha_lanes = [
                alpha_table[map[at] as usize],
                alpha_table[map[at + 1] as usize],
                alpha_table[map[at + 2] as usize],
                alpha_table[map[at + 3] as usize],
            ];
            let alpha = vld1q_f32(alpha_lanes.as_ptr());
            let channels = vld3q_f32(sums.as_ptr().add(at * 3));
            let valid = vcgtq_f32(alpha, vdupq_n_f32(1e-6));
            let scale = vdivq_f32(vdupq_n_f32(255.0), alpha);
            let red = bytes(vmulq_f32(channels.0, scale), valid);
            let green = bytes(vmulq_f32(channels.1, scale), valid);
            let blue = bytes(vmulq_f32(channels.2, scale), valid);
            let a = bytes(alpha, valid);
            let rg = vzip_u8(red, green).0;
            let ba = vzip_u8(blue, a).0;
            let rgba = vzip_u16(vreinterpret_u16_u8(rg), vreinterpret_u16_u8(ba));
            vst1q_u8(
                pixels.as_mut_ptr().add(at * 4),
                vcombine_u8(vreinterpret_u8_u16(rgba.0), vreinterpret_u8_u16(rgba.1)),
            );
            at += 4;
        }
    }
    while at < map.len() {
        let alpha = alpha_table[map[at] as usize];
        let pixel = &mut pixels[at * 4..at * 4 + 4];
        if alpha > 1e-6 {
            let unpremultiply = 255.0 / alpha;
            pixel[0] = byte(sums[at * 3] * unpremultiply);
            pixel[1] = byte(sums[at * 3 + 1] * unpremultiply);
            pixel[2] = byte(sums[at * 3 + 2] * unpremultiply);
            pixel[3] = byte(alpha);
        } else {
            pixel.fill(0);
        }
        at += 1;
    }
}
/// The same filter plan and operation order serve both the general RGBA path
/// and opaque PNG rows written directly to BMP, one output stripe at a time.
pub(crate) fn filtered_into(
    source: FilterSource<'_>,
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    filter: Filter,
    mut output: FilterOutput<'_>,
) -> Result<(), Error> {
    check_filtered_work(source_width, source_height, width, height)?;
    let opaque = match &source {
        FilterSource::Rgba(src) => all_opaque(src),
        FilterSource::OpaqueRgbRows { .. } => true,
    };
    let mut rgb_row = match &source {
        FilterSource::Rgba(_) => None,
        FilterSource::OpaqueRgbRows { .. } => Some(Buffer::new(source_width * CHANNELS)?),
    };
    let mut output_row = [0u8; STRIPE * CHANNELS];
    let mut stripe_x = 0;
    while stripe_x < width {
        let stripe_w = (width - stripe_x).min(STRIPE);
        let plan = Plan::new(source_width, width, stripe_x, stripe_w, filter, opaque)?;
        let row_values = stripe_w * CHANNELS;
        let row_bytes = row_values * size_of::<f32>();
        let ratio_y = source_height as f64 / height as f64;
        let scale_y = if ratio_y > 1.0 { ratio_y } else { 1.0 };
        let radius_y = if filter == Filter::Linear { 1.0 } else { 3.0 };
        let span_bound = (2.0 * radius_y * scale_y + 2.0) as usize;
        let slots = (ROW_CACHE_BYTES / row_bytes)
            .max(1)
            .min(source_height)
            .min(span_bound.max(1));
        let mut cache = Buffer::new(slots * row_bytes)?;
        let mut keys = Buffer::new(slots * size_of::<usize>())?;
        typed_mut::<usize>(&mut keys).fill(usize::MAX);
        let mut accum = Buffer::new(row_bytes)?;

        for y in 0..height {
            let vertical = make_span(source_height, height, y, filter);
            let sums = typed_mut::<f32>(&mut accum);
            sums.fill(0.0);
            for n in 0..vertical.count {
                let sy = vertical.first + n;
                let slot = sy % slots;
                let key_slice = typed_mut::<usize>(&mut keys);
                let row =
                    &mut typed_mut::<f32>(&mut cache)[slot * row_values..(slot + 1) * row_values];
                if key_slice[slot] != sy {
                    match &source {
                        FilterSource::Rgba(src) => horizontal(src, source_width, sy, &plan, row),
                        FilterSource::OpaqueRgbRows {
                            bytes,
                            stride,
                            expand,
                        } => {
                            let start = sy * (stride + 1) + 1;
                            let rgb = &bytes[start..start + stride];
                            let rgba = rgb_row.as_mut().ok_or("missing RGB row buffer")?;
                            expand(rgb, rgba.as_mut_slice());
                            horizontal(rgba.as_slice(), source_width, 0, &plan, row);
                        }
                    }
                    key_slice[slot] = sy;
                }
                let weight = span_weight(&vertical, n, filter);
                for i in 0..row_values {
                    sums[i] += row[i] * weight;
                }
            }
            let row_pixels = match &mut output {
                FilterOutput::Rgba(dst) => {
                    let start = (y * width + stripe_x) * CHANNELS;
                    &mut dst[start..start + row_values]
                }
                FilterOutput::Bmp { .. } => &mut output_row[..row_values],
            };
            rgba_from_sums(sums, row_pixels);
            if let FilterOutput::Bmp {
                bytes,
                stride,
                pack,
            } = &mut output
            {
                let start = 54 + (height - 1 - y) * *stride + stripe_x * 3;
                pack(
                    &output_row[..row_values],
                    &mut bytes[start..start + stripe_w * 3],
                );
            }
        }
        stripe_x += stripe_w;
    }
    Ok(())
}
pub(crate) fn filtered(
    image: Image,
    width: usize,
    height: usize,
    filter: Filter,
) -> Result<Image, Error> {
    check_filtered_work(image.width, image.height, width, height)?;
    let mut pixels = Buffer::new(width * height * CHANNELS)?;
    filtered_into(
        FilterSource::Rgba(image.pixels.as_slice()),
        image.width,
        image.height,
        width,
        height,
        filter,
        FilterOutput::Rgba(pixels.as_mut_slice()),
    )?;
    Ok(Image {
        width,
        height,
        pixels,
    })
}
