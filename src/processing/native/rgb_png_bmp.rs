//! Decode plain RGB PNG rows directly into lossless BMP row permutations.
use super::engine::NativeResult;
use crate::{
    ops::Rotation,
    request::{OutputFormat, Request},
    Error,
};
use fast_png::{BitDepth, ColorType, DecodeOptions, Decoder, Limits, Transformations};
use std::io::Cursor;

pub(super) fn try_process(
    data: &[u8],
    request: &Request,
    mark: &[u8],
) -> Result<Option<NativeResult>, Error> {
    if data.len() < 33
        || &data[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
        || data[24..29] != [8, 2, 0, 0, 0]
        || !super::indexed_bmp::eligible(request, mark)
        || matches!(request.options.rotation, Rotation::R90 | Rotation::R270)
    {
        return Ok(None);
    }
    if data.len() > request.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("PNG source exceeds byte limit");
    }
    let width = u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize;
    let height = u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize;
    crate::image::dimensions(width, height)?;
    let pixels = width.checked_mul(height).ok_or("PNG dimensions overflow")?;
    if pixels > request.extra.max_source_pixels || pixels > request.extra.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    if width.max(height) > request.extra.max_result_dimension {
        return Ok(None);
    }
    let row_bytes = width.checked_mul(3).ok_or("PNG row overflow")?;
    let source_size = row_bytes.checked_mul(height).ok_or("PNG raster overflow")?;
    let stride = row_bytes.checked_add(3).ok_or("BMP row overflow")? & !3;
    let length = stride
        .checked_mul(height)
        .and_then(|n| n.checked_add(54))
        .ok_or("BMP size overflow")?;
    if source_size > crate::MAX_FILE
        || length > crate::MAX_FILE
        || !super::indexed_bmp::image_chunks_only(data)?
    {
        return Ok(None);
    }
    let mut checks = DecodeOptions::default();
    checks.set_ignore_checksums(false);
    checks.set_skip_ancillary_crc_failures(false);
    let mut decoder = Decoder::new_with_options(Cursor::new(data), checks);
    decoder.set_transformations(Transformations::IDENTITY);
    decoder.set_limits(Limits {
        bytes: crate::MAX_FILE,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_| "PNG header decode failed")?;
    if !super::png_decode::plain(reader.info()) {
        return Ok(None);
    }
    if reader.info().width as usize != width
        || reader.info().height as usize != height
        || reader.output_color_type() != (ColorType::Rgb, BitDepth::Eight)
        || reader.output_buffer_size() != Some(source_size)
    {
        return Err("inconsistent PNG RGB rows");
    }
    let mut body = Vec::new();
    body.try_reserve_exact(length)
        .map_err(|_| "BMP allocation failed")?;
    body.resize(length, 0);
    body[..54].copy_from_slice(&crate::image::bmp_header(width, height, length));
    let half_turn = request.options.rotation == Rotation::R180;
    let horizontal = request.options.flip_horizontal ^ half_turn;
    let vertical = request.options.flip_vertical ^ half_turn;
    for y in 0..height {
        let row = reader
            .next_row()
            .map_err(|_| "PNG row decode failed")?
            .ok_or("missing PNG row")?;
        if row.data().len() != row_bytes {
            return Err("inconsistent PNG row size");
        }
        let output_y = if vertical { y } else { height - 1 - y };
        let at = 54 + output_y * stride;
        let output = &mut body[at..at + row_bytes];
        crate::resize::rgb_to_bgr(row.data(), output);
        if horizontal {
            for x in 0..width / 2 {
                for channel in 0..3 {
                    output.swap(x * 3 + channel, (width - 1 - x) * 3 + channel);
                }
            }
        }
    }
    if reader
        .next_row()
        .map_err(|_| "PNG row finalization failed")?
        .is_some()
    {
        return Err("unexpected PNG row");
    }
    reader
        .finish()
        .map_err(|_| "PNG trailer validation failed")?;
    if !super::png_decode::plain(reader.info()) {
        return Ok(None);
    }
    Ok(Some(NativeResult {
        body,
        dimensions: [width, height],
        origin: [width, height],
        format: OutputFormat::Bmp,
    }))
}

#[cfg(test)]
#[path = "rgb_png_bmp_tests.rs"]
mod tests;
