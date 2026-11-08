//! GIF87a/GIF89a first image only. Later animation frames are not decoded.

use super::{le16, lzw, make_image, section};
use crate::{Buffer, Error, Image};

fn subblocks(data: &[u8], cursor: &mut usize) -> Result<(usize, usize), Error> {
    let start = *cursor;
    let mut total = 0usize;
    loop {
        let size = *data.get(*cursor).ok_or("truncated GIF sub-block")? as usize;
        *cursor += 1;
        if size == 0 {
            return Ok((start, total));
        }
        section(data, *cursor, size)?;
        *cursor = cursor.checked_add(size).ok_or("GIF sub-block overflow")?;
        total = total.checked_add(size).ok_or("GIF sub-block overflow")?;
    }
}

fn collect_subblocks(data: &[u8], start: usize, output: &mut [u8]) -> Result<(), Error> {
    let mut at = start;
    let mut written = 0usize;
    loop {
        let n = *data.get(at).ok_or("truncated GIF sub-block")? as usize;
        at += 1;
        if n == 0 {
            return if written == output.len() {
                Ok(())
            } else {
                Err("GIF block length mismatch")
            };
        }
        let end = written.checked_add(n).ok_or("GIF block length overflow")?;
        output
            .get_mut(written..end)
            .ok_or("GIF block length mismatch")?
            .copy_from_slice(section(data, at, n)?);
        at += n;
        written = end;
    }
}

// Verify the remaining GIF container to its trailer, while intentionally not
// decompressing later animation frames. A first-frame API must still reject a
// truncated extension, image descriptor, data block or missing trailer.
fn verify_tail(data: &[u8], mut cursor: usize) -> Result<(), Error> {
    loop {
        let kind = *data.get(cursor).ok_or("GIF missing trailer")?;
        cursor += 1;
        match kind {
            0x3b => {
                return if cursor == data.len() {
                    Ok(())
                } else {
                    Err("GIF trailing data")
                }
            }
            0x21 => {
                let label = *data.get(cursor).ok_or("truncated GIF extension")?;
                cursor += 1;
                if label == 0xf9 {
                    let control = section(data, cursor, 6)?;
                    if control[0] != 4 || control[5] != 0 {
                        return Err("invalid GIF graphic control");
                    }
                    cursor += 6;
                } else {
                    let _ = subblocks(data, &mut cursor)?;
                }
            }
            0x2c => {
                let header = section(data, cursor, 9)?;
                let image_flags = header[8];
                cursor += 9;
                if image_flags & 0x80 != 0 {
                    let length = 3usize << ((image_flags & 7) as usize + 1);
                    section(data, cursor, length)?;
                    cursor += length;
                }
                let minimum = *data.get(cursor).ok_or("truncated GIF LZW header")?;
                if !(2..=8).contains(&minimum) {
                    return Err("invalid GIF LZW code size");
                }
                cursor += 1;
                let _ = subblocks(data, &mut cursor)?;
            }
            _ => return Err("invalid GIF block"),
        }
    }
}

pub(super) fn decode(data: &[u8]) -> Result<Image, Error> {
    section(data, 0, 13)?;
    let width = le16(data, 6)? as usize;
    let height = le16(data, 8)? as usize;
    if crate::image::dimensions(width, height)? > crate::MAX_FILE {
        return Err("decoded image exceeds limit");
    }
    let flags = data[10];
    let background = data[11] as usize;
    let mut cursor = 13usize;
    let global = if flags & 0x80 != 0 {
        let length = 3usize << ((flags & 7) as usize + 1);
        let palette = section(data, cursor, length)?;
        cursor += length;
        Some(palette)
    } else {
        None
    };
    if let Some(palette) = global {
        if background >= palette.len() / 3 {
            return Err("GIF background palette index out of bounds");
        }
    }
    let mut transparent: Option<u8> = None;
    loop {
        let kind = *data.get(cursor).ok_or("GIF has no image")?;
        cursor += 1;
        match kind {
            0x21 => {
                let label = *data.get(cursor).ok_or("truncated GIF extension")?;
                cursor += 1;
                if label == 0xf9 {
                    let control = section(data, cursor, 6)?;
                    if control[0] != 4 || control[5] != 0 {
                        return Err("invalid GIF graphic control");
                    }
                    transparent = if control[1] & 1 != 0 {
                        Some(control[4])
                    } else {
                        None
                    };
                    cursor += 6;
                } else {
                    let _ = subblocks(data, &mut cursor)?;
                    if label == 0x01 {
                        transparent = None;
                    }
                }
            }
            0x2c => {
                let header = section(data, cursor, 9)?;
                let left = le16(header, 0)? as usize;
                let top = le16(header, 2)? as usize;
                let frame_w = le16(header, 4)? as usize;
                let frame_h = le16(header, 6)? as usize;
                let image_flags = header[8];
                cursor += 9;
                if frame_w == 0
                    || frame_h == 0
                    || left.checked_add(frame_w).is_none_or(|v| v > width)
                    || top.checked_add(frame_h).is_none_or(|v| v > height)
                {
                    return Err("GIF image outside logical screen");
                }
                let palette = if image_flags & 0x80 != 0 {
                    let length = 3usize << ((image_flags & 7) as usize + 1);
                    let value = section(data, cursor, length)?;
                    cursor += length;
                    value
                } else {
                    global.ok_or("GIF image has no color table")?
                };
                let minimum = *data.get(cursor).ok_or("truncated GIF LZW header")?;
                cursor += 1;
                let (block_start, compressed_len) = subblocks(data, &mut cursor)?;
                let mut compressed = Buffer::new(compressed_len)?;
                collect_subblocks(data, block_start, compressed.as_mut_slice())?;
                let count = frame_w
                    .checked_mul(frame_h)
                    .ok_or("GIF image size overflow")?;
                let mut indices = Buffer::new(count)?;
                lzw::decode(
                    compressed.as_slice(),
                    indices.as_mut_slice(),
                    minimum,
                    lzw::Order::Least,
                )?;
                verify_tail(data, cursor)?;
                let mut image = make_image(width, height)?;

                // For the first frame a declared transparent index gives the
                // previously undrawn logical screen a transparent background.
                // Without transparency use the global background, or palette
                // entry zero when no global table was supplied.
                let (table, index, alpha) = match transparent {
                    Some(index) => (palette, index as usize, 0),
                    None => match global {
                        Some(table) => (table, background, 255),
                        None => (palette, 0, 255),
                    },
                };
                let p = section(table, index * 3, 3)
                    .map_err(|_| "GIF transparent palette index out of bounds")?;
                for pixel in image.pixels.as_mut_slice().as_chunks_mut::<4>().0 {
                    pixel.copy_from_slice(&[p[0], p[1], p[2], alpha]);
                }
                let mut row_index = 0usize;
                let passes: &[(usize, usize)] = if image_flags & 0x40 != 0 {
                    &[(0, 8), (4, 8), (2, 4), (1, 2)]
                } else {
                    &[(0, 1)]
                };
                for &(start, step) in passes {
                    let mut y = start;
                    while y < frame_h {
                        let row =
                            &indices.as_slice()[row_index * frame_w..(row_index + 1) * frame_w];
                        for (x, &index) in row.iter().enumerate() {
                            let i = index as usize * 3;
                            let color = section(palette, i, 3)
                                .map_err(|_| "GIF palette index out of bounds")?;
                            let at = ((top + y) * width + left + x) * 4;
                            let alpha = if transparent == Some(index) { 0 } else { 255 };
                            image.pixels.as_mut_slice()[at..at + 4]
                                .copy_from_slice(&[color[0], color[1], color[2], alpha]);
                        }
                        row_index += 1;
                        y += step;
                    }
                }
                if row_index != frame_h {
                    return Err("invalid GIF interlace rows");
                }
                return Ok(image);
            }
            0x3b => return Err("GIF has no image"),
            _ => return Err("invalid GIF block"),
        }
    }
}
