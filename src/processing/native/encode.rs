//! Bounded encoding attempts. Saver options are built from typed values only.
use crate::{
    request::{OutputFormat, Request},
    Buffer, Error, Image,
};
use libvips::{
    enums::{BandFormat, Interpretation},
    voption::{Setter, VOption},
    VipsImage,
};

#[cfg(test)]
#[path = "encoding_tests.rs"]
mod encoding_tests;

pub(super) fn raster(image: &VipsImage) -> Result<Image, Error> {
    let mut image = image
        .colourspace(Interpretation::Srgb)
        .map_err(|_| "native RGB conversion failed")?;
    if !image.hasalpha() {
        image = image.addalpha().map_err(|_| "native alpha failed")?;
    }
    image = image
        .cast(BandFormat::Uchar)
        .map_err(|_| "native raster conversion failed")?;
    super::engine::checked(image.get_width(), image.get_height())?;
    let memory = super::raster::export(&image)?;
    let mut pixels = Buffer::new(memory.len())?;
    pixels.as_mut_slice().copy_from_slice(&memory);
    Ok(Image {
        width: image.get_width() as usize,
        height: image.get_height() as usize,
        pixels,
    })
}

fn png_depth(image: &VipsImage, r: &Request) -> Option<i32> {
    if r.extra.encoding.png_quantize {
        return Some(match r.extra.encoding.png_colors {
            2 => 1,
            3..=4 => 2,
            5..=16 => 4,
            _ => 8,
        });
    }
    if image.get_typeof("palette").unwrap_or(0) == 0 || image.get_int("palette").unwrap_or(0) == 0 {
        return None;
    }
    match image.get_int("bits-per-sample").unwrap_or(0) {
        1 => Some(1),
        2 => Some(2),
        3..=4 => Some(4),
        5..=8 => Some(8),
        _ => None,
    }
}

fn attempt(
    image: &VipsImage,
    r: &Request,
    format: OutputFormat,
    quality: u8,
    page_height: i32,
) -> Result<Vec<u8>, Error> {
    if format == OutputFormat::Gif {
        return gif(image, page_height);
    }
    if format == OutputFormat::Bmp {
        return super::bmp_encode::save(image);
    }
    if format == OutputFormat::Ico {
        let raster = raster(image)?;
        let b = crate::extra_encode::encode_ico(&raster)?;
        return Ok(b.as_slice().to_vec());
    }
    let encoding = &r.extra.encoding;
    let keep = 31;
    let mut options = VOption::new()
        .set("keep", keep)
        .set("page_height", page_height);
    if r.extra.preserve_hdr
        && image
            .get_format()
            .map_err(|_| "invalid native pixel type")?
            == BandFormat::Ushort
    {
        match format {
            OutputFormat::Png | OutputFormat::Jxl => options = options.set("bitdepth", 16),
            OutputFormat::Avif | OutputFormat::Heic => options = options.set("bitdepth", 12),
            OutputFormat::Tiff => {}
            _ => return Err("selected format cannot preserve high precision"),
        }
    }
    if matches!(
        format,
        OutputFormat::Jpeg
            | OutputFormat::Webp
            | OutputFormat::Heic
            | OutputFormat::Avif
            | OutputFormat::Jxl
            | OutputFormat::Tiff
    ) {
        options = options.set("Q", i32::from(quality));
    }
    if format == OutputFormat::Jpeg {
        options = options.set("interlace", encoding.jpeg_progressive);
    }
    if format == OutputFormat::Jxl {
        options = options
            .set("effort", i32::from(encoding.jxl_effort))
            .set("lossless", quality == 100);
    }
    if format == OutputFormat::Webp {
        let preset = match encoding.webp_preset.as_str() {
            "picture" => 1,
            "photo" => 2,
            "drawing" => 3,
            "icon" => 4,
            "text" => 5,
            _ => 0,
        };
        options = options
            .set("lossless", quality == 100)
            .set("effort", i32::from(encoding.webp_effort))
            .set("preset", preset);
    }
    if format == OutputFormat::Avif {
        options = options
            .set(
                "compression",
                libvips::enums::ForeignHeifCompression::Av1 as i32,
            )
            .set("effort", i32::from(encoding.avif_effort));
    }
    if format == OutputFormat::Heic {
        options = options.set(
            "compression",
            libvips::enums::ForeignHeifCompression::Hevc as i32,
        );
    }
    if format == OutputFormat::Png {
        let depth = png_depth(image, r);
        let palette = depth.is_some();
        options = options
            .set("palette", palette)
            .set("interlace", encoding.png_interlaced)
            .set("bitdepth", depth.unwrap_or(8))
            .set(
                "filter",
                if palette {
                    libvips::enums::ForeignPngFilter::None as i32
                } else {
                    libvips::enums::ForeignPngFilter::All as i32
                },
            );
        if r.extra.preserve_hdr
            && image
                .get_format()
                .map_err(|_| "invalid native pixel type")?
                == BandFormat::Ushort
        {
            options = options.set("bitdepth", 16);
        }
    }
    let bytes = match image.write_to_buffer_with_opts(&format!(".{}", format.extension()), options)
    {
        Ok(bytes) => bytes,
        Err(_) if format == OutputFormat::Png && encoding.png_quantize && !r.extra.preserve_hdr => {
            image
                .write_to_buffer_with_opts(
                    ".png",
                    VOption::new().set("keep", keep).set("bitdepth", 8),
                )
                .map_err(|_| "native palette source export failed")?
        }
        Err(_) => return Err("native encoder unavailable or rejected output"),
    };
    if format == OutputFormat::Png && encoding.png_quantize {
        if r.extra.preserve_hdr {
            return Err("palette PNG cannot preserve high precision");
        }
        if bytes.get(25) != Some(&3) {
            return super::palette::encode(
                image,
                &bytes,
                quality,
                usize::from(encoding.png_colors),
                encoding.png_interlaced,
            );
        }
    }
    Ok(bytes)
}

pub(super) fn optimized_png(image: &Image) -> Result<Vec<u8>, Error> {
    let encoded = crate::png_encode::encode(image)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(encoded.as_slice().len())
        .map_err(|_| "PNG: allocation failed")?;
    bytes.extend_from_slice(encoded.as_slice());
    Ok(bytes)
}

fn filter_strip_rows(width: i32, bands: i32) -> i32 {
    // This route admits only 1..=4 uchar bands. Wider strips avoid repeated
    // filter halo work; cap the output strip at 4 MiB for very wide images.
    let bytes_per_row = i64::from(width.max(1)) * i64::from(bands.max(1));
    ((4 * 1024 * 1024) / bytes_per_row).clamp(1, 32) as i32
}

pub(super) fn save(
    image: VipsImage,
    r: &Request,
    format: OutputFormat,
    page_height: i32,
) -> Result<Vec<u8>, Error> {
    let pixel_type = image
        .get_format()
        .map_err(|_| "invalid native pixel type")?;
    if r.extra.preserve_hdr
        && pixel_type != BandFormat::Uchar
        && (!matches!(
            format,
            OutputFormat::Png
                | OutputFormat::Tiff
                | OutputFormat::Jxl
                | OutputFormat::Avif
                | OutputFormat::Heic
        ) || (pixel_type != BandFormat::Ushort
            && !matches!(format, OutputFormat::Tiff | OutputFormat::Jxl)))
    {
        return Err("selected format cannot preserve high precision");
    }
    let stripped = super::metadata::strip(&image, r)?;
    drop(image);
    let image = if format == OutputFormat::Jpeg
        && (r.effects.blur_sigma > 0.0 || r.effects.sharpen_sigma > 0.0)
        && !r.extra.preserve_hdr
        && super::shared::fits_memory(&stripped)
    {
        if r.extra.max_bytes == 0 {
            // Bounded output strips limit upstream filter working regions. Only
            // a single saver pass may consume this sequential image.
            let stream = stripped
                .sequential_with_opts(VOption::new().set(
                    "tile_height",
                    filter_strip_rows(stripped.get_width(), stripped.get_bands()),
                ))
                .map_err(|_| "native JPEG filter streaming failed")?;
            drop(stripped);
            stream
        } else {
            super::shared::materialize(stripped)?
        }
    } else {
        stripped
    };
    if format == OutputFormat::Png
        && r.strip_metadata
        && r.strip_color_profile
        && !r.extra.keep_copyright
        && !r.extra.preserve_hdr
        && !r.extra.encoding.png_quantize
        && png_depth(&image, r).is_none()
        && !r.extra.encoding.png_interlaced
        && image.get_format().ok() == Some(BandFormat::Uchar)
        && ((image.get_interpretation().ok() == Some(Interpretation::Srgb)
            && matches!(image.get_bands(), 3 | 4))
            || (image.get_interpretation().ok() == Some(Interpretation::BW)
                && matches!(image.get_bands(), 1 | 2)))
        && i64::from(image.get_width()) * i64::from(image.get_height()) * 4
            <= crate::MAX_FILE as i64
    {
        let pixels = super::raster::detach(image)?;
        let bytes = crate::png_encode::encode_pixels(
            pixels.width,
            pixels.height,
            pixels.memory,
            pixels.bands,
        )?
        .as_slice()
        .to_vec();
        if r.extra.max_bytes > 0 && bytes.len() > r.extra.max_bytes {
            return Err("size target unavailable for this format");
        }
        return Ok(bytes);
    }
    let image = &image;
    let quality = r
        .quality
        .filter(|&q| q > 0)
        .or_else(|| {
            r.extra
                .format_quality
                .iter()
                .find(|(f, _)| *f == format)
                .map(|(_, q)| *q)
                .filter(|q| *q > 0)
        })
        .unwrap_or(match format {
            OutputFormat::Webp => 79,
            OutputFormat::Avif => 63,
            OutputFormat::Jxl => 77,
            _ => 80,
        });
    let mut bytes = attempt(image, r, format, quality, page_height)?;
    let limit = r.extra.max_bytes;
    if limit > 0 && bytes.len() > limit {
        if !matches!(
            format,
            OutputFormat::Jpeg
                | OutputFormat::Webp
                | OutputFormat::Heic
                | OutputFormat::Avif
                | OutputFormat::Jxl
                | OutputFormat::Tiff
        ) {
            return Err("size target unavailable for this format");
        }
        // At most eight total encodes, with an explicit failure for an impossible target.
        let mut low = 1u8;
        let mut high = quality.saturating_sub(1);
        let mut best = None;
        for _ in 0..7 {
            if low > high {
                break;
            }
            let q = low + (high - low) / 2;
            let candidate = attempt(image, r, format, q, page_height)?;
            if candidate.len() <= limit {
                best = Some(candidate);
                low = q.saturating_add(1);
            } else {
                high = q.saturating_sub(1);
            }
        }
        bytes = best.ok_or("output cannot meet requested byte limit")?;
    }
    if bytes.len() > crate::MAX_FILE {
        return Err("native output exceeds file limit");
    }
    Ok(bytes)
}

fn gif(image: &VipsImage, page_height: i32) -> Result<Vec<u8>, Error> {
    let raster = raster(image)?;
    let width = u16::try_from(raster.width).map_err(|_| "GIF width exceeds limit")?;
    let height = u16::try_from(page_height).map_err(|_| "GIF height exceeds limit")?;
    let frame_bytes = raster
        .width
        .checked_mul(page_height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("GIF frame overflow")?;
    if frame_bytes == 0 || !raster.pixels.len().is_multiple_of(frame_bytes) {
        return Err("inconsistent GIF frame dimensions");
    }
    let delays = image.get_array_int("delay").unwrap_or_default();
    let loops = image.get_int("loop").unwrap_or(0);
    let mut out = Vec::new();
    {
        let mut encoder = ::gif::Encoder::new(&mut out, width, height, &[])
            .map_err(|_| "cannot initialize GIF encoder")?;
        if raster.pixels.len() > frame_bytes && loops != 1 {
            encoder
                .set_repeat(if loops <= 0 {
                    ::gif::Repeat::Infinite
                } else {
                    ::gif::Repeat::Finite((loops - 1).min(65535) as u16)
                })
                .map_err(|_| "cannot encode GIF loop")?;
        }
        for (index, bytes) in raster.pixels.chunks_exact(frame_bytes).enumerate() {
            let mut rgba = bytes.to_vec();
            let mut frame = ::gif::Frame::from_rgba_speed(width, height, &mut rgba, 10);
            frame.delay = (delays.get(index).copied().unwrap_or(100).max(0) / 10).min(65535) as u16;
            frame.dispose = ::gif::DisposalMethod::Background;
            encoder
                .write_frame(&frame)
                .map_err(|_| "cannot encode GIF frame")?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod filter_stream_tests {
    use super::*;

    #[test]
    fn filter_strips_fill_but_never_exceed_the_memory_budget() {
        for width in [640, 16384, 65536, 100000] {
            for bands in [1, 3, 4] {
                let rows = filter_strip_rows(width, bands);
                assert!((1..=32).contains(&rows));
                assert!(width * bands * rows <= 4 * 1024 * 1024);
                if rows < 32 {
                    assert!(width * bands * (rows + 1) > 4 * 1024 * 1024);
                }
            }
        }
    }

    #[test]
    fn wider_strips_preserve_native_blur_and_sharpen_jpeg_bytes() {
        super::super::engine::initialize().unwrap();
        let data: Vec<u8> = (0..97 * 61 * 3)
            .map(|v| ((v * 71 + v / 97) % 256) as u8)
            .collect();
        for filter in ["bl:2", "sh:1"] {
            let request = crate::request::parse(
                &format!("/unsafe/{filter}/q:85/plain/local:///fixture.jpg@jpg"),
                &crate::request::Config::unsafe_local(),
            )
            .unwrap();
            let mut outputs = Vec::new();
            for rows in [16, filter_strip_rows(73, 3)] {
                let image = VipsImage::new_from_memory_copy(&data, 97, 61, 3, BandFormat::Uchar)
                    .unwrap()
                    .copy_with_opts(
                        VOption::new().set("interpretation", Interpretation::Srgb as i32),
                    )
                    .unwrap()
                    .resize(0.75)
                    .unwrap();
                let image = if filter == "bl:2" {
                    image.gaussblur(2.0).unwrap()
                } else {
                    image
                        .sharpen_with_opts(VOption::new().set("sigma", 1.0))
                        .unwrap()
                };
                let image = super::super::metadata::strip(&image, &request).unwrap();
                let image = image
                    .sequential_with_opts(VOption::new().set("tile_height", rows))
                    .unwrap();
                outputs.push(
                    attempt(&image, &request, OutputFormat::Jpeg, 85, image.get_height()).unwrap(),
                );
            }
            assert_eq!(outputs[0], outputs[1], "{filter} changed encoded samples");
        }
    }
}
