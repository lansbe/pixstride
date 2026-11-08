use super::*;
pub(crate) fn decode_scan(
    data: &[u8],
    pos: usize,
    source_width: usize,
    source_height: usize,
    count: usize,
    max_h: usize,
    max_v: usize,
    components: &mut [Component; 3],
    order: &[usize; 3],
    quant: &[[u16; 64]; 4],
    quant_present: &[bool; 4],
    dc: &[Huffman; 4],
    ac: &[Huffman; 4],
    restart_interval: usize,
    shrink: usize,
    defer_full_ycbcr: bool,
) -> Result<Image, Error> {
    for c in &components[..count] {
        if !quant_present[c.quant] || !dc[c.dc].present || !ac[c.ac].present {
            return Err("JPEG: missing required table");
        }
    }
    let width = source_width.div_ceil(shrink);
    let height = source_height.div_ceil(shrink);
    let mut pixels = Buffer::new(width * height * 4)?;
    let target = pixels.as_mut_slice();
    let mcu_width = max_h * 8;
    let mcu_height = max_v * 8;
    let columns = source_width.div_ceil(mcu_width);
    let rows = source_height.div_ceil(mcu_height);
    let total = columns * rows;
    let mut bits = Entropy::new(data, pos);
    let mut restart = 0u8;
    // Full-size channels go directly into the output buffer. Only channels
    // needing upsampling use temporary compact planes, avoiding a full-frame
    // luminance copy for common 4:2:0 and 4:2:2 images.
    let mut plane_width = [0usize; 3];
    let mut plane_height = [0usize; 3];
    let mut plane_offset = [0usize; 3];
    let mut component_factor = [1usize; 3];
    let mut effective_h = [0usize; 3];
    let mut effective_v = [0usize; 3];
    let mut plane_bytes = 0usize;
    for i in 0..count {
        let c = components[i];
        // JPEG's IDCT is square: pick one component factor that avoids more
        // samples than the output in either axis, without claiming both axes
        // reach that resolution. For 4:2:0 the chroma may retain twice as many
        // samples; 4:2:2 remains subsampled and is interpolated horizontally.
        component_factor[i] =
            component_shrink(shrink, c.h, max_h).max(component_shrink(shrink, c.v, max_v));
        effective_h[i] = c.h * shrink / component_factor[i];
        effective_v[i] = c.v * shrink / component_factor[i];
        let full_width = (source_width * c.h).div_ceil(max_h);
        let full_height = (source_height * c.v).div_ceil(max_v);
        plane_width[i] = full_width.div_ceil(component_factor[i]);
        plane_height[i] = full_height.div_ceil(component_factor[i]);
        plane_offset[i] = plane_bytes;
        if effective_h[i] != max_h || effective_v[i] != max_v {
            plane_bytes = plane_bytes
                .checked_add(plane_width[i] * plane_height[i])
                .ok_or("JPEG: invalid plane dimensions")?;
        }
    }
    let mut planes = if plane_bytes == 0 {
        None
    } else {
        Some(Buffer::new(plane_bytes)?)
    };
    let mut block_pixels = [0u8; 64];
    for index in 0..total {
        let mx = index % columns;
        let my = index / columns;
        for &ci in &order[..count] {
            let comp = &mut components[ci];
            for by in 0..comp.v {
                for bx in 0..comp.h {
                    block(
                        &mut bits,
                        comp,
                        &quant[comp.quant],
                        &dc[comp.dc],
                        &ac[comp.ac],
                        &mut block_pixels,
                        component_factor[ci],
                    )?;
                    let block_size = 8 / component_factor[ci];
                    let px = (mx * comp.h + bx) * block_size;
                    let py = (my * comp.v + by) * block_size;
                    if px >= plane_width[ci] || py >= plane_height[ci] {
                        continue;
                    }
                    let cols = block_size.min(plane_width[ci] - px);
                    let rows = block_size.min(plane_height[ci] - py);
                    if effective_h[ci] == max_h && effective_v[ci] == max_v {
                        for y in 0..rows {
                            let start = ((py + y) * width + px) * 4;
                            let dst = &mut target[start..start + cols * 4];
                            let src = &block_pixels[y * 8..y * 8 + cols];
                            for (pixel, &sample) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                                pixel[ci] = sample;
                            }
                        }
                    } else {
                        let plane = planes.as_mut().ok_or("JPEG: plan temporaire absent")?;
                        let dst = plane.as_mut_slice();
                        for y in 0..rows {
                            let start = plane_offset[ci] + (py + y) * plane_width[ci] + px;
                            dst[start..start + cols]
                                .copy_from_slice(&block_pixels[y * 8..y * 8 + cols]);
                        }
                    }
                }
            }
        }
        if restart_interval != 0 && (index + 1) % restart_interval == 0 && index + 1 < total {
            bits.marker(0xd0 + restart)?;
            restart = (restart + 1) % 8;
            for c in &mut components[..count] {
                c.predictor = 0;
            }
        }
    }
    bits.marker(0xd9)?;
    let plane_data = match &planes {
        Some(plane) => plane.as_slice(),
        None => &[],
    };
    let common_half_chroma = count == 3
        && effective_h[0] == max_h
        && effective_v[0] == max_v
        && effective_h[1] == effective_h[2]
        && effective_v[1] == effective_v[2]
        && effective_h[1] * 2 == max_h
        && (effective_v[1] == max_v || effective_v[1] * 2 == max_v);
    if shrink == 1 && source_width <= 4 && common_half_chroma && plane_width[1] <= 2 {
        // For 4:2:2 and 4:2:0 at this narrow boundary, the independently
        // verified decoder expands each chroma sample by replication instead
        // of centred interpolation. Keep the existing fast path unchanged for
        // all wider sources and for reduced-geometry decoding.
        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * 4;
                let luminance = target[offset];
                let mut values = [0u8; 2];
                for ci in 1..3 {
                    values[ci - 1] = cmyk::adobe_sample_plane(
                        plane_data,
                        plane_offset[ci],
                        plane_width[ci],
                        plane_height[ci],
                        x,
                        y,
                        components[ci],
                        max_h,
                        max_v,
                        2,
                        ci,
                    );
                }
                store_rgb(target, offset, luminance, values[0], values[1]);
            }
        }
        return Ok(Image {
            width,
            height,
            pixels,
        });
    }
    if common_half_chroma {
        let stride = plane_width[1];
        let vertical_half = effective_v[1] * 2 == max_v;
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        let fused = if width >= 32 && stride >= 16 {
            // SAFETY: the component planes and destination are distinct
            // buffers, and the helper checks each vector window before load.
            unsafe {
                convert_half_chroma(
                    target,
                    width,
                    height,
                    plane_data,
                    [plane_offset[1], plane_offset[2]],
                    stride,
                    plane_height[1],
                    vertical_half,
                )
            };
            true
        } else {
            false
        };
        #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
        let fused = false;
        if !fused {
            for y in 0..height {
                let (y0, y1, wy) = if vertical_half {
                    half_axis(y, plane_height[1])
                } else {
                    (y, y, 0)
                };
                // The horizontally adjacent output pixels reuse the same two
                // vertical sums. Rearranging these small integer products is exact;
                // the final +8 and >>4 remain after both axes, as before.
                let column = |component, x| {
                    vertical_quarters(plane_data, plane_offset[component], stride, x, y0, y1, wy)
                };
                let mut cb_previous = column(1, 0);
                let mut cb_current = cb_previous;
                let mut cb_next = column(1, 1.min(stride - 1));
                let mut cr_previous = column(2, 0);
                let mut cr_current = cr_previous;
                let mut cr_next = column(2, 1.min(stride - 1));
                for x in (0..width).step_by(2) {
                    let cb = ((cb_previous + 3 * cb_current + 8) >> 4) as u8;
                    let cr = ((cr_previous + 3 * cr_current + 8) >> 4) as u8;
                    let offset = (y * width + x) * 4;
                    target[offset + 1] = cb;
                    target[offset + 2] = cr;
                    if x + 1 < width {
                        let cb = ((3 * cb_current + cb_next + 8) >> 4) as u8;
                        let cr = ((3 * cr_current + cr_next + 8) >> 4) as u8;
                        target[offset + 5] = cb;
                        target[offset + 6] = cr;
                    }
                    if x + 2 < width {
                        cb_previous = cb_current;
                        cb_current = cb_next;
                        cr_previous = cr_current;
                        cr_current = cr_next;
                        let next = (x / 2 + 2).min(stride - 1);
                        cb_next = column(1, next);
                        cr_next = column(2, next);
                    }
                }
            }
            convert_ycbcr_in_place(target);
        }
    } else if count == 3 && (0..3).all(|ci| effective_h[ci] == max_h && effective_v[ci] == max_v) {
        // Reduced 4:2:0 and 4:4:4 can already have all samples at output
        // resolution. Avoid the generic per-component sampling checks.
        if !defer_full_ycbcr {
            convert_ycbcr_in_place(target);
        }
    } else if count == 1 && effective_h[0] == max_h && effective_v[0] == max_v {
        for pixel in target.as_chunks_mut::<4>().0 {
            pixel[1] = pixel[0];
            pixel[2] = pixel[0];
            pixel[3] = 255;
        }
    } else {
        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * 4;
                let mut values = [0u8; 3];
                for ci in 0..count {
                    let mut c = components[ci];
                    c.h = effective_h[ci];
                    c.v = effective_v[ci];
                    values[ci] = if c.h == max_h && c.v == max_v {
                        target[offset + ci]
                    } else {
                        sample_plane(
                            plane_data,
                            plane_offset[ci],
                            plane_width[ci],
                            plane_height[ci],
                            x,
                            y,
                            c,
                            max_h,
                            max_v,
                        )
                    };
                }
                if count == 1 {
                    target[offset] = values[0];
                    target[offset + 1] = values[0];
                    target[offset + 2] = values[0];
                    target[offset + 3] = 255;
                } else {
                    store_rgb(target, offset, values[0], values[1], values[2]);
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
/// Decode a supported complete JPEG byte stream into non-premultiplied RGBA8.
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    decode_impl(data, None)
}
/// Experimental box-prefiltered IDCT. The largest factor 2, 4 or 8 that does
/// not make either decoded axis smaller than the requested target is chosen.
/// For reduced decodes, use a floor-sized top-left grid before final resizing.
pub(crate) fn decode_for_target(data: &[u8], width: usize, height: usize) -> Result<Image, Error> {
    let (parsed, shrink) = parse_for_target(data, width, height)?;
    parsed.into_reduced(shrink)
}
// Emit one RGB row using the same quarter-weight chroma interpolation and
// color conversion as the complete v8 decoder. The virtual chroma plane has
// the preceding row at index zero and the current eight-row band at 1..=8.
pub(crate) fn render_420_row(
    luma: &[u8],
    plane: &[u8],
    stride: usize,
    virtual_y: usize,
    rgb: &mut [u8],
) {
    let width = luma.len();
    debug_assert_eq!(rgb.len(), width * 3);
    let offset = stride * 10;
    let (y0, y1, wy) = half_axis(virtual_y, 10);
    let cb0 = &plane[y0 * stride..(y0 + 1) * stride];
    let cb1 = &plane[y1 * stride..(y1 + 1) * stride];
    let cr0 = &plane[offset + y0 * stride..offset + (y0 + 1) * stride];
    let cr1 = &plane[offset + y1 * stride..offset + (y1 + 1) * stride];
    let column = |a: &[u8], b: &[u8], x: usize| a[x] as u32 * (4 - wy) + b[x] as u32 * wy;
    let mut x = 0usize;
    while x < width {
        let sample = x / 2;
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        if sample > 0 && sample + 15 <= stride && x + 16 <= width {
            use core::arch::aarch64::{vld1q_u8, vst3q_u8};
            let window = sample - 1..sample + 15;
            // SAFETY: all four windows and 16 input/output pixels are bounded
            // by the guard, and the distinct buffers cannot alias.
            unsafe {
                let cb = half_chroma_sixteen(&cb0[window.clone()], &cb1[window.clone()], wy);
                let cr = half_chroma_sixteen(&cr0[window.clone()], &cr1[window], wy);
                let y = vld1q_u8(luma.as_ptr().add(x));
                let result = rgb_sixteen_values(y, cb, cr);
                vst3q_u8(rgb.as_mut_ptr().add(x * 3), result);
            }
            x += 16;
            continue;
        }
        let previous = sample.saturating_sub(1);
        let next = (sample + 1).min(stride - 1);
        let cb = column(cb0, cb1, sample);
        let cr = column(cr0, cr1, sample);
        let cb_even = ((column(cb0, cb1, previous) + 3 * cb + 8) >> 4) as u8;
        let cr_even = ((column(cr0, cr1, previous) + 3 * cr + 8) >> 4) as u8;
        let mut pixel = [0u8; 4];
        store_rgb(&mut pixel, 0, luma[x], cb_even, cr_even);
        rgb[x * 3..x * 3 + 3].copy_from_slice(&pixel[..3]);
        if x + 1 < width {
            let cb_odd = ((3 * cb + column(cb0, cb1, next) + 8) >> 4) as u8;
            let cr_odd = ((3 * cr + column(cr0, cr1, next) + 8) >> 4) as u8;
            store_rgb(&mut pixel, 0, luma[x + 1], cb_odd, cr_odd);
            rgb[(x + 1) * 3..(x + 1) * 3 + 3].copy_from_slice(&pixel[..3]);
        }
        x += 2;
    }
}
