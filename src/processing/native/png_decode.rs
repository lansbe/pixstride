//! Bounded Rust PNG inflation before native geometry; preserve metadata/HDR fallbacks.
use crate::{request::Request, Error};
use fast_png::{BitDepth, ColorType, DecodeOptions, Decoder, Info, Limits, Transformations};
use libvips::{
    enums::Interpretation,
    voption::{Setter, VOption},
    VipsImage,
};
use std::io::Cursor;

struct Decoded {
    dimensions: [i32; 2],
    bands: i32,
    pixels: Vec<u8>,
}

fn plain(info: &Info<'_>) -> bool {
    !info.interlaced
        && info.bit_depth == BitDepth::Eight
        && matches!(
            info.color_type,
            ColorType::Rgb | ColorType::Rgba | ColorType::Grayscale | ColorType::GrayscaleAlpha
        )
        && info.icc_profile.is_none()
        && info.exif_metadata.is_none()
        && info.source_gamma.is_none()
        && info.source_chromaticities.is_none()
        && info.srgb.is_none()
        && info.coding_independent_code_points.is_none()
        && info.mastering_display_color_volume.is_none()
        && info.content_light_level.is_none()
        && info.trns.is_none()
        && info.sbit.is_none()
        && info.bkgd.is_none()
        && info.palette.is_none()
        && info.animation_control.is_none()
        && info.frame_control.is_none()
}

fn decode(data: &[u8], r: &Request) -> Result<Option<Decoded>, Error> {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n")
        || !r.strip_metadata
        || !r.strip_color_profile
        || r.extra.keep_copyright
        || r.extra.preserve_hdr
    {
        return Ok(None);
    }
    if data.len() > r.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("PNG source exceeds byte limit");
    }
    // png defaults to ignoring Adler32 and tolerating ancillary CRC failures.
    // Require both checks, including late chunks read by finish().
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
    let info = reader.info();
    if !plain(info) {
        return Ok(None);
    }
    let (width, height) = (info.width as usize, info.height as usize);
    crate::image::dimensions(width, height)?;
    let pixels = width.checked_mul(height).ok_or("PNG dimensions overflow")?;
    if pixels > r.extra.max_source_pixels || pixels > r.extra.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    let bands = info.color_type.samples();
    let expected = pixels
        .checked_mul(bands)
        .ok_or("PNG raster exceeds limit")?;
    if expected > crate::MAX_FILE {
        return Ok(None);
    }
    if reader.output_buffer_size() != Some(expected) {
        return Err("inconsistent PNG raster size");
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(expected)
        .map_err(|_| "PNG decode allocation failed")?;
    pixels.resize(expected, 0);
    let output = reader
        .next_frame(&mut pixels)
        .map_err(|_| "PNG frame decode failed")?;
    if output.width as usize != width
        || output.height as usize != height
        || output.bit_depth != BitDepth::Eight
        || output.buffer_size() != expected
    {
        return Err("inconsistent PNG decoded frame");
    }
    // Validate the final image/chunk checksums and late metadata before exposing pixels.
    reader
        .finish()
        .map_err(|_| "PNG trailer validation failed")?;
    if !plain(reader.info()) {
        return Ok(None);
    }
    Ok(Some(Decoded {
        dimensions: [width as i32, height as i32],
        bands: bands as i32,
        pixels,
    }))
}

pub(super) fn try_load(data: &[u8], r: &Request) -> Result<Option<VipsImage>, Error> {
    let Some(Decoded {
        dimensions,
        bands,
        pixels,
    }) = decode(data, r)?
    else {
        return Ok(None);
    };
    VipsImage::new_from_owned_u8(pixels, dimensions[0], dimensions[1], bands)
        .and_then(|image| {
            image.copy_with_opts(VOption::new().set(
                "interpretation",
                if bands <= 2 {
                    Interpretation::BW
                } else {
                    Interpretation::Srgb
                } as i32,
            ))
        })
        .map(Some)
        .map_err(|_| "native PNG raster import failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        request::{self, Config},
        Buffer, Image,
    };

    fn sample() -> Vec<u8> {
        let mut pixels = Buffer::new(8).unwrap();
        pixels
            .as_mut_slice()
            .copy_from_slice(&[255, 0, 0, 128, 2, 3, 4, 0]);
        crate::png_encode::encode(&Image {
            width: 2,
            height: 1,
            pixels,
        })
        .unwrap()
        .as_slice()
        .to_vec()
    }

    #[test]
    fn decoding_rejects_wrong_adler_even_with_valid_idat_crc_and_late_bad_crc() {
        let source = sample();
        let mut corrupt = source.clone();
        let end = 41 + u32::from_be_bytes(corrupt[33..37].try_into().unwrap()) as usize;
        corrupt[end - 1] ^= 1;
        let crc = crc32fast::hash(&corrupt[37..end]);
        corrupt[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        assert!(decode(&corrupt, &request()).is_err(), "unchecked Adler32");
        let mut late = source[..source.len() - 12].to_vec();
        late.extend_from_slice(&3u32.to_be_bytes());
        late.extend_from_slice(b"tEXtk\0v");
        late.extend_from_slice(&0u32.to_be_bytes());
        late.extend_from_slice(&source[source.len() - 12..]);
        assert!(
            decode(&late, &request()).is_err(),
            "unchecked ancillary CRC"
        );
    }

    fn request() -> request::Request {
        request::parse(
            "/unsafe/plain/local:///fixture.png@png",
            &Config::unsafe_local(),
        )
        .unwrap()
    }

    #[test]
    fn decoding_preserves_straight_alpha_and_invisible_rgb() {
        let decoded = decode(&sample(), &request()).unwrap().unwrap();
        assert_eq!(decoded.dimensions, [2, 1]);
        assert_eq!(decoded.bands, 4);
        assert_eq!(decoded.pixels, [255, 0, 0, 128, 2, 3, 4, 0]);
    }

    #[test]
    fn rejects_crc_truncation_and_source_budget_without_dropping_metadata() {
        let source = sample();
        let mut bad = source.clone();
        let idat_len = u32::from_be_bytes(bad[33..37].try_into().unwrap()) as usize;
        bad[33 + 8 + idat_len] ^= 1;
        assert!(decode(&bad, &request()).is_err());
        assert!(decode(&source[..source.len() - 1], &request()).is_err());
        let mut r = request();
        r.extra.max_source_pixels = 1;
        assert!(decode(&source, &r).is_err());
        let mut r = request();
        r.strip_metadata = false;
        assert!(decode(&source, &r).unwrap().is_none());
        let mut r = request();
        r.strip_color_profile = false;
        assert!(decode(&source, &r).unwrap().is_none());
    }
}
