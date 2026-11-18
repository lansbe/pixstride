//! Lossless opaque palette permutations inside the isolated worker.
use super::engine::NativeResult;
use crate::{
    ops::Rotation,
    request::{OutputFormat, Request},
    Error,
};

const OFFSET: usize = 1078;

fn eligible(r: &Request, mark: &[u8]) -> bool {
    let (o, e) = (&r.options, &r.extra);
    r.format == Some(OutputFormat::Bmp)
        && r.strip_metadata
        && r.strip_color_profile
        && !e.keep_copyright
        && !e.preserve_hdr
        && !e.raw
        && e.skip.is_empty()
        && e.force_formats.is_empty()
        && e.auto_formats.is_empty()
        && e.base_opacity == 1.0
        && !e.encoding.linear
        && !e.encoding.png_quantize
        && !e.encoding.png_interlaced
        && o.width == 0
        && o.height == 0
        && !o.enlarge
        && o.crop.is_none()
        && e.crop_size.is_none()
        && o.padding == Default::default()
        && o.background.is_none()
        && o.watermark.is_none()
        && r.watermark.is_none()
        && mark.is_empty()
        && e.dpr == 1.0
        && e.zoom == [1.0; 2]
        && e.min == [0; 2]
        && !e.extend
        && !e.extend_aspect
        && !e.fill_down
        && !e.smart
        && !e.crop_smart
        && !e.enforce_thumbnail
        && e.max_bytes == 0
        && r.effects == Default::default()
        && (o.rotation != Rotation::R0 || o.flip_horizontal || o.flip_vertical)
}

// Keep this shortcut to a complete image stream. All CRC/Adler/frame checks
// still run in the shared decoder, including finish() and late chunk guards.
fn image_chunks_only(data: &[u8]) -> Result<bool, Error> {
    let mut at = 8usize;
    while let Some(header) = data.get(at..at.saturating_add(8)) {
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .filter(|&n| n <= data.len())
            .ok_or("invalid PNG chunk framing")?;
        match &header[4..8] {
            b"IHDR" | b"PLTE" | b"IDAT" => (),
            b"IEND" => return Ok(end == data.len()),
            _ => return Ok(false),
        }
        at = end;
    }
    Err("missing PNG end chunk")
}

fn input_index(x: usize, y: usize, width: usize, height: usize, r: &Request) -> usize {
    let (ow, oh) = oriented(width, height, r.options.rotation);
    let x = if r.options.flip_horizontal {
        ow - 1 - x
    } else {
        x
    };
    let y = if r.options.flip_vertical {
        oh - 1 - y
    } else {
        y
    };
    let (x, y) = match r.options.rotation {
        Rotation::R0 => (x, y),
        Rotation::R90 => (y, height - 1 - x),
        Rotation::R180 => (width - 1 - x, height - 1 - y),
        Rotation::R270 => (width - 1 - y, x),
    };
    y * width + x
}

fn oriented(width: usize, height: usize, rotation: Rotation) -> (usize, usize) {
    match rotation {
        Rotation::R90 | Rotation::R270 => (height, width),
        _ => (width, height),
    }
}

pub(super) fn try_process(
    data: &[u8],
    r: &Request,
    mark: &[u8],
) -> Result<Option<NativeResult>, Error> {
    if !eligible(r, mark)
        || data.len() < 33
        || &data[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
        || data[24..29] != [8, 3, 0, 0, 0]
    {
        return Ok(None);
    }
    if data.len() > r.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("PNG source exceeds byte limit");
    }
    let width = u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize;
    let height = u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize;
    crate::image::dimensions(width, height)?;
    let count = width.checked_mul(height).ok_or("PNG dimensions overflow")?;
    if count > r.extra.max_source_pixels || count > r.extra.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    let (ow, oh) = oriented(width, height, r.options.rotation);
    if ow.max(oh) > r.extra.max_result_dimension {
        return Ok(None);
    }
    let stride = ow.checked_add(3).ok_or("BMP stride overflow")? & !3;
    let size = stride
        .checked_mul(oh)
        .and_then(|n| n.checked_add(OFFSET))
        .ok_or("BMP size overflow")?;
    let rgb_size = ow
        .checked_mul(3)
        .and_then(|n| n.checked_add(3))
        .map(|n| n & !3)
        .and_then(|n| n.checked_mul(oh))
        .and_then(|n| n.checked_add(54))
        .ok_or("BMP size overflow")?;
    // Avoid growing tiny files or introducing output our RGB reader cannot admit.
    if size > rgb_size || rgb_size > crate::MAX_FILE || !image_chunks_only(data)? {
        return Ok(None);
    }
    let Some(decoded) = super::png_decode::indices(data, r)? else {
        return Ok(None);
    };
    if decoded.dimensions != [width as i32, height as i32] || decoded.pixels.len() != count {
        return Err("inconsistent PNG indices");
    }
    let mut body = Vec::new();
    body.try_reserve_exact(size)
        .map_err(|_| "BMP allocation failed")?;
    body.resize(size, 0);
    body[..2].copy_from_slice(b"BM");
    for (at, value) in [
        (2, size),
        (10, OFFSET),
        (14, 40),
        (18, ow),
        (22, oh),
        (34, size - OFFSET),
        (46, 256),
    ] {
        body[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    body[26] = 1;
    body[28] = 8;
    for (entry, rgb) in body[54..OFFSET]
        .chunks_exact_mut(4)
        .zip(decoded.colors.chunks_exact(3))
    {
        entry.copy_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
    }
    for (row, output) in body[OFFSET..].chunks_exact_mut(stride).enumerate() {
        let y = oh - 1 - row;
        if r.options.rotation == Rotation::R0 && !r.options.flip_horizontal {
            let sy = if r.options.flip_vertical {
                height - 1 - y
            } else {
                y
            };
            output[..ow].copy_from_slice(&decoded.pixels[sy * width..(sy + 1) * width]);
        } else {
            for (x, out) in output[..ow].iter_mut().enumerate() {
                *out = decoded.pixels[input_index(x, y, width, height, r)];
            }
        }
    }
    Ok(Some(NativeResult {
        body,
        dimensions: [ow, oh],
        origin: [width, height],
        format: OutputFormat::Bmp,
    }))
}

#[cfg(test)]
#[path = "indexed_bmp_tests.rs"]
mod tests;
