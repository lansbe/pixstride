//! Compatibility processing facade. Existing benchmark entry points remain unchanged.
use crate::request::{OutputFormat, Request};
use crate::{Buffer, Error, Image};

pub struct Processed {
    pub body: Buffer,
    pub mime: &'static str,
    pub width: usize,
    pub height: usize,
}

fn jpeg_frame(data: &[u8]) -> Result<Option<(u8, usize, usize, usize)>, Error> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return Ok(None);
    }
    let mut p = 2usize;
    while p < data.len() {
        if data[p] != 0xff {
            return Err("invalid JPEG marker");
        }
        while data.get(p) == Some(&0xff) {
            p += 1;
        }
        let marker = *data.get(p).ok_or("truncated JPEG marker")?;
        p += 1;
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let len = data.get(p..p + 2).ok_or("truncated JPEG segment")?;
        let n = u16::from_be_bytes([len[0], len[1]]) as usize;
        if n < 2 || n > data.len() - p {
            return Err("invalid JPEG segment length");
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if n < 8 {
                return Err("truncated JPEG frame");
            }
            let h = u16::from_be_bytes([data[p + 3], data[p + 4]]) as usize;
            let w = u16::from_be_bytes([data[p + 5], data[p + 6]]) as usize;
            crate::image::dimensions(w, h)?;
            return Ok(Some((marker, w, h, data[p + 7] as usize)));
        }
        p += n;
    }
    Err("JPEG frame absent")
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() > crate::MAX_FILE {
        return Err("input exceeds file limit");
    }
    if crate::svg::looks_like(data) {
        return crate::svg::decode(data);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        if data.len() < 33 {
            return Err("truncated PNG header");
        }
        if data[24] != 8 || data[28] != 0 {
            return crate::png_extended::decode(data);
        }
        return crate::png::decode(data);
    }
    if let Some((frame, _, _, components)) = jpeg_frame(data)? {
        return if frame == 0xc2 {
            crate::jpeg::progressive::decode(data)
        } else if components == 4 {
            crate::jpeg::cmyk::decode(data)
        } else {
            crate::jpeg::decode(data)
        };
    }
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        return crate::webp::decode(data);
    }
    if crate::extra_formats::handles(data) {
        return crate::extra_formats::decode(data);
    }
    crate::image::decode(data)
}

pub fn needs_extended_decode(data: &[u8]) -> bool {
    if crate::svg::looks_like(data) {
        return true;
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return data.len() >= 29 && (data[24] != 8 || data[28] != 0);
    }
    if data.starts_with(&[0xff, 0xd8]) {
        return matches!(
            jpeg_frame(data),
            Ok(Some((0xc2, _, _, _))) | Ok(Some((_, _, _, 4)))
        );
    }
    (data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"))
        || crate::extra_formats::handles(data)
}

pub fn legacy_transform(
    data: &[u8],
    width: usize,
    height: usize,
    method: &[u8],
    format: &[u8],
) -> Result<Buffer, Error> {
    let image = decode(data)?;
    let image = crate::resize::resize(
        image,
        width,
        height,
        if method == b"fast" {
            b"lanczos3"
        } else {
            method
        },
    )?;
    match format {
        b"bmp" => crate::image::encode_bmp(&image),
        b"png" => crate::png_encode::encode(&image),
        b"jpg" | b"jpeg" => crate::jpeg_encode::encode(&image, 80),
        b"webp" => crate::webp::encode(&image),
        b"tif" | b"tiff" => crate::extra_encode::encode_tiff(&image),
        b"ico" => crate::extra_encode::encode_ico(&image),
        b"gif" => crate::gif_encode::encode(&image),
        _ => Err("unsupported output format"),
    }
}

fn default_format(data: &[u8]) -> Result<OutputFormat, Error> {
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Ok(OutputFormat::Gif)
    } else if data.starts_with(&[0xff, 0xd8]) {
        Ok(OutputFormat::Jpeg)
    } else if data.starts_with(b"BM") {
        Ok(OutputFormat::Bmp)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Ok(OutputFormat::Webp)
    } else if data.starts_with(b"II*") || data.starts_with(b"MM\0*") {
        Ok(OutputFormat::Tiff)
    } else if data.starts_with(b"\0\0\x01\0") {
        Ok(OutputFormat::Ico)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(OutputFormat::Png)
    } else {
        Err("explicit supported output format required for this source")
    }
}

/// Stream a fully validated opaque RGB8 PNG into the same Lanczos3 RGBA
/// destination as the general path. Other color modes and unsupported stream
/// plans return None and retain their ordinary decoder and geometry path.
fn stream_png_x4(data: &[u8], width: usize, height: usize) -> Result<Option<Image>, Error> {
    let parsed = crate::png::parse_for_transform(data)?;
    let (source_width, source_height) = match parsed.opaque_rgb_dimensions() {
        Some(dimensions) => dimensions,
        None => return Ok(None),
    };
    if width.checked_mul(4) != Some(source_width) || height.checked_mul(4) != Some(source_height) {
        return Ok(None);
    }
    let target = match crate::resize::StreamRgbaTarget::new(width, height) {
        Ok(target) => target,
        Err(_) => return Ok(None),
    };
    let stride = width.checked_mul(3).ok_or("image size overflow")?;
    let mut stream = match crate::resize::StreamRgbBmp::new(
        source_width,
        source_height,
        width,
        height,
        b"lanczos3",
        target,
        stride,
        crate::png::opaque_rgb_to_rgba,
        crate::image::pack_bgr_row,
    ) {
        Ok(Some(stream)) => stream,
        Ok(None) | Err(_) => return Ok(None),
    };
    // No result is returned until the complete stream, row filters, chunk
    // CRCs and Adler checksum have all been validated.
    parsed.stream_rgb_rows(&mut |y, row| stream.push(y, row))?;
    Ok(Some(stream.finish()?.into_image()))
}

pub fn process(
    data: &mut [u8],
    request: &Request,
    watermark: &crate::watermark::Config,
) -> Result<Processed, Error> {
    if !request.strip_metadata {
        return Err("retaining all source metadata is not yet supported");
    }
    let has_watermark = request
        .watermark
        .as_ref()
        .is_some_and(|spec| spec.opacity > 0.0);
    if has_watermark && !watermark.is_configured() {
        return Err("watermark image is not configured");
    }
    let format = match request.format {
        Some(f) => f,
        None => default_format(data)?,
    };
    let orientation = if request.auto_rotate {
        crate::metadata::orientation(data)?
    } else {
        1
    };
    let mut profile = crate::metadata::extract_icc(data)?;
    if profile.is_some()
        && !request.strip_color_profile
        && (request.options.background.is_some()
            || request.options.watermark.is_some()
            || has_watermark)
    {
        return Err(
            "ICC preservation with background or watermark needs explicit color conversion",
        );
    }
    if format == OutputFormat::Webp && request.quality.is_some_and(|q| q != 100) {
        return Err("lossy WebP quality is unsupported");
    }
    // Reuse qualified banded BMP resizes and validated RGB8 PNG identities.
    // Extended operations and formats still pass through the full facade below.
    let o = &request.options;
    if matches!(format, OutputFormat::Bmp | OutputFormat::Png)
        && orientation == 1
        && profile.is_none()
        && o.crop.is_none()
        && o.rotation == crate::ops::Rotation::R0
        && !o.flip_horizontal
        && !o.flip_vertical
        && o.padding == crate::ops::Padding::default()
        && o.background.is_none()
        && o.watermark.is_none()
        && matches!(
            o.mode,
            crate::ops::ResizeMode::Fit | crate::ops::ResizeMode::Force
        )
        && request.effects.blur_sigma == 0.0
        && request.effects.sharpen_sigma == 0.0
        && request.effects.pixelate <= 1
        && request.effects.trim.is_none()
        && !has_watermark
        && !needs_extended_decode(data)
    {
        let dims = if data.starts_with(b"\x89PNG\r\n\x1a\n") && data.len() >= 33 {
            Some((
                u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize,
                u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize,
            ))
        } else {
            jpeg_frame(data)?.map(|(_, w, h, _)| (w, h))
        };
        if let Some((sw, sh)) = dims {
            crate::image::dimensions(sw, sh)?;
            let (width, height) = crate::ops::output_resize_dimensions(sw, sh, o)?;
            let png_identity = format == OutputFormat::Png
                && sw == width
                && sh == height
                && data.starts_with(b"\x89PNG\r\n\x1a\n")
                && data.len() >= 33
                && data[24] == 8
                && data[25] == 2
                && data[28] == 0;
            if format == OutputFormat::Bmp || png_identity {
                let method: &[u8] = if png_identity {
                    b"fast"
                } else {
                    match o.method {
                        crate::ops::Filter::Lanczos3 => b"fast",
                    }
                };
                let output_format: &[u8] = if png_identity { b"png" } else { b"bmp" };
                let out = crate::image::transform(data, width, height, method, output_format)?;
                let body = match out {
                    crate::image::Output::Owned(b) => b,
                    crate::image::Output::Borrowed(bytes) => {
                        let mut b = Buffer::new(bytes.len())?;
                        b.as_mut_slice().copy_from_slice(bytes);
                        b
                    }
                };
                return Ok(Processed {
                    body,
                    mime: if png_identity {
                        "image/png"
                    } else {
                        "image/bmp"
                    },
                    width,
                    height,
                });
            }
        }
    }
    // The compressed output path must use full Lanczos arithmetic, not the
    // different x4 BMP prehalve shortcut. Keep eligibility intentionally
    // narrow; all unsupported operations retain the existing facade below.
    if matches!(format, OutputFormat::Png | OutputFormat::Jpeg)
        && orientation == 1
        && profile.is_none()
        && request.watermark.is_none()
        && o.crop.is_none()
        && o.rotation == crate::ops::Rotation::R0
        && !o.flip_horizontal
        && !o.flip_vertical
        && o.padding == crate::ops::Padding::default()
        && o.background.is_none()
        && o.watermark.is_none()
        && matches!(
            o.mode,
            crate::ops::ResizeMode::Fit | crate::ops::ResizeMode::Force
        )
        && matches!(o.gravity, crate::ops::Gravity::Center)
        && matches!(o.method, crate::ops::Filter::Lanczos3)
        && request.effects.blur_sigma == 0.0
        && request.effects.sharpen_sigma == 0.0
        && request.effects.pixelate <= 1
        && request.effects.trim.is_none()
        && data.starts_with(b"\x89PNG\r\n\x1a\n")
        && data.len() >= 33
        && data[24] == 8
        && data[25] == 2
        && data[28] == 0
    {
        let source_width = u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize;
        let source_height = u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize;
        let raw_size = source_width
            .checked_mul(3)
            .and_then(|n| n.checked_add(1))
            .and_then(|n| n.checked_mul(source_height));
        if raw_size.is_some_and(|n| n >= 256 * 1024) {
            crate::image::dimensions(source_width, source_height)?;
            let (width, height) =
                crate::ops::output_resize_dimensions(source_width, source_height, o)?;
            if width.checked_mul(4) == Some(source_width)
                && height.checked_mul(4) == Some(source_height)
            {
                if let Some(image) = stream_png_x4(data, width, height)? {
                    let (body, mime) = match format {
                        OutputFormat::Png => (crate::png_encode::encode(&image)?, "image/png"),
                        OutputFormat::Jpeg => (
                            crate::jpeg_encode::encode(
                                &image,
                                request.quality.filter(|q| *q != 0).unwrap_or(80),
                            )?,
                            "image/jpeg",
                        ),
                        _ => unreachable!(),
                    };
                    return Ok(Processed {
                        body,
                        mime,
                        width,
                        height,
                    });
                }
            }
        }
    }
    let mut decoded = decode(data)?;
    if request.strip_color_profile {
        if let Some(icc) = profile.take() {
            crate::metadata::convert_to_srgb(&mut decoded, &icc)?;
        }
    }
    let oriented = crate::metadata::orient(decoded, orientation)?;
    let trimmed = match request.effects.trim {
        Some(t) => crate::effects::trim(oriented, t)?,
        None => oriented,
    };
    let geometry = crate::ops::apply_geometry(trimmed, &request.options)?;
    let filtered = crate::effects::apply_filters(geometry, &request.effects)?;
    let image = crate::ops::apply_decorations(filtered, &request.options)?;
    let image = crate::watermark::apply(image, watermark, request.watermark.as_ref())?;
    let width = image.width;
    let height = image.height;
    let (mut body, mime) = if format == OutputFormat::Webp {
        (crate::webp::encode_owned(image)?, "image/webp")
    } else {
        let encoded = match format {
            OutputFormat::Bmp => (crate::image::encode_bmp(&image)?, "image/bmp"),
            OutputFormat::Png => (crate::png_encode::encode(&image)?, "image/png"),
            OutputFormat::Jpeg => (
                crate::jpeg_encode::encode(
                    &image,
                    request.quality.filter(|q| *q != 0).unwrap_or(80),
                )?,
                "image/jpeg",
            ),
            OutputFormat::Tiff => (crate::extra_encode::encode_tiff(&image)?, "image/tiff"),
            OutputFormat::Ico => (crate::extra_encode::encode_ico(&image)?, "image/x-icon"),
            OutputFormat::Gif => (crate::gif_encode::encode(&image)?, "image/gif"),
            OutputFormat::Webp => unreachable!(),
        };
        drop(image);
        encoded
    };
    if let Some(icc) = profile {
        body = match format {
            OutputFormat::Png => crate::metadata::preserve_icc_png(body.as_slice(), &icc)?,
            OutputFormat::Jpeg => crate::metadata::preserve_icc_jpeg(body.as_slice(), &icc)?,
            OutputFormat::Webp => crate::metadata::preserve_icc_webp(body.as_slice(), &icc)?,
            OutputFormat::Bmp | OutputFormat::Tiff | OutputFormat::Ico | OutputFormat::Gif => {
                return Err("ICC preservation unavailable for selected output")
            }
        };
    }
    Ok(Processed {
        body,
        mime,
        width,
        height,
    })
}
