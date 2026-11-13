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
    let expected = (image.get_width() as usize)
        .checked_mul(image.get_height() as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("native raster overflow")?;
    let mut size = 0usize;
    // SAFETY: image remains live; libvips sets size and returns an owned GLib
    // allocation or NULL. Validate both before reading and always use g_free.
    let raw =
        unsafe { libvips::bindings::vips_image_write_to_memory(image.as_mut_ptr(), &mut size) };
    if raw.is_null() {
        return Err("native raster export failed");
    }
    struct Memory(*mut std::ffi::c_void);
    impl Drop for Memory {
        fn drop(&mut self) {
            unsafe { libvips::bindings::g_free(self.0) };
        }
    }
    let allocation = Memory(raw);
    if size != expected {
        return Err("inconsistent native RGBA raster");
    }
    let mut pixels = Buffer::new(size)?;
    // SAFETY: a non-NULL allocation of the validated size is live and disjoint
    // from the new Rust buffer. Both contain exactly width*height RGBA8 bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(
            allocation.0.cast(),
            pixels.as_mut_slice().as_mut_ptr(),
            size,
        );
    }
    Ok(Image {
        width: image.get_width() as usize,
        height: image.get_height() as usize,
        pixels,
    })
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
    if matches!(format, OutputFormat::Bmp | OutputFormat::Ico) {
        let raster = raster(image)?;
        let b = if format == OutputFormat::Bmp {
            crate::image::encode_bmp(&raster)?
        } else {
            crate::extra_encode::encode_ico(&raster)?
        };
        return Ok(b.as_slice().to_vec());
    }
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
            | OutputFormat::Png
    ) {
        options = options.set("Q", i32::from(quality));
    }
    if format == OutputFormat::Webp {
        options = options.set("lossless", quality == 100).set("effort", 4);
    }
    if format == OutputFormat::Avif {
        options = options
            .set(
                "compression",
                libvips::enums::ForeignHeifCompression::Av1 as i32,
            )
            .set("effort", 1);
    }
    if format == OutputFormat::Heic {
        options = options.set(
            "compression",
            libvips::enums::ForeignHeifCompression::Hevc as i32,
        );
    }
    if format == OutputFormat::Png {
        let palette = std::env::var("PIXSTRIDE_PNG_QUANTIZE").as_deref() == Ok("1");
        options = options.set("palette", palette).set(
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
    let bytes = image
        .write_to_buffer_with_opts(&format!(".{}", format.extension()), options)
        .map_err(|_| "native encoder unavailable or rejected output")?;
    if format == OutputFormat::Png && std::env::var("PIXSTRIDE_PNG_QUANTIZE").as_deref() == Ok("1")
    {
        if r.extra.preserve_hdr {
            return Err("palette PNG cannot preserve high precision");
        }
        if bytes.get(25) != Some(&3) {
            return super::palette::encode(image, &bytes, quality);
        }
    }
    Ok(bytes)
}

pub(super) fn save(
    image: &VipsImage,
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
    let image = super::metadata::strip(image, r)?;
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
        .unwrap_or(80);
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
