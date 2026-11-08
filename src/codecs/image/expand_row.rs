use super::*;
/// Expand a validated RGB/BGR row into the internal RGBA representation.
/// For BI_RGB32 the fourth source byte is unused and must remain ignored.
pub(crate) fn expand_row(source: &[u8], out: &mut [u8], channels: usize, bgr: bool) {
    debug_assert!(channels == 3 || channels == 4);
    debug_assert_eq!(source.len() / channels * 4, out.len());
    #[cfg(target_arch = "aarch64")]
    let count = out.len() / 4;
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut x = 0;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let opaque = vdupq_n_u8(255);
        while x + 16 <= count {
            // Every load/store is wholly inside the row slices. Unaligned
            // addresses are supported by these NEON interleaved operations.
            let (first, green, third) = if channels == 3 {
                let v = vld3q_u8(source.as_ptr().add(x * 3));
                (v.0, v.1, v.2)
            } else {
                let v = vld4q_u8(source.as_ptr().add(x * 4));
                (v.0, v.1, v.2)
            };
            let (red, blue) = if bgr { (third, first) } else { (first, third) };
            vst4q_u8(
                out.as_mut_ptr().add(x * 4),
                uint8x16x4_t(red, green, blue, opaque),
            );
            x += 16;
        }
    }
    for (input, pixel) in source[x * channels..]
        .chunks_exact(channels)
        .zip(out[x * 4..].chunks_exact_mut(4))
    {
        let (r, b) = if bgr {
            (input[2], input[0])
        } else {
            (input[0], input[2])
        };
        pixel.copy_from_slice(&[r, input[1], b, 255]);
    }
}
/// Flatten RGBA onto white and encode one BMP row, preserving the original
/// integer rounding. The fast path is selected from all 16 actual alpha bytes.
pub(crate) fn pack_bgr_row(source: &[u8], out: &mut [u8]) {
    debug_assert_eq!(source.len() / 4 * 3, out.len());
    #[cfg(target_arch = "aarch64")]
    let count = source.len() / 4;
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut x = 0;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        while x + 16 <= count {
            // The loop condition covers the entire 64-byte read and 48-byte
            // write. Pixel values are never read across a row boundary.
            let rgba = vld4q_u8(source.as_ptr().add(x * 4));
            if vminvq_u8(rgba.3) == 255 {
                vst3q_u8(
                    out.as_mut_ptr().add(x * 3),
                    uint8x16x3_t(rgba.2, rgba.1, rgba.0),
                );
            } else {
                for i in x..x + 16 {
                    flatten_pixel(&source[i * 4..i * 4 + 4], &mut out[i * 3..i * 3 + 3]);
                }
            }
            x += 16;
        }
    }
    for (pixel, bgr) in source[x * 4..]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out[x * 3..].chunks_exact_mut(3))
    {
        flatten_pixel(pixel, bgr);
    }
}
#[inline]
pub(crate) fn flatten_pixel(pixel: &[u8], bgr: &mut [u8]) {
    let alpha = pixel[3] as u32;
    for c in 0..3 {
        bgr[c] = ((pixel[2 - c] as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
    }
}
pub(crate) fn dimensions(width: usize, height: usize) -> Result<usize, Error> {
    let pixels = width.checked_mul(height).ok_or("dimension overflow")?;
    if width == 0 || height == 0 || pixels > MAX_PIXELS || width > 100_000 || height > 100_000 {
        return Err("dimensions outside configured limits");
    }
    pixels.checked_mul(4).ok_or("size overflow")
}
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return crate::png::decode(data);
    }
    if data.starts_with(b"\xff\xd8") {
        return crate::jpeg::decode(data);
    }
    if data.starts_with(b"P6") {
        return ppm(data);
    }
    if data.starts_with(b"BM") {
        return bmp(data);
    }
    Err("unsupported image format")
}
pub(crate) fn token(data: &[u8], at: &mut usize) -> Result<usize, Error> {
    loop {
        while *at < data.len() && data[*at].is_ascii_whitespace() {
            *at += 1;
        }
        if data.get(*at) != Some(&b'#') {
            break;
        }
        while *at < data.len() && data[*at] != b'\n' {
            *at += 1;
        }
    }
    let start = *at;
    let mut value = 0usize;
    while *at < data.len() && data[*at].is_ascii_digit() {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add((data[*at] - b'0') as usize))
            .ok_or("PPM number overflow")?;
        *at += 1;
    }
    if start == *at {
        return Err("invalid PPM header");
    }
    Ok(value)
}
pub(crate) fn ppm(data: &[u8]) -> Result<Image, Error> {
    if !data.get(2).is_some_and(u8::is_ascii_whitespace) {
        return Err("invalid PPM magic");
    }
    let mut at = 2;
    let width = token(data, &mut at)?;
    let height = token(data, &mut at)?;
    let max = token(data, &mut at)?;
    let size = dimensions(width, height)?;
    if max != 255 {
        return Err("only PPM P6 maxval 255 supported");
    }
    if !data.get(at).is_some_and(u8::is_ascii_whitespace) {
        return Err("invalid PPM separator");
    }
    at += 1;
    let count = width * height;
    let source = data.get(at..at + count * 3).ok_or("truncated PPM pixels")?;
    if at + count * 3 != data.len() {
        return Err("multiple PPM images/trailing data unsupported");
    }
    let mut pixels = Buffer::new(size)?;
    expand_row(source, pixels.as_mut_slice(), 3, false);
    Ok(Image {
        width,
        height,
        pixels,
    })
}
pub(crate) fn le16(data: &[u8], at: usize) -> Result<u16, Error> {
    let b = data.get(at..at + 2).ok_or("truncated BMP header")?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
pub(crate) fn le32(data: &[u8], at: usize) -> Result<u32, Error> {
    let b = data.get(at..at + 4).ok_or("truncated BMP header")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
impl BmpLayout {
    pub(crate) fn parse(data: &[u8], file_len: usize) -> Result<Self, Error> {
        let offset = le32(data, 10)? as usize;
        let header = le32(data, 14)? as usize;
        if header < 40 || offset < 14 + header || offset > file_len {
            return Err("unsupported BMP header");
        }
        let w = le32(data, 18)? as i32;
        let h = le32(data, 22)? as i32;
        if w <= 0 || h == 0 || h == i32::MIN {
            return Err("invalid BMP dimensions");
        }
        let width = w as usize;
        let height = h.unsigned_abs() as usize;
        dimensions(width, height)?;
        let bpp = le16(data, 28)?;
        if le16(data, 26)? != 1 || le32(data, 30)? != 0 || (bpp != 24 && bpp != 32) {
            return Err("only uncompressed BMP RGB24/RGB32 supported");
        }
        let channels = (bpp / 8) as usize;
        let stride = (width * channels + 3) & !3;
        let end = offset + height * stride;
        if end > file_len {
            return Err("truncated BMP pixels");
        }
        Ok(Self {
            width,
            height,
            top_down: h < 0,
            channels,
            stride,
            offset,
            end,
        })
    }
}
impl<'a> BmpView<'a> {
    pub(crate) fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let layout = BmpLayout::parse(data, data.len())?;
        Ok(Self {
            width: layout.width,
            height: layout.height,
            top_down: layout.top_down,
            channels: layout.channels,
            stride: layout.stride,
            source: &data[layout.offset..layout.end],
        })
    }

    pub(crate) fn decode(&self) -> Result<Image, Error> {
        let mut pixels = Buffer::new(dimensions(self.width, self.height)?)?;
        for y in 0..self.height {
            let sy = if self.top_down {
                y
            } else {
                self.height - 1 - y
            };
            let row = &self.source[sy * self.stride..sy * self.stride + self.width * self.channels];
            let out = &mut pixels.as_mut_slice()[y * self.width * 4..(y + 1) * self.width * 4];
            expand_row(row, out, self.channels, true);
        }
        Ok(Image {
            width: self.width,
            height: self.height,
            pixels,
        })
    }

    /// Normalize a validated, unchanged raster to the exact BMP24 layout used by
    /// encode_bmp. Copy pixels only: source padding/unused alpha are not metadata
    /// that may be forwarded to the newly zero-initialized output.
    pub(crate) fn encode_identity(&self) -> Result<Buffer, Error> {
        let (mut buffer, stride) = bmp_output(self.width, self.height)?;
        let b = buffer.as_mut_slice();
        for row in 0..self.height {
            let input_row = if self.top_down {
                self.height - 1 - row
            } else {
                row
            };
            let source = &self.source
                [input_row * self.stride..input_row * self.stride + self.width * self.channels];
            let out = &mut b[54 + row * stride..54 + row * stride + self.width * 3];
            if self.channels == 3 {
                out.copy_from_slice(source);
            } else {
                bgr32_to_bgr24(source, out);
            }
        }
        Ok(buffer)
    }
}
/// Discard the unused fourth byte of BI_RGB32 without interpreting alpha.
pub(crate) fn bgr32_to_bgr24(source: &[u8], out: &mut [u8]) {
    debug_assert_eq!(source.len() / 4 * 3, out.len());
    #[cfg(target_arch = "aarch64")]
    let count = source.len() / 4;
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut x = 0;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        while x + 16 <= count {
            let v = vld4q_u8(source.as_ptr().add(x * 4));
            vst3q_u8(out.as_mut_ptr().add(x * 3), uint8x16x3_t(v.0, v.1, v.2));
            x += 16;
        }
    }
    for (pixel, bgr) in source[x * 4..]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out[x * 3..].chunks_exact_mut(3))
    {
        bgr.copy_from_slice(&pixel[..3]);
    }
}
pub(crate) fn bmp(data: &[u8]) -> Result<Image, Error> {
    BmpView::parse(data)?.decode()
}
pub(crate) fn bmp_header(width: usize, height: usize, len: usize) -> [u8; 54] {
    let mut b = [0u8; 54];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&(len as u32).to_le_bytes());
    b[10..14].copy_from_slice(&54u32.to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&(width as u32).to_le_bytes());
    b[22..26].copy_from_slice(&(height as u32).to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&24u16.to_le_bytes());
    b[34..38].copy_from_slice(&((len - 54) as u32).to_le_bytes());
    b
}
pub(crate) fn bmp_output(width: usize, height: usize) -> Result<(Buffer, usize), Error> {
    let stride = (width * 3 + 3) & !3;
    let len = 54 + stride * height;
    let mut buffer = Buffer::new(len)?;
    buffer.as_mut_slice()[..54].copy_from_slice(&bmp_header(width, height, len));
    Ok((buffer, stride))
}
pub(crate) fn encode_bmp(image: &Image) -> Result<Buffer, Error> {
    let (mut buffer, stride) = bmp_output(image.width, image.height)?;
    let b = buffer.as_mut_slice();
    for y in 0..image.height {
        let start = 54 + (image.height - 1 - y) * stride;
        let source = &image.pixels.as_slice()[y * image.width * 4..(y + 1) * image.width * 4];
        pack_bgr_row(source, &mut b[start..start + image.width * 3]);
    }
    Ok(buffer)
}
pub(crate) fn full_jpeg_bmp(
    parsed: crate::jpeg::ParsedJpeg<'_>,
    width: usize,
    height: usize,
    method: &[u8],
) -> Result<Buffer, Error> {
    if let Some((source_width, source_height)) = parsed.opaque_rgb_dimensions() {
        if source_width * source_height * 3 >= 256 * 1024
            && (method == b"nearest" || method == b"bilinear" || method == b"lanczos3")
        {
            if let Ok((bmp, bmp_stride)) = bmp_output(width, height) {
                if let Ok(Some(mut stream)) = crate::resize::StreamRgbBmp::new(
                    source_width,
                    source_height,
                    width,
                    height,
                    method,
                    bmp,
                    bmp_stride,
                    crate::png::opaque_rgb_to_rgba,
                    pack_bgr_row,
                ) {
                    parsed.stream_rgb_rows(&mut |y, row| stream.push(y, row))?;
                    return stream.finish();
                }
            }
        }
    }
    let image = parsed.into_image()?;
    let resized = crate::resize::resize(image, width, height, method)?;
    encode_bmp(&resized)
}
