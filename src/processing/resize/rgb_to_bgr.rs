use super::*;
/// Swap a validated opaque RGB row directly into BMP's BGR channel order.
pub(crate) fn rgb_to_bgr(source: &[u8], dest: &mut [u8]) {
    debug_assert_eq!(source.len(), dest.len());
    debug_assert_eq!(source.len() % 3, 0);
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut at = 0usize;
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    unsafe {
        use core::arch::aarch64::{uint8x16x3_t, vld3q_u8, vst3q_u8};
        while at + 48 <= source.len() {
            // SAFETY: the loop covers 16 complete source and destination
            // pixels, and their owning buffers are distinct.
            let rgb = vld3q_u8(source.as_ptr().add(at));
            vst3q_u8(dest.as_mut_ptr().add(at), uint8x16x3_t(rgb.2, rgb.1, rgb.0));
            at += 48;
        }
    }
    for (rgb, bgr) in source[at..]
        .as_chunks::<3>()
        .0
        .iter()
        .zip(dest[at..].chunks_exact_mut(3))
    {
        bgr.copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
    }
}
impl BmpTarget for Buffer {
    #[inline]
    fn stripe(
        &mut self,
        stride: usize,
        height: usize,
        y: usize,
        first: usize,
        bytes: usize,
    ) -> &mut [u8] {
        let start = 54 + (height - 1 - y) * stride + first * 3;
        &mut self.as_mut_slice()[start..start + bytes]
    }
    #[inline]
    fn end_row(&mut self, _: usize, _: usize, _: usize) -> Result<(), Error> {
        Ok(())
    }
}
impl StreamRgbaTarget {
    pub(crate) fn new(width: usize, height: usize) -> Result<Self, Error> {
        crate::image::dimensions(width, height)?;
        let size = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(CHANNELS))
            .ok_or("image size overflow")?;
        Ok(Self {
            width,
            height,
            pixels: Buffer::new(size)?,
            unexpected_bgr: false,
        })
    }

    pub(crate) fn into_image(self) -> Image {
        Image {
            width: self.width,
            height: self.height,
            pixels: self.pixels,
        }
    }
}
impl BmpTarget for StreamRgbaTarget {
    fn stripe(&mut self, _: usize, _: usize, y: usize, first: usize, bytes: usize) -> &mut [u8] {
        // The caller may only publish a finished stream. If a BGR-only path
        // ever reaches this target, end_row rejects the private output.
        self.unexpected_bgr = true;
        let start = (y * self.width + first) * CHANNELS;
        &mut self.pixels.as_mut_slice()[start..start + bytes]
    }

    fn end_row(&mut self, _: usize, _: usize, _: usize) -> Result<(), Error> {
        if self.unexpected_bgr {
            Err("RGBA stream requires filtered rows")
        } else {
            Ok(())
        }
    }

    #[inline]
    fn write_rgba(
        &mut self,
        _: usize,
        _: usize,
        y: usize,
        first: usize,
        pixels: &[u8],
        _: fn(&[u8], &mut [u8]),
    ) {
        let start = (y * self.width + first) * CHANNELS;
        self.pixels.as_mut_slice()[start..start + pixels.len()].copy_from_slice(pixels);
    }
}
impl StreamOpaqueAlpha {
    pub(crate) fn from_plans(plans: &[Option<Plan>], width: usize) -> Option<Self> {
        let mut map = Buffer::new(width).ok()?;
        let mut values = [0.0f32; MAX_ALPHA_PATTERNS];
        let mut len = 0usize;
        for (stripe, plan) in plans.iter().enumerate() {
            for (local, span) in typed::<Span>(&plan.as_ref()?.spans).iter().enumerate() {
                let index = values[..len]
                    .iter()
                    .position(|a| a.to_bits() == span.opaque_alpha.to_bits())
                    .unwrap_or(len);
                if index == len {
                    if len == MAX_ALPHA_PATTERNS {
                        return None;
                    }
                    values[len] = span.opaque_alpha;
                    len += 1;
                }
                map.as_mut_slice()[stripe * STRIPE + local] = index as u8;
            }
        }
        Some(Self { map, values, len })
    }

    /// Match the original vertical loop's exact order of f32 products/adds.
    pub(crate) fn sums(&self, plan: &Plan, span: &Span) -> [f32; MAX_ALPHA_PATTERNS] {
        let mut out = [0.0f32; MAX_ALPHA_PATTERNS];
        for (index, result) in out[..self.len].iter_mut().enumerate() {
            for n in 0..span.count {
                *result += self.values[index] * plan.weight(span, n);
            }
        }
        out
    }
}
