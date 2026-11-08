use super::*;
impl<T: BmpTarget> StreamRgbBmp<T> {
    /// Return None when the exact row-cache budget is too large. The caller
    /// then keeps the existing full-raster path rather than changing pixels.
    pub(crate) fn new(
        source_width: usize,
        source_height: usize,
        width: usize,
        height: usize,
        method: &[u8],
        bmp: T,
        bmp_stride: usize,
        expand: fn(&[u8], &mut [u8]),
        pack: fn(&[u8], &mut [u8]),
    ) -> Result<Option<Self>, Error> {
        let filter = match method {
            b"nearest" => None,
            b"bilinear" => Some(Filter::Linear),
            b"lanczos3" => Some(Filter::Lanczos3),
            _ => return Ok(None),
        };
        let filtered = if source_width == width && source_height == height {
            None
        } else if let Some(filter) = filter {
            match StreamFiltered::new(source_width, source_height, width, height, filter)? {
                Some(filtered) => Some(filtered),
                None => return Ok(None),
            }
        } else {
            None
        };
        Ok(Some(Self {
            source_width,
            source_height,
            width,
            height,
            bmp_stride,
            bmp,
            error: None,
            next_source: 0,
            next_output: 0,
            filtered,
            expand,
            pack,
        }))
    }

    pub(crate) fn push(&mut self, y: usize, rgb: &[u8]) {
        if self.error.is_some() {
            return;
        }
        debug_assert_eq!(y, self.next_source);
        debug_assert_eq!(rgb.len(), self.source_width * 3);
        self.next_source += 1;
        if let Some(filtered) = &mut self.filtered {
            self.error = filtered
                .push_row(
                    y,
                    rgb,
                    self.source_width,
                    self.width,
                    self.height,
                    &mut self.next_output,
                    &mut self.bmp,
                    self.bmp_stride,
                    self.expand,
                    self.pack,
                )
                .err();
            return;
        }
        while self.next_output < self.height {
            let sy = (((2 * self.next_output + 1) * self.source_height) / (2 * self.height))
                .min(self.source_height - 1);
            if sy != y {
                break;
            }
            let target = self.bmp.stripe(
                self.bmp_stride,
                self.height,
                self.next_output,
                0,
                self.width * 3,
            );
            if self.source_width == self.width {
                rgb_to_bgr(rgb, target);
            } else {
                for x in 0..self.width {
                    let sx = (((2 * x + 1) * self.source_width) / (2 * self.width))
                        .min(self.source_width - 1);
                    let i = sx * 3;
                    target[x * 3..x * 3 + 3].copy_from_slice(&[rgb[i + 2], rgb[i + 1], rgb[i]]);
                }
            }
            if let Err(error) = self
                .bmp
                .end_row(self.bmp_stride, self.height, self.next_output)
            {
                self.error = Some(error);
                return;
            }
            self.next_output += 1;
        }
    }

    pub(crate) fn finish(self) -> Result<T, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.next_source != self.source_height || self.next_output != self.height {
            return Err("incomplete streamed image");
        }
        Ok(self.bmp)
    }
}
/// Resize reconstructed PNG RGB rows into an already allocated final BMP,
/// without allocating a complete RGBA source or destination raster. PNG
/// CRC/Adler/filter validation has already finished.
pub(crate) fn opaque_rgb_to_bmp(
    rows: &[u8],
    source_width: usize,
    source_height: usize,
    source_stride: usize,
    width: usize,
    height: usize,
    method: &[u8],
    out: &mut [u8],
    bmp_stride: usize,
    expand: fn(&[u8], &mut [u8]),
    pack: fn(&[u8], &mut [u8]),
) -> Result<(), Error> {
    if source_width == 0
        || source_height == 0
        || width == 0
        || height == 0
        || source_width > 100_000
        || source_height > 100_000
        || width > 100_000
        || height > 100_000
        || source_width
            .checked_mul(source_height)
            .filter(|&n| n <= MAX_PIXELS)
            .is_none()
        || width
            .checked_mul(height)
            .filter(|&n| n <= MAX_PIXELS)
            .is_none()
    {
        return Err("image dimensions outside limits");
    }
    if source_stride != source_width * 3
        || (source_stride + 1).checked_mul(source_height) != Some(rows.len())
        || bmp_stride != ((width * 3 + 3) & !3)
        || 54usize.checked_add(bmp_stride * height) != Some(out.len())
    {
        return Err("invalid PNG RGB or BMP buffer");
    }
    let filter = match method {
        b"nearest" => None,
        b"bilinear" => Some(Filter::Linear),
        b"lanczos3" => Some(Filter::Lanczos3),
        _ => return Err("unsupported resize method"),
    };
    if source_width == width && source_height == height {
        for y in 0..height {
            let start = y * (source_stride + 1) + 1;
            let dest = 54 + (height - 1 - y) * bmp_stride;
            rgb_to_bgr(
                &rows[start..start + source_stride],
                &mut out[dest..dest + width * 3],
            );
        }
    } else if let Some(filter) = filter {
        filtered_into(
            FilterSource::OpaqueRgbRows {
                bytes: rows,
                stride: source_stride,
                expand,
            },
            source_width,
            source_height,
            width,
            height,
            filter,
            FilterOutput::Bmp {
                bytes: out,
                stride: bmp_stride,
                pack,
            },
        )?;
    } else {
        for y in 0..height {
            let sy = (((2 * y + 1) * source_height) / (2 * height)).min(source_height - 1);
            let src = sy * (source_stride + 1) + 1;
            let dst = 54 + (height - 1 - y) * bmp_stride;
            for x in 0..width {
                let sx = (((2 * x + 1) * source_width) / (2 * width)).min(source_width - 1);
                let i = src + sx * 3;
                let o = dst + x * 3;
                out[o..o + 3].copy_from_slice(&[rows[i + 2], rows[i + 1], rows[i]]);
            }
        }
    }
    Ok(())
}
pub(crate) fn resize(
    image: Image,
    width: usize,
    height: usize,
    method: &[u8],
) -> Result<Image, Error> {
    if image.width == 0 || image.height == 0 || width == 0 || height == 0 {
        return Err("zero image dimension");
    }
    let source_pixels = image
        .width
        .checked_mul(image.height)
        .ok_or("image dimensions overflow")?;
    let output_pixels = width
        .checked_mul(height)
        .ok_or("image dimensions overflow")?;
    if source_pixels > MAX_PIXELS || output_pixels > MAX_PIXELS {
        return Err("image exceeds pixel limit");
    }
    let source_bytes = source_pixels
        .checked_mul(CHANNELS)
        .ok_or("image size overflow")?;
    if image.pixels.len() != source_bytes {
        return Err("invalid RGBA buffer length");
    }
    let filter = match method {
        b"nearest" => None,
        b"bilinear" => Some(Filter::Linear),
        b"lanczos3" => Some(Filter::Lanczos3),
        _ => return Err("unsupported resize method"),
    };
    if width == image.width && height == image.height {
        return Ok(image);
    }
    match filter {
        Some(f) => filtered(image, width, height, f),
        None => nearest(image, width, height),
    }
}
