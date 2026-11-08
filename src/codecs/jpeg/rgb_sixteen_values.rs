use super::*;
// Exact v8 color arithmetic with a packed RGB destination. The uncommon
// rounding exceptions still use the existing scalar conversion.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
#[inline]
pub(crate) unsafe fn rgb_sixteen_values(
    y: core::arch::aarch64::uint8x16_t,
    cb: core::arch::aarch64::uint8x16_t,
    cr: core::arch::aarch64::uint8x16_t,
) -> core::arch::aarch64::uint8x16x3_t {
    use core::arch::aarch64::*;
    let green_tie_1 = vandq_u8(vceqq_u8(cb, vdupq_n_u8(78)), vceqq_u8(cr, vdupq_n_u8(178)));
    let green_tie_2 = vandq_u8(vceqq_u8(cb, vdupq_n_u8(178)), vceqq_u8(cr, vdupq_n_u8(78)));
    let exceptional = vorrq_u8(
        vorrq_u8(green_tie_1, green_tie_2),
        vceqq_u8(cb, vdupq_n_u8(3)),
    );
    if vmaxvq_u8(exceptional) != 0 {
        let mut pixels = [0u8; 64];
        vst4q_u8(
            pixels.as_mut_ptr(),
            uint8x16x4_t(y, cb, cr, vdupq_n_u8(255)),
        );
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let (y, cb, cr) = (pixel[0], pixel[1], pixel[2]);
            store_rgb(pixel, 0, y, cb, cr);
        }
        let converted = vld4q_u8(pixels.as_ptr());
        return uint8x16x3_t(converted.0, converted.1, converted.2);
    }
    let ylo = vmovl_u8(vget_low_u8(y));
    let yhi = vmovl_u8(vget_high_u8(y));
    let cblo = vmovl_u8(vget_low_u8(cb));
    let cbhi = vmovl_u8(vget_high_u8(cb));
    let crlo = vmovl_u8(vget_low_u8(cr));
    let crhi = vmovl_u8(vget_high_u8(cr));
    let (r0, g0, b0) = rgb_four(vget_low_u16(ylo), vget_low_u16(cblo), vget_low_u16(crlo));
    let (r1, g1, b1) = rgb_four(vget_high_u16(ylo), vget_high_u16(cblo), vget_high_u16(crlo));
    let (r2, g2, b2) = rgb_four(vget_low_u16(yhi), vget_low_u16(cbhi), vget_low_u16(crhi));
    let (r3, g3, b3) = rgb_four(vget_high_u16(yhi), vget_high_u16(cbhi), vget_high_u16(crhi));
    let red = vcombine_u8(
        vqmovn_u16(vcombine_u16(r0, r1)),
        vqmovn_u16(vcombine_u16(r2, r3)),
    );
    let green = vcombine_u8(
        vqmovn_u16(vcombine_u16(g0, g1)),
        vqmovn_u16(vcombine_u16(g2, g3)),
    );
    let blue = vcombine_u8(
        vqmovn_u16(vcombine_u16(b0, b1)),
        vqmovn_u16(vcombine_u16(b2, b3)),
    );
    uint8x16x3_t(red, green, blue)
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
#[inline]
pub(crate) unsafe fn rgb_sixteen_channels(
    pixels: &mut [u8],
    y: core::arch::aarch64::uint8x16_t,
    cb: core::arch::aarch64::uint8x16_t,
    cr: core::arch::aarch64::uint8x16_t,
) {
    use core::arch::aarch64::*;
    debug_assert_eq!(pixels.len(), 64);
    let channels = rgb_sixteen_values(y, cb, cr);
    vst4q_u8(
        pixels.as_mut_ptr(),
        uint8x16x4_t(channels.0, channels.1, channels.2, vdupq_n_u8(255)),
    );
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rgb_sixteen(pixels: &mut [u8]) {
    use core::arch::aarch64::vld4q_u8;
    debug_assert_eq!(pixels.len(), 64);
    // SAFETY: the caller supplies exactly 16 complete RGBA pixels.
    let lanes = vld4q_u8(pixels.as_ptr());
    rgb_sixteen_channels(pixels, lanes.0, lanes.1, lanes.2);
}
pub(crate) fn convert_ycbcr_in_place(pixels: &mut [u8]) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    let vectorized = pixels.len() / 64 * 64;
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    let vectorized = 0;
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    for group in pixels[..vectorized].as_chunks_mut::<64>().0 {
        // SAFETY: AArch64 has NEON, and each group is exactly 16 pixels.
        unsafe { rgb_sixteen(group) };
    }
    for pixel in pixels[vectorized..].as_chunks_mut::<4>().0 {
        let (y, cb, cr) = (pixel[0], pixel[1], pixel[2]);
        store_rgb(pixel, 0, y, cb, cr);
    }
}
// Encode a reduced, full-resolution YCbCr grid directly into BMP. This keeps
// the exact decoder's color arithmetic while avoiding a write/read pass of the
// converted RGBA grid. The scan and its EOI marker have already been checked.
pub(crate) fn full_ycbcr_to_bmp(image: &Image) -> Result<Buffer, Error> {
    let (mut bmp, stride) = crate::image::bmp_output(image.width, image.height)?;
    let source = image.pixels.as_slice();
    let output = bmp.as_mut_slice();
    for y in 0..image.height {
        let input_row = &source[y * image.width * 4..(y + 1) * image.width * 4];
        let start = 54 + (image.height - 1 - y) * stride;
        let output_row = &mut output[start..start + image.width * 3];
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        let vectorized = image.width / 16 * 16;
        #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
        let vectorized = 0;
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        for x in (0..vectorized).step_by(16) {
            use core::arch::aarch64::{uint8x16x3_t, vld4q_u8, vst3q_u8};
            // SAFETY: these disjoint rows contain 16 input and output pixels.
            unsafe {
                let lanes = vld4q_u8(input_row.as_ptr().add(x * 4));
                let rgb = rgb_sixteen_values(lanes.0, lanes.1, lanes.2);
                vst3q_u8(
                    output_row.as_mut_ptr().add(x * 3),
                    uint8x16x3_t(rgb.2, rgb.1, rgb.0),
                );
            }
        }
        for x in vectorized..image.width {
            let pixel = &input_row[x * 4..x * 4 + 4];
            let mut rgb = [0u8; 4];
            store_rgb(&mut rgb, 0, pixel[0], pixel[1], pixel[2]);
            output_row[x * 3..x * 3 + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
    }
    Ok(bmp)
}
// Produce eight even/odd chroma pairs. The 16-byte source windows begin at
// the preceding sample, so the three shifted vectors have valid neighbors.
// All arithmetic remains integral and retains the original final rounding.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
#[inline]
pub(crate) unsafe fn half_chroma_sixteen(
    top: &[u8],
    bottom: &[u8],
    weight: u32,
) -> core::arch::aarch64::uint8x16_t {
    use core::arch::aarch64::*;
    debug_assert!(top.len() >= 16 && bottom.len() >= 16 && weight <= 4);
    // SAFETY: both slices have at least 16 bytes, and vext reads only the
    // loaded vectors. All weighted sums are at most 4088, below u16::MAX.
    let a = vld1q_u8(top.as_ptr());
    let b = vld1q_u8(bottom.as_ptr());
    let aw = vdup_n_u8((4 - weight) as u8);
    let bw = vdup_n_u8(weight as u8);
    let vertical = |left, right| vmlal_u8(vmull_u8(left, aw), right, bw);
    let previous = vertical(vget_low_u8(a), vget_low_u8(b));
    let current = vertical(
        vget_low_u8(vextq_u8::<1>(a, a)),
        vget_low_u8(vextq_u8::<1>(b, b)),
    );
    let next = vertical(
        vget_low_u8(vextq_u8::<2>(a, a)),
        vget_low_u8(vextq_u8::<2>(b, b)),
    );
    let triple = vaddq_u16(current, vshlq_n_u16::<1>(current));
    let rounding = vdupq_n_u16(8);
    let even = vshrn_n_u16::<4>(vaddq_u16(vaddq_u16(previous, triple), rounding));
    let odd = vshrn_n_u16::<4>(vaddq_u16(vaddq_u16(triple, next), rounding));
    let interleaved = vzip_u8(even, odd);
    vcombine_u8(interleaved.0, interleaved.1)
}
// Fuse the common 2:1 horizontal interpolation and RGB conversion without
// a separate whole-image chroma write/read. Edge pixels use the exact scalar
// equations and rounding already used by the original path.
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn convert_half_chroma(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    plane: &[u8],
    offsets: [usize; 2],
    stride: usize,
    plane_height: usize,
    vertical_half: bool,
) {
    use core::arch::aarch64::vld4q_u8;
    for y in 0..height {
        let (y0, y1, wy) = if vertical_half {
            half_axis(y, plane_height)
        } else {
            (y, y, 0)
        };
        let cb0 = &plane[offsets[0] + y0 * stride..offsets[0] + (y0 + 1) * stride];
        let cb1 = &plane[offsets[0] + y1 * stride..offsets[0] + (y1 + 1) * stride];
        let cr0 = &plane[offsets[1] + y0 * stride..offsets[1] + (y0 + 1) * stride];
        let cr1 = &plane[offsets[1] + y1 * stride..offsets[1] + (y1 + 1) * stride];
        let column =
            |a: &[u8], b: &[u8], index: usize| a[index] as u32 * (4 - wy) + b[index] as u32 * wy;
        let mut x = 0usize;
        while x < width {
            let sample = x / 2;
            if sample > 0 && sample + 15 <= stride && x + 16 <= width {
                let window = sample - 1..sample + 15;
                let cb = half_chroma_sixteen(&cb0[window.clone()], &cb1[window.clone()], wy);
                let cr = half_chroma_sixteen(&cr0[window.clone()], &cr1[window], wy);
                let start = (y * width + x) * 4;
                let out = &mut pixels[start..start + 64];
                // SAFETY: `out` contains 16 complete pixels and the chroma
                // windows are within separate plane rows, as checked above.
                let luminance = vld4q_u8(out.as_ptr()).0;
                rgb_sixteen_channels(out, luminance, cb, cr);
                x += 16;
                continue;
            }
            let previous = sample.saturating_sub(1);
            let next = (sample + 1).min(stride - 1);
            let cb_cur = column(cb0, cb1, sample);
            let cr_cur = column(cr0, cr1, sample);
            let cb = ((column(cb0, cb1, previous) + 3 * cb_cur + 8) >> 4) as u8;
            let cr = ((column(cr0, cr1, previous) + 3 * cr_cur + 8) >> 4) as u8;
            let offset = (y * width + x) * 4;
            let luminance = pixels[offset];
            store_rgb(pixels, offset, luminance, cb, cr);
            if x + 1 < width {
                let cb = ((3 * cb_cur + column(cb0, cb1, next) + 8) >> 4) as u8;
                let cr = ((3 * cr_cur + column(cr0, cr1, next) + 8) >> 4) as u8;
                let luminance = pixels[offset + 4];
                store_rgb(pixels, offset + 4, luminance, cb, cr);
            }
            x += 2;
        }
    }
}
