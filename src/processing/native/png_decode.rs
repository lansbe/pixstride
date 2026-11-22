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
    indexed: bool,
    plan: Option<super::scale::Plan>,
}

pub(super) struct Palette {
    pub dimensions: [i32; 2],
    pub pixels: Vec<u8>,
    pub colors: [u8; 768],
}

pub(super) fn indices(data: &[u8], request: &Request) -> Result<Option<Palette>, Error> {
    let mut colors = [0; 768];
    Ok(
        decode_raster(data, request, Some(&mut colors))?.map(|decoded| Palette {
            dimensions: decoded.dimensions,
            pixels: decoded.pixels,
            colors,
        }),
    )
}

pub(super) fn plain(info: &Info<'_>) -> bool {
    !info.interlaced
        && info.bit_depth == BitDepth::Eight
        && (matches!(
            info.color_type,
            ColorType::Rgb | ColorType::Rgba | ColorType::Grayscale | ColorType::GrayscaleAlpha
        ) && info.palette.is_none()
            || info.color_type == ColorType::Indexed
                && info.palette.as_deref().is_some_and(|p| p.len() == 256 * 3))
        && info.icc_profile.is_none()
        && info.exif_metadata.is_none()
        && info.source_gamma.is_none()
        && info.source_chromaticities.is_none()
        && info.gamma().is_none()
        && info.chromaticities().is_none()
        && info.srgb.is_none()
        && info.coding_independent_code_points.is_none()
        && info.mastering_display_color_volume.is_none()
        && info.content_light_level.is_none()
        && info.trns.is_none()
        && info.sbit.is_none()
        && info.bkgd.is_none()
        && info.animation_control.is_none()
        && info.frame_control.is_none()
}

// The decoder verifies CRCs in finish(), but may ignore malformed ancillary
// metadata rather than expose it through Info. A palette import must not erase
// any profile/precision/animation declaration, even one placed after IDAT.
fn palette_chunks_plain(data: &[u8]) -> Result<bool, Error> {
    let mut at = 8usize;
    let mut palette = false;
    let mut pixels = false;
    while let Some(header) = data.get(at..at.saturating_add(8)) {
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .filter(|&n| n <= data.len())
            .ok_or("invalid PNG chunk framing")?;
        match &header[4..8] {
            b"PLTE" if palette || pixels => return Ok(false),
            b"PLTE" => palette = true,
            b"IDAT" => pixels = true,
            b"IEND" => return Ok(true),
            b"tRNS" | b"iCCP" | b"eXIf" | b"gAMA" | b"cHRM" | b"sRGB" | b"cICP" | b"mDCV"
            | b"cLLI" | b"sBIT" | b"bKGD" | b"acTL" | b"fcTL" | b"fdAT" => return Ok(false),
            _ => {}
        }
        at = end;
    }
    Err("missing PNG end chunk")
}

fn decode(data: &[u8], r: &Request) -> Result<Option<Decoded>, Error> {
    decode_raster(data, r, None)
}

fn decode_raster(
    data: &[u8],
    r: &Request,
    palette: Option<&mut [u8; 768]>,
) -> Result<Option<Decoded>, Error> {
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
    // Normal loading expands a full opaque Indexed8 palette to RGB. Exact
    // palette permutations retain the one-byte plane with the same validation.
    decoder.set_transformations(if palette.is_some() {
        Transformations::IDENTITY
    } else {
        Transformations::EXPAND
    });
    decoder.set_limits(Limits {
        bytes: crate::MAX_FILE,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_| "PNG header decode failed")?;
    let info = reader.info();
    if !plain(info) || (palette.is_some() && info.color_type != ColorType::Indexed) {
        return Ok(None);
    }
    if let Some(palette) = palette {
        palette.copy_from_slice(info.palette.as_deref().ok_or("missing PNG palette")?);
    }
    let (width, height) = (info.width as usize, info.height as usize);
    crate::image::dimensions(width, height)?;
    let pixels = width.checked_mul(height).ok_or("PNG dimensions overflow")?;
    if pixels > r.extra.max_source_pixels || pixels > r.extra.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    let indexed = info.color_type == ColorType::Indexed;
    let (output_color, output_depth) = reader.output_color_type();
    if output_depth != BitDepth::Eight {
        return Err("inconsistent PNG output precision");
    }
    let bands = output_color.samples();
    let expected = pixels
        .checked_mul(bands)
        .ok_or("PNG raster exceeds limit")?;
    if expected > crate::MAX_FILE {
        return Ok(None);
    }
    if reader.output_buffer_size() != Some(expected) {
        return Err("inconsistent PNG raster size");
    }
    let source = [width as i32, height as i32];
    let strip_plan = super::png_resize::strips::plan(source, bands as i32, r)?;
    let (dimensions, pixels, plan) = if let Some(mut plan) = strip_plan {
        let (target, pixels) = super::png_resize::strips::resize(&mut reader, source, &plan)?;
        plan.scale = [1.0; 2];
        (target, pixels, Some(plan))
    } else {
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
            || output.color_type != output_color
            || output.buffer_size() != expected
        {
            return Err("inconsistent PNG decoded frame");
        }
        (source, pixels, None)
    };
    // Validate the final image/chunk checksums and late metadata before exposing pixels.
    reader
        .finish()
        .map_err(|_| "PNG trailer validation failed")?;
    if !plain(reader.info()) || (indexed && !palette_chunks_plain(data)?) {
        return Ok(None);
    }
    Ok(Some(Decoded {
        dimensions,
        plan,
        bands: bands as i32,
        pixels,
        indexed,
    }))
}

pub(super) fn try_load(
    data: &[u8],
    r: &Request,
) -> Result<Option<(VipsImage, Option<super::scale::Plan>)>, Error> {
    let Some(Decoded {
        mut dimensions,
        bands,
        mut pixels,
        indexed,
        mut plan,
    }) = decode(data, r)?
    else {
        return Ok(None);
    };
    if plan.is_none() {
        plan = super::png_resize::plan(dimensions, bands, r)?;
        if let Some(plan) = &mut plan {
            (dimensions, pixels) = super::png_resize::resize(pixels, dimensions, plan)?;
        }
    }
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
        .and_then(|mut image| {
            if indexed {
                image.set_int("palette", 1)?;
                image.set_int("bits-per-sample", 8)?;
            }
            Ok(image)
        })
        .map(|image| Some((image, plan)))
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
    fn resized_alpha_jpeg_validates_the_complete_original_before_downscaling() {
        super::super::engine::initialize().unwrap();
        let source = sample();
        let mut r = request::parse(
            "/unsafe/rs:fit:1:1/plain/local:///fixture.png@jpg",
            &Config::unsafe_local(),
        )
        .unwrap();
        let (image, plan) = try_load(&source, &r).unwrap().unwrap();
        assert_eq!((image.get_width(), image.get_height()), (1, 1));
        assert_eq!(plan.unwrap().scale, [1.0; 2]);
        assert!(try_load(&source[..source.len() - 1], &r).is_err());
        let mut corrupt = source.clone();
        let end = 41 + u32::from_be_bytes(corrupt[33..37].try_into().unwrap()) as usize;
        corrupt[end - 1] ^= 1;
        let crc = crc32fast::hash(&corrupt[37..end]);
        corrupt[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        assert!(try_load(&corrupt, &r).is_err());
        r.extra.max_source_pixels = 1;
        assert!(try_load(&source, &r).is_err());
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

#[cfg(test)]
#[path = "png_palette_decode_tests.rs"]
mod palette_tests;
