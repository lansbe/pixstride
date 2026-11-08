//! Classic TIFF, first IFD, 8-bit interleaved grayscale/RGB(A), strip layout.
//! Tiled, planar, high-bit-depth and palette images are rejected. Orientation
//! is validated but applied by the outer pipeline. Compression: uncompressed,
//! PackBits, TIFF LZW, zlib Deflate.

use super::{lzw, make_image, section};
use crate::{Buffer, Error, Image, MAX_FILE};

#[derive(Clone, Copy)]
pub(crate) enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, bytes: &[u8], offset: usize) -> Result<u16, Error> {
        let b = section(bytes, offset, 2)?;
        Ok(match self {
            Self::Little => u16::from_le_bytes([b[0], b[1]]),
            Self::Big => u16::from_be_bytes([b[0], b[1]]),
        })
    }
    fn u32(self, bytes: &[u8], offset: usize) -> Result<u32, Error> {
        let b = section(bytes, offset, 4)?;
        Ok(match self {
            Self::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Self::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Field {
    kind: u16,
    count: usize,
    data_offset: usize,
}

impl Field {
    fn read(data: &[u8], endian: Endian, entry: usize) -> Result<Self, Error> {
        let kind = endian.u16(data, entry + 2)?;
        let count = endian.u32(data, entry + 4)? as usize;
        let element_size = match kind {
            1 => 1usize,
            3 => 2,
            4 => 4,
            _ => return Err("unsupported TIFF field type"),
        };
        let size = count
            .checked_mul(element_size)
            .ok_or("TIFF field size overflow")?;
        if size > MAX_FILE {
            return Err("TIFF field too large");
        }
        let data_offset = if size <= 4 {
            entry + 8
        } else {
            endian.u32(data, entry + 8)? as usize
        };
        section(data, data_offset, size)?;
        Ok(Self {
            kind,
            count,
            data_offset,
        })
    }
    fn value(self, data: &[u8], endian: Endian, index: usize) -> Result<usize, Error> {
        if index >= self.count {
            return Err("TIFF field count mismatch");
        }
        match self.kind {
            1 => Ok(section(data, self.data_offset + index, 1)?[0] as usize),
            3 => Ok(endian.u16(data, self.data_offset + index * 2)? as usize),
            4 => Ok(endian.u32(data, self.data_offset + index * 4)? as usize),
            _ => Err("unsupported TIFF field type"),
        }
    }
    fn scalar(self, data: &[u8], endian: Endian) -> Result<usize, Error> {
        if self.count != 1 {
            return Err("TIFF scalar field count mismatch");
        }
        self.value(data, endian, 0)
    }
}

fn packbits(data: &[u8], output: &mut [u8]) -> Result<(), Error> {
    let (mut src, mut dst) = (0usize, 0usize);
    while dst < output.len() {
        let control = *data.get(src).ok_or("truncated TIFF PackBits")? as i8;
        src += 1;
        if control >= 0 {
            let n = control as usize + 1;
            let end = dst.checked_add(n).ok_or("TIFF PackBits size overflow")?;
            output
                .get_mut(dst..end)
                .ok_or("TIFF PackBits output overflow")?
                .copy_from_slice(section(data, src, n)?);
            src += n;
            dst = end;
        } else if control != -128 {
            let n = (1i16 - control as i16) as usize;
            let value = *data.get(src).ok_or("truncated TIFF PackBits run")?;
            src += 1;
            let end = dst.checked_add(n).ok_or("TIFF PackBits size overflow")?;
            output
                .get_mut(dst..end)
                .ok_or("TIFF PackBits output overflow")?
                .fill(value);
            dst = end;
        }
    }
    Ok(())
}

// First-image decode still checks the IFD chain for out-of-bounds or cycles.
// Later images are not decoded, and the bounded traversal avoids a metadata
// cycle or unbounded work on hostile input.
fn verify_ifd_chain(data: &[u8], endian: Endian, mut offset: usize) -> Result<(), Error> {
    let mut seen = [0usize; 1024];
    let mut count = 0usize;
    while offset != 0 {
        if offset < 8 || count == seen.len() {
            return Err("invalid TIFF IFD chain");
        }
        if seen[..count].contains(&offset) {
            return Err("cyclic TIFF IFD chain");
        }
        seen[count] = offset;
        count += 1;
        let entries = endian.u16(data, offset)? as usize;
        if entries > 4096 {
            return Err("too many TIFF directory entries");
        }
        let next_pos = offset
            .checked_add(2)
            .and_then(|v| v.checked_add(entries.checked_mul(12)?))
            .ok_or("TIFF directory overflow")?;
        offset = endian.u32(data, next_pos)? as usize;
    }
    Ok(())
}

pub(super) fn decode(data: &[u8]) -> Result<Image, Error> {
    let endian = if data.starts_with(b"II\x2a\0") {
        Endian::Little
    } else if data.starts_with(b"MM\0\x2a") {
        Endian::Big
    } else {
        return Err("invalid TIFF header");
    };
    let directory = endian.u32(data, 4)? as usize;
    if directory < 8 {
        return Err("invalid TIFF directory offset");
    }
    verify_ifd_chain(data, endian, directory)?;
    let entries = endian.u16(data, directory)? as usize;
    if entries > 4096 {
        return Err("too many TIFF directory entries");
    }
    let entries_len = entries.checked_mul(12).ok_or("TIFF directory overflow")?;
    let table_start = directory.checked_add(2).ok_or("TIFF directory overflow")?;
    section(
        data,
        table_start,
        entries_len
            .checked_add(4)
            .ok_or("TIFF directory overflow")?,
    )?;
    // Only read fields relevant to supported formats; unrelated metadata may
    // use other TIFF types and is ignored, not interpreted.
    let tags = [
        256u16, 257, 258, 259, 262, 273, 274, 277, 278, 279, 284, 317, 338, 339,
    ];
    let mut fields: [Option<Field>; 14] = [None; 14];
    for index in 0..entries {
        let at = table_start + index * 12;
        let tag = endian.u16(data, at)?;
        if let Some(slot) = tags.iter().position(|&value| value == tag) {
            if fields[slot].is_some() {
                return Err("duplicate TIFF image field");
            }
            fields[slot] = Some(Field::read(data, endian, at)?);
        }
    }
    let required = |i: usize| fields[i].ok_or("missing TIFF image field");
    let optional = |i: usize, default: usize| -> Result<usize, Error> {
        match fields[i] {
            Some(field) => field.scalar(data, endian),
            None => Ok(default),
        }
    };
    let width = required(0)?.scalar(data, endian)?;
    let height = required(1)?.scalar(data, endian)?;
    if crate::image::dimensions(width, height)? > MAX_FILE {
        return Err("decoded image exceeds limit");
    }
    let bits = required(2)?;
    let compression = optional(3, 1)?;
    let photo = required(4)?.scalar(data, endian)?;
    let offsets = required(5)?;
    // Keep native pixel order; the outer pipeline reads the tag and applies the
    // requested orientation exactly once. Reject malformed values even when
    // the caller elects not to autorotate.
    if !(1..=8).contains(&optional(6, 1)?) {
        return Err("invalid TIFF orientation");
    }
    let samples = optional(7, 1)?;
    let row_count = optional(8, height)?;
    let counts = required(9)?;
    if optional(10, 1)? != 1 {
        return Err("unsupported TIFF planar layout");
    }
    let predictor = optional(11, 1)?;
    if predictor != 1 && predictor != 2 {
        return Err("unsupported TIFF predictor");
    }
    if predictor == 2 && !matches!(compression, 5 | 8 | 32946) {
        return Err("TIFF horizontal predictor requires LZW or Deflate");
    }
    let base = match photo {
        0 | 1 => 1,
        2 => 3,
        _ => return Err("unsupported TIFF color model"),
    };
    if samples != base && samples != base + 1 {
        return Err("unsupported TIFF samples per pixel");
    }
    if bits.count != samples {
        return Err("TIFF bits per sample count mismatch");
    }
    for i in 0..samples {
        if bits.value(data, endian, i)? != 8 {
            return Err("only 8-bit TIFF samples supported");
        }
    }
    if let Some(format) = fields[13] {
        if format.count != samples {
            return Err("TIFF sample format count mismatch");
        }
        for i in 0..samples {
            if format.value(data, endian, i)? != 1 {
                return Err("unsupported TIFF sample format");
            }
        }
    }
    let extra = if samples == base + 1 {
        let value = required(12)?.scalar(data, endian)?;
        if value != 1 && value != 2 {
            return Err("unsupported TIFF extra sample");
        }
        value
    } else {
        if fields[12].is_some() {
            return Err("unexpected TIFF extra samples");
        }
        0
    };
    if photo == 0 && extra == 1 {
        return Err("TIFF WhiteIsZero associated alpha unsupported");
    }
    if row_count == 0 {
        return Err("invalid TIFF rows per strip");
    }
    let strip_rows = row_count.min(height);
    let strip_count = height
        .checked_add(strip_rows - 1)
        .ok_or("TIFF strip count overflow")?
        / strip_rows;
    if offsets.count != strip_count || counts.count != strip_count {
        return Err("TIFF strip count mismatch");
    }
    let row_bytes = width.checked_mul(samples).ok_or("TIFF row size overflow")?;
    if !matches!(compression, 1 | 5 | 8 | 32946 | 32773) {
        return Err("unsupported TIFF compression");
    }
    for strip in 0..strip_count {
        let offset = offsets.value(data, endian, strip)?;
        let length = counts.value(data, endian, strip)?;
        section(data, offset, length)?;
    }
    let mut image = make_image(width, height)?;
    for strip in 0..strip_count {
        let offset = offsets.value(data, endian, strip)?;
        let length = counts.value(data, endian, strip)?;
        let compressed = section(data, offset, length)?;
        let y_start = strip * strip_rows;
        let rows = strip_rows.min(height - y_start);
        let raw_len = row_bytes
            .checked_mul(rows)
            .ok_or("TIFF strip size overflow")?;
        if raw_len > MAX_FILE {
            return Err("TIFF strip too large");
        }
        let mut raw = Buffer::new(raw_len)?;
        match compression {
            1 => raw
                .as_mut_slice()
                .copy_from_slice(section(compressed, 0, raw_len)?),
            32773 => packbits(compressed, raw.as_mut_slice())?,
            5 => lzw::decode(compressed, raw.as_mut_slice(), 8, lzw::Order::Most)?,
            8 | 32946 => crate::png::inflate(compressed, raw.as_mut_slice())?,
            _ => return Err("unsupported TIFF compression"),
        }
        for row in 0..rows {
            let source = &mut raw.as_mut_slice()[row * row_bytes..(row + 1) * row_bytes];
            if predictor == 2 {
                for x in samples..source.len() {
                    source[x] = source[x].wrapping_add(source[x - samples]);
                }
            }
            for (x, pixel) in source.chunks_exact(samples).enumerate() {
                let alpha = if samples == base + 1 {
                    pixel[base]
                } else {
                    255
                };
                let mut rgb = if base == 1 {
                    let gray = if photo == 0 { 255 - pixel[0] } else { pixel[0] };
                    [gray, gray, gray]
                } else {
                    [pixel[0], pixel[1], pixel[2]]
                };
                if extra == 1 {
                    for channel in &mut rgb {
                        *channel = if alpha == 0 {
                            0
                        } else {
                            ((*channel as u32 * 255) / alpha as u32).min(255) as u8
                        };
                    }
                }
                let dst = ((y_start + row) * width + x) * 4;
                image.pixels.as_mut_slice()[dst..dst + 4]
                    .copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
        }
    }
    Ok(image)
}
