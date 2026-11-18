//! Native loading, colour handling and frame processing. Runs only in a child.
use super::{encode, geometry, sniff};
use crate::{
    request::{OutputFormat, Request},
    Error,
};
use libvips::{
    enums::{Access, BandFormat, Interpretation},
    voption::{Setter, VOption},
    Vips, VipsImage,
};
pub(super) struct NativeResult {
    pub body: Vec<u8>,
    pub dimensions: [usize; 2],
    pub origin: [usize; 2],
    pub format: OutputFormat,
}

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
    load_with_owner(data, frames, thumbnail, None, Access::Random)
}

fn load_with_owner(
    data: &[u8],
    frames: usize,
    thumbnail: bool,
    owner: Option<&std::sync::Arc<Vec<u8>>>,
    access: Access,
) -> Result<VipsImage, Error> {
    let format = sniff(data);
    if format == Some(OutputFormat::Svg) {
        super::svg::validate(data)?;
    }
    if format == Some(OutputFormat::Bmp) {
        if let Some(image) = super::bmp_gray::try_load(data)? {
            return Ok(image);
        }
        if data.len() > crate::MAX_FILE {
            return Err("input exceeds file limit");
        }
        let (width, height, pixels) = crate::image::BmpView::parse(data)?.decode_rgb()?;
        return VipsImage::new_from_owned_u8(pixels, width as i32, height as i32, 3)
            .and_then(|i| {
                i.copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            })
            .map_err(|_| "native RGB import failed");
    }
    if format == Some(OutputFormat::Ico) || data.starts_with(b"P6") {
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
        .set("access", access as i32);
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
    if let Some(owner) = owner {
        super::shared::load(owner, option)
    } else {
        VipsImage::new_from_buffer_with_opts(data, "", option)
            .map_err(|_| "native source decode failed")
    }
}

fn prepare(mut image: VipsImage, r: &Request) -> Result<VipsImage, Error> {
    if r.auto_rotate
        && (image.get_typeof("orientation").unwrap_or(0) != 0
            || image.get_typeof("exif-ifd0-Orientation").unwrap_or(0) != 0)
    {
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
        let space = if gray {
            Interpretation::BW
        } else {
            Interpretation::Srgb
        };
        if image.get_interpretation().ok() != Some(space) {
            image = image
                .colourspace(space)
                .map_err(|_| "native colour conversion failed")?;
        }
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

pub(super) fn initialize() -> Result<(), Error> {
    static INITIALIZED: std::sync::OnceLock<Result<(), Error>> = std::sync::OnceLock::new();
    *INITIALIZED.get_or_init(|| {
        Vips::init("pixstride-native").map_err(|_| "cannot initialize native engine")?;
        Vips::cache_set_max(0);
        Vips::cache_set_max_mem(0);
        Vips::cache_set_max_files(0);
        Vips::concurrency_set(1);
        Ok(())
    })
}

struct ThreadCleanup;
impl Drop for ThreadCleanup {
    fn drop(&mut self) {
        Vips::error_clear();
        Vips::thread_shutdown();
    }
}

pub(super) fn process(mut data: Vec<u8>, r: &Request, mark: &[u8]) -> Result<NativeResult, Error> {
    if data.len() > r.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("native source exceeds byte limit");
    }
    if let Some([width, height]) = super::bmp_gray::source_dimensions(&data)? {
        let pixels = width.checked_mul(height).ok_or("BMP dimensions overflow")?;
        if pixels > r.extra.max_source_pixels || pixels > r.extra.max_frame_pixels {
            return Err("BMP source pixel budget exceeded");
        }
    }
    if let Some(result) = super::png_identity::try_process(&mut data, r, mark)? {
        return Ok(result);
    }
    if let Some(result) = super::indexed_bmp::try_process(&data, r, mark)? {
        return Ok(result);
    }
    if let Some(result) = super::rgb_png_bmp::try_process(&data, r, mark)? {
        return Ok(result);
    }
    if sniff(&data) == Some(OutputFormat::Svg) {
        if let Some(result) = super::fast_svg::try_process(&data, r)? {
            return Ok(result);
        }
    }
    initialize()?;
    // Declared before every image: their last references drop first, then
    // libvips releases this reused thread's private regions and error state.
    let _cleanup = ThreadCleanup;
    let encoded = std::sync::Arc::new(
        if r.extra.encoding.sanitize_svg && sniff(&data) == Some(OutputFormat::Svg) {
            super::svg::sanitize(&data)?
        } else {
            data
        },
    );
    let data = encoded.as_slice();
    let head = load_with_owner(
        data,
        1,
        r.extra.enforce_thumbnail,
        Some(&encoded),
        Access::Random,
    )?;
    let frame_height = head.get_page_height().max(1);
    let origin = [head.get_width() as usize, frame_height as usize];
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
        return Ok(NativeResult {
            body: data.to_vec(),
            dimensions: origin,
            origin,
            format: source_format,
        });
    }
    let animated = count > 1
        && matches!(
            format,
            OutputFormat::Gif | OutputFormat::Webp | OutputFormat::Tiff
        );
    let mut decoded_plan = None;
    let mut decoded_request = None;
    let input = {
        // Scope the header owner here: a reduced loader must not leave a second
        // source graph alive through encoding.
        let head = head;
        if animated {
            load_with_owner(
                data,
                count,
                r.extra.enforce_thumbnail,
                Some(&encoded),
                Access::Random,
            )?
        } else if let Some(loaded) =
            super::scaled_decode::try_load(&encoded, &head, r, source_format)?
        {
            decoded_plan = Some(loaded.plan);
            decoded_request = Some(loaded.request);
            loaded.image
        } else if source_format == OutputFormat::Jpeg {
            if let Some((image, plan)) = super::jpeg_decode::try_load(&encoded, &head, r)? {
                decoded_plan = Some(plan);
                image
            } else if let Some(image) = super::jpeg_decode::crop_input(&encoded, &head, r)? {
                image
            } else {
                head
            }
        } else if super::jpeg_decode::source_access(Some(source_format), &head, r)
            == Access::Sequential
        {
            load_with_owner(
                data,
                1,
                r.extra.enforce_thumbnail,
                Some(&encoded),
                Access::Sequential,
            )?
        } else if source_format == OutputFormat::Png
            && head.get_typeof("icc-profile-data").unwrap_or(0) == 0
            && (!r.auto_rotate || head.get_int("orientation").unwrap_or(1) == 1)
        {
            if let Some((image, plan)) = super::png_decode::try_load(data, r)? {
                decoded_plan = plan;
                image
            } else {
                head
            }
        } else {
            head
        }
    };
    let input = super::svg_alpha::prepare(input, r, source_format)?;
    // Lazy source graphs retain their own Arc; independent rasters can now
    // release the outer encoded allocation before filtering and encoding.
    drop(encoded);
    let count = if animated { count } else { 1 };
    let mut frames = Vec::new();
    let mut sum = 0usize;
    for index in 0..count {
        let frame = if animated {
            input
                .extract_area(
                    0,
                    index as i32 * frame_height,
                    input.get_width(),
                    frame_height,
                )
                .map_err(|_| "native frame extraction failed")?
        } else {
            input.clone()
        };
        let frame = prepare(frame, r)?;
        let (frame, dpr) = geometry::apply(
            frame,
            decoded_request.as_ref().unwrap_or(r),
            source_format == OutputFormat::Svg,
            decoded_plan.take(),
        )?;
        let frame = super::filters::effects(frame, r)?;
        let frame = geometry::decorate(frame, r, dpr)?;
        let frame = if format == OutputFormat::Jpeg {
            geometry::background(
                frame,
                r.options.background.unwrap_or([255; 4]),
                r.extra.preserve_hdr,
            )?
        } else {
            frame
        };
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
    drop(input);
    drop(frames);
    let bytes = encode::save(image, r, format, height as i32)?;
    Ok(NativeResult {
        body: bytes,
        dimensions: [width, height],
        origin,
        format,
    })
}

#[cfg(test)]
#[path = "scaled_pipeline_tests.rs"]
mod scaled_pipeline_tests;

#[cfg(test)]
#[path = "bmp_load_tests.rs"]
mod bmp_load_tests;

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[test]
    fn failed_native_request_clears_the_codec_thread_error_state() {
        initialize().unwrap();
        Vips::error_clear();
        let r = crate::request::parse(
            "/unsafe/plain/local:///broken.jpg@jpg",
            &crate::request::Config::unsafe_local(),
        )
        .unwrap();
        assert!(process(vec![0xff, 0xd8, 0, 0], &r, &[]).is_err());
        assert!(Vips::error_buffer().unwrap().is_empty());
    }
}
