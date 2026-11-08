//! Windows ICO directory: deterministic largest entry, PNG or uncompressed
//! 1/4/8/24/32-bit DIB with AND transparency. Other DIB modes are rejected.

use super::{le16, le32, make_image, section};
use crate::{Error, Image};

pub(super) fn decode(data: &[u8]) -> Result<Image, Error> {
    section(data, 0, 6)?;
    if le16(data, 0)? != 0 || le16(data, 2)? != 1 {
        return Err("invalid ICO header");
    }
    let count = le16(data, 4)? as usize;
    if count == 0 || count > 256 {
        return Err("invalid ICO image count");
    }
    let directory_end = 6usize
        .checked_add(count.checked_mul(16).ok_or("ICO directory overflow")?)
        .ok_or("ICO directory overflow")?;
    section(data, 0, directory_end)?;
    let mut selected: Option<(usize, usize, usize, bool, &[u8])> = None;
    for index in 0..count {
        let at = 6 + index * 16;
        let width = if data[at] == 0 {
            256
        } else {
            data[at] as usize
        };
        let height = if data[at + 1] == 0 {
            256
        } else {
            data[at + 1] as usize
        };
        let bpp = le16(data, at + 6)? as usize;
        let size = le32(data, at + 8)? as usize;
        let offset = le32(data, at + 12)? as usize;
        if size == 0 || offset < directory_end {
            return Err("invalid ICO image offset");
        }
        let bytes = section(data, offset, size)?;
        let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
        let rank = (width * height, bpp, png);
        if selected
            .as_ref()
            .is_none_or(|v| rank > (v.0 * v.1, v.2, v.3))
        {
            selected = Some((width, height, bpp, png, bytes));
        }
    }
    let (width, height, _, png, bytes) = selected.ok_or("ICO has no image")?;
    let image = if png {
        section(bytes, 0, 29)?;
        if bytes[24] == 8 && bytes[28] == 0 {
            crate::png::decode(bytes)?
        } else {
            crate::png_extended::decode(bytes)?
        }
    } else {
        dib(bytes, width, height)?
    };
    if image.width != width || image.height != height {
        return Err("ICO directory dimensions mismatch");
    }
    Ok(image)
}

fn dib(data: &[u8], expected_width: usize, expected_height: usize) -> Result<Image, Error> {
    let header_len = le32(data, 0)? as usize;
    if header_len < 40 {
        return Err("unsupported ICO DIB header");
    }
    section(data, 0, header_len)?;
    let w = le32(data, 4)? as i32;
    let double_h = le32(data, 8)? as i32;
    if w <= 0 || double_h <= 0 || double_h % 2 != 0 {
        return Err("invalid ICO DIB dimensions");
    }
    let width = w as usize;
    let height = (double_h / 2) as usize;
    if width != expected_width || height != expected_height {
        return Err("ICO directory dimensions mismatch");
    }
    let planes = le16(data, 12)?;
    let bpp = le16(data, 14)? as usize;
    if planes != 1 || !matches!(bpp, 1 | 4 | 8 | 24 | 32) || le32(data, 16)? != 0 {
        return Err("unsupported ICO DIB pixel format");
    }
    let palette_count = if bpp <= 8 {
        let declared = le32(data, 32)? as usize;
        let maximum = 1usize << bpp;
        if declared == 0 {
            maximum
        } else if declared <= maximum {
            declared
        } else {
            return Err("invalid ICO palette length");
        }
    } else {
        0
    };
    let palette_len = palette_count.checked_mul(4).ok_or("ICO palette overflow")?;
    let palette = section(data, header_len, palette_len)?;
    let pixel_offset = header_len
        .checked_add(palette_len)
        .ok_or("ICO pixel offset overflow")?;
    let pixel_stride = width
        .checked_mul(bpp)
        .and_then(|v| v.checked_add(31))
        .map(|v| v / 32 * 4)
        .ok_or("ICO row size overflow")?;
    let xor_len = pixel_stride
        .checked_mul(height)
        .ok_or("ICO bitmap size overflow")?;
    let xor = section(data, pixel_offset, xor_len)?;
    let mask_offset = pixel_offset
        .checked_add(xor_len)
        .ok_or("ICO mask offset overflow")?;
    let mask_stride = width
        .checked_add(31)
        .map(|v| v / 32 * 4)
        .ok_or("ICO mask row overflow")?;
    let mask_len = mask_stride
        .checked_mul(height)
        .ok_or("ICO mask size overflow")?;
    let mask = if data.len().saturating_sub(mask_offset) >= mask_len {
        Some(section(data, mask_offset, mask_len)?)
    } else if bpp == 32 && data.len() == mask_offset {
        None
    } else {
        return Err("truncated ICO mask");
    };
    let mut image = make_image(width, height)?;
    let has_alpha = bpp == 32
        && (mask.is_none()
            || (0..height).any(|y| {
                let row = &xor[y * pixel_stride..y * pixel_stride + width * 4];
                row.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0)
            }));
    for y in 0..height {
        let source_y = height - 1 - y;
        let row = &xor[source_y * pixel_stride..(source_y + 1) * pixel_stride];
        for x in 0..width {
            let (r, g, b, alpha) = match bpp {
                32 => {
                    let p = &row[x * 4..x * 4 + 4];
                    (p[2], p[1], p[0], if has_alpha { p[3] } else { 255 })
                }
                24 => {
                    let p = &row[x * 3..x * 3 + 3];
                    (p[2], p[1], p[0], 255)
                }
                _ => {
                    let bit = x * bpp;
                    let shift = 8 - bpp - bit % 8;
                    let index = ((row[bit / 8] as usize) >> shift) & ((1usize << bpp) - 1);
                    let p = section(palette, index * 4, 4)
                        .map_err(|_| "ICO palette index out of bounds")?;
                    (p[2], p[1], p[0], 255)
                }
            };
            let transparent = !has_alpha
                && mask
                    .is_some_and(|m| (m[source_y * mask_stride + x / 8] & (0x80 >> (x % 8))) != 0);
            let dst = (y * width + x) * 4;
            image.pixels.as_mut_slice()[dst..dst + 4].copy_from_slice(&[
                r,
                g,
                b,
                if transparent { 0 } else { alpha },
            ]);
        }
    }
    Ok(image)
}
