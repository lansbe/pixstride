//! In-tree TIFF and ICO writers for the  compatibility facade.
//! TIFF uses a single zlib/DEFLATE RGBA8 strip with unassociated alpha.
//! ICO embeds exactly one PNG and accepts dimensions from 1 to 256 pixels.

use crate::{deflate_encode, png_encode, Buffer, Error, Image, MAX_FILE, MAX_PIXELS};

fn checked_image(image: &Image) -> Result<&[u8], Error> {
    let pixels = image
        .width
        .checked_mul(image.height)
        .ok_or("image dimensions overflow")?;
    if image.width == 0
        || image.height == 0
        || image.width > 100_000
        || image.height > 100_000
        || pixels > MAX_PIXELS
    {
        return Err("image dimensions outside limits");
    }
    let len = pixels.checked_mul(4).ok_or("image size overflow")?;
    if len > MAX_FILE || image.pixels.len() != len {
        return Err("inconsistent RGBA image");
    }
    Ok(image.pixels.as_slice())
}

fn le16(dst: &mut [u8], at: usize, value: u16) {
    dst[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn le32(dst: &mut [u8], at: usize, value: u32) {
    dst[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn tiff_entry(dst: &mut [u8], index: usize, tag: u16, kind: u16, count: u32, value: u32) {
    let at = 10 + index * 12;
    le16(dst, at, tag);
    le16(dst, at + 2, kind);
    le32(dst, at + 4, count);
    le32(dst, at + 8, value);
}

/// Encode a single-image little-endian classic TIFF. The strip stores the
/// original unassociated RGBA8 pixels without a predictor or color profile.
pub fn encode_tiff(image: &Image) -> Result<Buffer, Error> {
    let pixels = checked_image(image)?;
    let compressed = deflate_encode::compress(pixels)?;
    // TIFF header + IFD with 11 entries + next-IFD pointer + 4 SHORT bit depths.
    const N: usize = 11;
    const BITS_OFFSET: usize = 8 + 2 + N * 12 + 4;
    const STRIP_OFFSET: usize = BITS_OFFSET + 8;
    let total = STRIP_OFFSET
        .checked_add(compressed.len())
        .ok_or("TIFF size overflow")?;
    if total > MAX_FILE || compressed.len() > u32::MAX as usize {
        return Err("TIFF output too large");
    }
    let mut output = Buffer::new(total)?;
    let dst = output.as_mut_slice();
    dst[..4].copy_from_slice(b"II\x2a\0");
    le32(dst, 4, 8);
    le16(dst, 8, N as u16);
    tiff_entry(dst, 0, 256, 4, 1, image.width as u32); // ImageWidth
    tiff_entry(dst, 1, 257, 4, 1, image.height as u32); // ImageLength
    tiff_entry(dst, 2, 258, 3, 4, BITS_OFFSET as u32); // BitsPerSample
    tiff_entry(dst, 3, 259, 3, 1, 8); // Adobe Deflate, zlib wrapper
    tiff_entry(dst, 4, 262, 3, 1, 2); // RGB
    tiff_entry(dst, 5, 273, 4, 1, STRIP_OFFSET as u32); // StripOffsets
    tiff_entry(dst, 6, 277, 3, 1, 4); // SamplesPerPixel
    tiff_entry(dst, 7, 278, 4, 1, image.height as u32); // RowsPerStrip
    tiff_entry(dst, 8, 279, 4, 1, compressed.len() as u32); // StripByteCounts
    tiff_entry(dst, 9, 284, 3, 1, 1); // Contiguous planar configuration
    tiff_entry(dst, 10, 338, 3, 1, 2); // Unassociated alpha
    le32(dst, 10 + N * 12, 0); // No second image.
    for i in 0..4 {
        le16(dst, BITS_OFFSET + i * 2, 8);
    }
    dst[STRIP_OFFSET..].copy_from_slice(&compressed);
    Ok(output)
}

/// Encode one PNG-backed ICO entry, including the 256-pixel zero convention.
pub fn encode_ico(image: &Image) -> Result<Buffer, Error> {
    let pixels = checked_image(image)?;
    if image.width > 256 || image.height > 256 {
        return Err("ICO dimensions must be 1..256");
    }
    let opaque = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255);
    let png = png_encode::encode(image)?;
    let total = png.len().checked_add(22).ok_or("ICO size overflow")?;
    if total > MAX_FILE || png.len() > u32::MAX as usize {
        return Err("ICO output too large");
    }
    let mut output = Buffer::new(total)?;
    let dst = output.as_mut_slice();
    le16(dst, 0, 0);
    le16(dst, 2, 1);
    le16(dst, 4, 1);
    dst[6] = if image.width == 256 {
        0
    } else {
        image.width as u8
    };
    dst[7] = if image.height == 256 {
        0
    } else {
        image.height as u8
    };
    dst[8] = 0; // Truecolor rather than an ICO palette.
    dst[9] = 0;
    le16(dst, 10, 1);
    le16(dst, 12, if opaque { 24 } else { 32 });
    le32(dst, 14, png.len() as u32);
    le32(dst, 18, 22);
    dst[22..].copy_from_slice(png.as_slice());
    Ok(output)
}
