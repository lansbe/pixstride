//! Mirror ordinary eight-bit PNGs one row at a time inside the isolated worker.
use super::engine::NativeResult;
use crate::{
    ops::Rotation,
    png_encode,
    request::{OutputFormat, Request},
    Error,
};
use fast_png::{BitDepth, ColorType, DecodeOptions, Decoder, Limits, Transformations};
use std::io::Cursor;

pub(super) fn try_process(
    data: &[u8],
    r: &Request,
    mark: &[u8],
) -> Result<Option<NativeResult>, Error> {
    if !super::indexed_bmp::eligible(r, mark, OutputFormat::Png)
        || !matches!(r.options.rotation, Rotation::R0 | Rotation::R180)
        || r.options.flip_vertical != (r.options.rotation == Rotation::R180)
        || r.options.flip_horizontal == (r.options.rotation == Rotation::R180)
        || data.get(..16) != Some(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        || data.len() < 33
        || data[24] != 8
        || !matches!(data[25], 0 | 2 | 4 | 6)
        || data[26..29] != [0; 3]
    {
        return Ok(None);
    }
    if data.len() > r.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("PNG source exceeds byte limit");
    }
    if !super::indexed_bmp::image_chunks_only(data)? {
        return Ok(None);
    }
    let mut options = DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_skip_ancillary_crc_failures(false);
    let mut decoder = Decoder::new_with_options(Cursor::new(data), options);
    decoder.set_transformations(Transformations::IDENTITY);
    decoder.set_limits(Limits {
        bytes: crate::MAX_FILE,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_| "PNG header decode failed")?;
    let info = reader.info();
    if !super::png_decode::plain(info) {
        return Ok(None);
    }
    let (w, h) = (info.width as usize, info.height as usize);
    crate::image::dimensions(w, h)?;
    let pixels = w.checked_mul(h).ok_or("PNG dimensions overflow")?;
    if pixels > r.extra.max_source_pixels || pixels > r.extra.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    let (color, depth) = reader.output_color_type();
    let channels = color.samples();
    let stride = w * channels;
    let size = (stride + 1).checked_mul(h).ok_or("PNG raster overflow")?;
    if w.max(h) > r.extra.max_result_dimension || size > crate::MAX_FILE || depth != BitDepth::Eight
    {
        return Ok(None);
    }
    let alpha = matches!(color, ColorType::GrayscaleAlpha | ColorType::Rgba);
    let mut previous = vec![0; stride];
    let mut mirrored = vec![0; stride];
    let mut raw = Vec::new();
    for y in 0..h {
        let row = reader
            .next_row()
            .map_err(|_| "PNG row decode failed")?
            .ok_or("missing PNG row")?;
        let row = row.data();
        if row.len() != stride {
            return Err("invalid PNG scanline size");
        }
        if y == 0 {
            // The packed encoder omits an entirely opaque alpha band. Keep its
            // full-raster route when the first row cannot establish transparency.
            if alpha && row.chunks_exact(channels).all(|px| px[channels - 1] == 255) {
                return Ok(None);
            }
            raw.try_reserve_exact(size)
                .map_err(|_| "PNG allocation failed")?;
        }
        match channels {
            1 => mirror::<1>(row, &mut mirrored),
            2 => mirror::<2>(row, &mut mirrored),
            3 => mirror::<3>(row, &mut mirrored),
            4 => mirror::<4>(row, &mut mirrored),
            _ => return Err("invalid PNG channel count"),
        }
        png_encode::append_best_row(&mut raw, &mirrored, &previous, channels);
        std::mem::swap(&mut previous, &mut mirrored);
    }
    reader
        .finish()
        .map_err(|_| "PNG trailer validation failed")?;
    Ok(Some(NativeResult {
        body: png_encode::finish(w, h, channels, raw)?,
        dimensions: [w, h],
        origin: [w, h],
        format: OutputFormat::Png,
    }))
}

fn mirror<const N: usize>(row: &[u8], out: &mut [u8]) {
    for (dst, src) in out
        .as_chunks_mut::<N>()
        .0
        .iter_mut()
        .zip(row.as_chunks::<N>().0.iter().rev())
    {
        *dst = *src;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{self, Config};
    #[test]
    fn streamed_mirror_matches_the_packed_encoder_and_validates_trailer() {
        let r = request::parse(
            "/unsafe/fl:1:0/plain/local:///source.png@png",
            &Config::unsafe_local(),
        )
        .unwrap();
        for channels in 1..=4 {
            let pixels: Vec<u8> = (0..37 * 5 * channels).map(|n| (n * 71) as u8).collect();
            let mut src = png_encode::encode_pixels(37, 5, &pixels, channels).unwrap();
            let expected: Vec<u8> = pixels
                .chunks_exact(37 * channels)
                .flat_map(|row| row.chunks_exact(channels).rev().flatten().copied())
                .collect();
            let actual = try_process(&src, &r, &[]).unwrap().unwrap();
            assert_eq!(
                actual.body,
                png_encode::encode_pixels(37, 5, expected, channels).unwrap()
            );
            let end = src.len() - 1;
            src[end] ^= 1;
            assert!(try_process(&src, &r, &[]).is_err());
        }
    }
}
