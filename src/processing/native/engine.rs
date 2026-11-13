//! Native loading, colour handling and frame processing. Runs only in a child.
use super::{encode, geometry, sniff};
use crate::{
    request::{OutputFormat, Request},
    Error,
};
use libvips::{
    enums::{BandFormat, Interpretation, Kernel},
    voption::{Setter, VOption},
    Vips, VipsImage,
};

pub(super) fn checked(width: i32, height: i32) -> Result<(), Error> {
    if width <= 0
        || height <= 0
        || width > 100_000
        || height > 100_000
        || i64::from(width) * i64::from(height) > crate::MAX_PIXELS as i64
    {
        Err("native image exceeds pixel limit")
    } else {
        Ok(())
    }
}

pub(super) fn load(data: &[u8], frames: usize, thumbnail: bool) -> Result<VipsImage, Error> {
    let format = sniff(data);
    if format == Some(OutputFormat::Svg) {
        super::svg::validate(data)?;
    }
    if matches!(format, Some(OutputFormat::Bmp | OutputFormat::Ico)) || data.starts_with(b"P6") {
        let img = crate::pipeline::decode(data)?;
        return VipsImage::new_from_memory_copy(
            img.pixels.as_slice(),
            img.width as i32,
            img.height as i32,
            4,
            BandFormat::Uchar,
        )
        .and_then(|i| {
            i.copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        })
        .map_err(|_| "native raster import failed");
    }
    if format.is_none() && !data.starts_with(b"P6") && !data.starts_with(b"P5") {
        return Err("unsupported native source format");
    }
    let mut option = VOption::new()
        .set("fail_on", libvips::enums::FailOn::Error as i32)
        .set("access", libvips::enums::Access::Random as i32);
    if matches!(
        format,
        Some(
            OutputFormat::Gif
                | OutputFormat::Jxl
                | OutputFormat::Webp
                | OutputFormat::Tiff
                | OutputFormat::Heic
                | OutputFormat::Avif
        )
    ) {
        option = option.set("n", frames as i32);
    }
    if thumbnail && matches!(format, Some(OutputFormat::Heic | OutputFormat::Avif)) {
        option = option.set("thumbnail", true);
    }
    VipsImage::new_from_buffer_with_opts(data, "", option)
        .map_err(|_| "native source decode failed")
}

fn prepare(mut image: VipsImage, r: &Request) -> Result<VipsImage, Error> {
    if r.auto_rotate {
        image = image.autorot().map_err(|_| "native EXIF rotation failed")?;
    }
    if r.strip_color_profile && image.get_typeof("icc-profile-data").unwrap_or(0) != 0 {
        image = image
            .icc_transform_with_opts(
                "srgb",
                VOption::new()
                    .set("embedded", true)
                    .set("depth", if r.extra.preserve_hdr { 16 } else { 8 }),
            )
            .map_err(|_| "native ICC conversion failed")?;
    } else if !r.extra.preserve_hdr {
        let gray = matches!(
            image.get_interpretation(),
            Ok(Interpretation::BW | Interpretation::Grey16)
        );
        image = image
            .colourspace(if gray {
                Interpretation::BW
            } else {
                Interpretation::Srgb
            })
            .map_err(|_| "native colour conversion failed")?;
    }
    if !r.extra.preserve_hdr
        && image.get_format().map_err(|_| "invalid pixel type")? != BandFormat::Uchar
    {
        image = image
            .cast(BandFormat::Uchar)
            .map_err(|_| "native eight-bit conversion failed")?;
    }
    if let Some(trim) = r.effects.trim {
        let options = VOption::new().set("threshold", f64::from(trim.threshold));
        let background = if let Some(c) = trim.color {
            c.map(f64::from).to_vec()
        } else {
            image
                .getpoint(0, 0)
                .map_err(|_| "native trim sample failed")?
        };
        let options = options.set("background", background.as_slice());
        let (mut x, mut y, mut w, mut h) = image
            .find_trim_with_opts(options)
            .map_err(|_| "native trim failed")?;
        if w > 0 && h > 0 {
            if r.extra.trim_equal[0] {
                x = x.min(image.get_width() - x - w);
                w = image.get_width() - 2 * x;
            }
            if r.extra.trim_equal[1] {
                y = y.min(image.get_height() - y - h);
                h = image.get_height() - 2 * y;
            }
            image = image
                .extract_area(x, y, w, h)
                .map_err(|_| "native trim crop failed")?;
        }
    }
    Ok(image)
}

fn effects(mut image: VipsImage, r: &Request) -> Result<VipsImage, Error> {
    if r.effects.blur_sigma > 0.0 {
        image = image
            .gaussblur(r.effects.blur_sigma)
            .map_err(|_| "native blur failed")?;
    }
    if r.effects.sharpen_sigma > 0.0 {
        image = image
            .sharpen_with_opts(VOption::new().set("sigma", r.effects.sharpen_sigma))
            .map_err(|_| "native sharpen failed")?;
    }
    if r.effects.pixelate > 1 {
        let block = r.effects.pixelate as f64;
        let width = (f64::from(image.get_width()) / block).ceil().max(1.0) as i32;
        let height = (f64::from(image.get_height()) / block).ceil().max(1.0) as i32;
        let (w, h) = (image.get_width(), image.get_height());
        image = image
            .resize_with_opts(
                f64::from(width) / f64::from(w),
                VOption::new()
                    .set("vscale", f64::from(height) / f64::from(h))
                    .set("kernel", Kernel::Linear as i32),
            )
            .map_err(|_| "native pixelation failed")?;
        image = geometry::resize(&image, w, h, true)?;
    }
    Ok(image)
}

pub(super) fn process(
    data: &[u8],
    r: &Request,
    mark: &[u8],
) -> Result<(Vec<u8>, usize, usize, OutputFormat), Error> {
    Vips::init("pixstride-native").map_err(|_| "cannot initialize native engine")?;
    Vips::cache_set_max(0);
    Vips::cache_set_max_mem(0);
    Vips::cache_set_max_files(0);
    Vips::concurrency_set(1);
    let sanitized;
    let data = if r.extra.encoding.sanitize_svg && sniff(data) == Some(OutputFormat::Svg) {
        sanitized = super::svg::sanitize(data)?;
        &sanitized[..]
    } else {
        data
    };
    let head = load(data, 1, r.extra.enforce_thumbnail)?;
    let frame_height = head.get_page_height().max(1);
    checked(head.get_width(), frame_height)?;
    let total = head.get_n_pages().max(1) as usize;
    let count = total.min(r.extra.max_frames).min(256);
    let pixels = (head.get_width() as usize)
        .checked_mul(frame_height as usize)
        .and_then(|n| n.checked_mul(count))
        .ok_or("animation dimensions overflow")?;
    if pixels > r.extra.max_source_pixels.min(crate::MAX_PIXELS)
        || (head.get_width() as usize) * (frame_height as usize) > r.extra.max_frame_pixels
    {
        return Err("native source pixel budget exceeded");
    }
    let source_format = sniff(data).unwrap_or(OutputFormat::Png);
    let preferred = if r.extra.preferred.is_empty() {
        &[OutputFormat::Jpeg, OutputFormat::Png, OutputFormat::Gif][..]
    } else {
        &r.extra.preferred
    };
    let transparent = r.options.background.is_none()
        && (head.hasalpha()
            || r.extra.extend
            || r.extra.extend_aspect
            || r.options.padding != Default::default());
    let compatible = |f: &&OutputFormat| count == 1 || **f == OutputFormat::Webp;
    let selected = r
        .extra
        .force_formats
        .iter()
        .find(compatible)
        .copied()
        .or(r.format)
        .or_else(|| r.extra.auto_formats.iter().find(compatible).copied());
    let mut format = selected.unwrap_or_else(|| {
        if source_format == OutputFormat::Svg && !r.extra.encoding.rasterize_svg {
            return OutputFormat::Svg;
        }
        if preferred.contains(&source_format) {
            return source_format;
        }
        preferred
            .iter()
            .copied()
            .find(|f| {
                (!transparent || *f != OutputFormat::Jpeg)
                    && (count == 1
                        || matches!(
                            f,
                            OutputFormat::Gif | OutputFormat::Webp | OutputFormat::Tiff
                        ))
            })
            .unwrap_or(preferred[0])
    });
    if format == OutputFormat::Svg
        || r.extra.raw
        || (r.extra.skip.contains(&source_format)
            && (r.format.is_none() || format == source_format))
    {
        if format == OutputFormat::Svg && source_format != OutputFormat::Svg {
            return Err("SVG output requires an SVG source");
        }
        head.copy_memory().map_err(|_| "native validation failed")?;
        return Ok((
            data.to_vec(),
            head.get_width() as usize,
            frame_height as usize,
            source_format,
        ));
    }
    let animated = count > 1
        && matches!(
            format,
            OutputFormat::Gif | OutputFormat::Webp | OutputFormat::Tiff
        );
    let input = if animated {
        load(data, count, r.extra.enforce_thumbnail)?
    } else {
        head
    };
    let count = if animated { count } else { 1 };
    let mut frames = Vec::new();
    let mut sum = 0usize;
    for index in 0..count {
        let frame = input
            .extract_area(
                0,
                index as i32 * frame_height,
                input.get_width(),
                frame_height,
            )
            .map_err(|_| "native frame extraction failed")?;
        let frame = prepare(frame, r)?;
        let (frame, dpr) = geometry::apply(frame, r, source_format == OutputFormat::Svg)?;
        let frame = effects(frame, r)?;
        let frame = geometry::decorate(frame, r, dpr)?;
        let mut frame = super::composite::watermark(frame, mark, r, dpr)?;
        let limit = match format {
            OutputFormat::Ico => 256,
            OutputFormat::Webp => 16383,
            OutputFormat::Avif | OutputFormat::Heic => 16384,
            OutputFormat::Gif => 65535,
            _ => 100_000,
        };
        if frame.get_width().max(frame.get_height()) > limit {
            let factor = f64::from(limit) / f64::from(frame.get_width().max(frame.get_height()));
            frame = geometry::resize_scale(&frame, factor, factor, false)?;
        }
        checked(frame.get_width(), frame.get_height())?;
        sum = sum
            .checked_add(frame.get_width() as usize * frame.get_height() as usize)
            .ok_or("animation output overflow")?;
        if sum > crate::MAX_PIXELS {
            return Err("animation output exceeds cumulative pixel limit");
        }
        frames.push(frame);
    }
    let width = frames[0].get_width() as usize;
    let height = frames[0].get_height() as usize;
    if frames
        .iter()
        .any(|f| f.get_width() as usize != width || f.get_height() as usize != height)
    {
        return Err("animation frames have inconsistent dimensions");
    }
    let mut image = if count > 1 {
        VipsImage::arrayjoin_with_opts(&frames, VOption::new().set("across", 1))
            .map_err(|_| "native animation join failed")?
    } else {
        frames.pop().unwrap()
    };
    if count > 1 {
        image
            .set_int("page-height", height as i32)
            .map_err(|_| "cannot set animation page height")?;
        image
            .set_int("loop", input.get_int("loop").unwrap_or(0))
            .map_err(|_| "cannot set animation loop")?;
        if let Ok(mut delay) = input.get_array_int("delay") {
            delay.truncate(count);
            image
                .set_array_int("delay", &delay)
                .map_err(|_| "cannot set animation delay")?;
        }
    }
    if format == OutputFormat::Avif && (width < 16 || height < 16) {
        format = if image.hasalpha() {
            OutputFormat::Png
        } else {
            OutputFormat::Jpeg
        };
    }
    let bytes = encode::save(&image, r, format, height as i32)?;
    Ok((bytes, width, height, format))
}
