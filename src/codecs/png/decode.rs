use super::*;
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    let parsed = parse_for_transform(data)?;
    if parsed.opaque_rgb_dimensions().is_some() {
        // The final RGBA allocation also holds the filtered RGB stream:
        // h*(3*w+1) <= h*4*w for every nonempty image. Validate the complete
        // compressed stream before reconstructing or exposing any pixel.
        let mut pixels = Buffer::new(parsed.pixel_size)?;
        let compressed = parsed.compressed()?;
        inflate(
            compressed.as_slice(),
            &mut pixels.as_mut_slice()[..parsed.raw_size],
        )?;
        drop(compressed);
        reconstruct(
            &mut pixels.as_mut_slice()[..parsed.raw_size],
            parsed.height,
            parsed.stride,
            3,
        )?;
        let bytes = pixels.as_mut_slice();
        for y in (0..parsed.height).rev() {
            #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
            let mut remaining = parsed.width;
            #[cfg(target_arch = "aarch64")]
            unsafe {
                use core::arch::aarch64::{uint8x16x4_t, vdupq_n_u8, vld3q_u8, vst4q_u8};
                while remaining >= 16 {
                    remaining -= 16;
                    let src = y * (parsed.stride + 1) + 1 + remaining * 3;
                    let dst = (y * parsed.width + remaining) * 4;
                    // Read all 48 RGB bytes before the overlapping 64-byte
                    // write. Both complete groups are within their row and
                    // the final allocation; earlier pixels remain untouched.
                    let rgb = vld3q_u8(bytes.as_ptr().add(src));
                    let rgba = uint8x16x4_t(rgb.0, rgb.1, rgb.2, vdupq_n_u8(255));
                    vst4q_u8(bytes.as_mut_ptr().add(dst), rgba);
                }
            }
            for x in (0..remaining).rev() {
                let src = y * (parsed.stride + 1) + 1 + x * 3;
                let dst = (y * parsed.width + x) * 4;
                // Read the entire pixel first. Reverse expansion never
                // overwrites an earlier unread RGB pixel, including w=1.
                let rgba = [bytes[src], bytes[src + 1], bytes[src + 2], 255];
                bytes[dst..dst + 4].copy_from_slice(&rgba);
            }
        }
        return Ok(Image {
            width: parsed.width,
            height: parsed.height,
            pixels,
        });
    }
    parsed.into_decoded()?.into_image()
}
impl DecodedPng {
    /// RGB pixels have one filter byte before each already reconstructed row.
    /// All other formats, including tRNS, use the general RGBA conversion.
    pub(crate) fn opaque_rgb_rows(&self) -> Option<(&[u8], usize, usize, usize)> {
        if self.color == 2 && self.transparent_rgb.is_none() {
            Some((self.raw.as_slice(), self.width, self.height, self.stride))
        } else {
            None
        }
    }
}
pub(crate) fn write_chunk(dst: &mut [u8], pos: usize, kind: &[u8; 4], content: &[u8]) -> usize {
    dst[pos..pos + 4].copy_from_slice(&(content.len() as u32).to_be_bytes());
    dst[pos + 4..pos + 8].copy_from_slice(kind);
    dst[pos + 8..pos + 8 + content.len()].copy_from_slice(content);
    let crc = crc32(&dst[pos + 4..pos + 8 + content.len()]);
    dst[pos + 8 + content.len()..pos + 12 + content.len()].copy_from_slice(&crc.to_be_bytes());
    pos + content.len() + 12
}
pub(crate) fn encode(image: &Image) -> Result<Buffer, Error> {
    let width = image.width;
    let height = image.height;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || mul_checked(width, height)? > MAX_PIXELS
    {
        return Err("PNG: dimensions out of bounds");
    }
    let input_row_bytes = mul_checked(width, 4)?;
    let pixel_bytes = mul_checked(input_row_bytes, height)?;
    if image.pixels.as_slice().len() != pixel_bytes {
        return Err("PNG: inconsistent pixels");
    }
    let opaque = image
        .pixels
        .as_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255);
    let output_channels = if opaque { 3 } else { 4 };
    let row_bytes = mul_checked(width, output_channels)?;
    let raw_len = mul_checked(add_checked(row_bytes, 1)?, height)?;
    let blocks = add_checked(raw_len, 65534)? / 65535;
    let zlib_len = add_checked(add_checked(raw_len, mul_checked(blocks, 5)?)?, 6)?;
    let total = add_checked(zlib_len, 57)?;
    if total > MAX_FILE || zlib_len > u32::MAX as usize {
        return Err("PNG: output too large");
    }
    let mut result = Buffer::new(total)?;
    let dst = result.as_mut_slice();
    dst[..8].copy_from_slice(SIGNATURE);
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&(width as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = if opaque { 2 } else { 6 };
    let pos = write_chunk(dst, 8, b"IHDR", &ihdr);
    dst[pos..pos + 4].copy_from_slice(&(zlib_len as u32).to_be_bytes());
    dst[pos + 4..pos + 8].copy_from_slice(b"IDAT");
    let mut p = pos + 8;
    dst[p..p + 2].copy_from_slice(&[0x78, 0x01]);
    p += 2;
    let src = image.pixels.as_slice();
    let mut remaining = raw_len;
    let mut row = 0usize;
    let mut in_row = 0usize;
    let mut adler_a = 1u32;
    let mut adler_b = 0u32;
    while remaining != 0 {
        let n = core::cmp::min(remaining, 65535);
        dst[p] = if n == remaining { 1 } else { 0 };
        dst[p + 1..p + 3].copy_from_slice(&(n as u16).to_le_bytes());
        dst[p + 3..p + 5].copy_from_slice(&(!(n as u16)).to_le_bytes());
        p += 5;
        let mut written = 0usize;
        while written < n {
            if in_row == 0 {
                dst[p] = 0; // PNG filter: None
                p += 1;
                written += 1;
                in_row = 1;
                adler_b += adler_a;
                if adler_b >= 65521 {
                    adler_b -= 65521;
                }
                if written == n {
                    break;
                }
            }
            let from_row = in_row - 1;
            let take = core::cmp::min(row_bytes - from_row, n - written);
            if opaque {
                // RGB output drops the known-opaque alpha without allocating a
                // second image. A stored block may split any individual pixel.
                let mut j = 0usize;
                while j < take {
                    let component = from_row + j;
                    let available = core::cmp::min(3 - component % 3, take - j);
                    let src_at = row * input_row_bytes + (component / 3) * 4 + component % 3;
                    dst[p + j..p + j + available].copy_from_slice(&src[src_at..src_at + available]);
                    j += available;
                }
            } else {
                dst[p..p + take].copy_from_slice(
                    &src[row * input_row_bytes + from_row..row * input_row_bytes + from_row + take],
                );
            }
            for chunk in dst[p..p + take].chunks(5552) {
                for &byte in chunk {
                    adler_a += byte as u32;
                    adler_b += adler_a;
                }
                adler_a %= 65521;
                adler_b %= 65521;
            }
            p += take;
            written += take;
            in_row += take;
            if in_row == row_bytes + 1 {
                in_row = 0;
                row += 1;
            }
        }
        remaining -= n;
    }
    dst[p..p + 4].copy_from_slice(&((adler_b << 16) | adler_a).to_be_bytes());
    p += 4;
    let crc = crc32(&dst[pos + 4..p]);
    dst[p..p + 4].copy_from_slice(&crc.to_be_bytes());
    p += 4;
    let end = write_chunk(dst, p, b"IEND", &[]);
    if end != total {
        return Err("PNG: inconsistent encoded size");
    }
    Ok(result)
}
