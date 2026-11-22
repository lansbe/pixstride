//! Decode static opaque GIFs as an index plane. The native crop and resize
//! can then ask for RGB lookup only over the part of that plane they consume.
use crate::{request::Request, Error};
use libvips::{
    enums::Interpretation,
    voption::{Setter, VOption},
    VipsImage,
};

#[path = "gif_lzw.rs"]
mod lzw;

fn plane(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| "GIF index allocation failed")?;
    bytes.resize(len, 0);
    Ok(bytes)
}

pub(super) fn try_load(
    data: &[u8],
    head: &VipsImage,
    r: &Request,
) -> Result<Option<VipsImage>, Error> {
    // The native header has already applied the request's byte/frame/pixel
    // limits. Composited frames, transparency and retained metadata use it
    // directly. The caller selects this import for JPEG output.
    if head.get_n_pages() > 1
        || head.get_bands() != 3
        || !r.strip_metadata
        || !r.strip_color_profile
        || r.extra.keep_copyright
        || r.extra.preserve_hdr
        || head.get_typeof("icc-profile-data").unwrap_or(0) != 0
        || head.get_int("orientation").unwrap_or(1) != 1
    {
        return Ok(None);
    }
    let mut options = gif::DecodeOptions::new();
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        (crate::MAX_FILE as u64).try_into().unwrap(),
    ));
    options.check_frame_consistency(true);
    options.skip_frame_decoding(true);
    let mut reader = options.read_info(data).map_err(|_| "invalid GIF header")?;
    let (width, height) = (reader.width(), reader.height());
    let count = usize::from(width) * usize::from(height);
    if (i32::from(width), i32::from(height)) != (head.get_width(), head.get_height()) {
        return Ok(None);
    }
    if count
        > r.extra
            .max_source_pixels
            .min(r.extra.max_frame_pixels)
            .min(crate::MAX_PIXELS)
    {
        return Err("GIF source pixel budget exceeded");
    }
    let global = reader.global_palette().map(<[u8]>::to_vec);
    let frame = reader
        .read_next_frame()
        .map_err(|_| "invalid GIF frame")?
        .ok_or("missing GIF image")?;
    if (frame.width, frame.height) != (width, height)
        || frame.left != 0
        || frame.top != 0
        || frame.transparent.is_some()
    {
        return Ok(None);
    }
    let palette = frame
        .palette
        .as_deref()
        .or(global.as_deref())
        .ok_or("missing GIF palette")?;
    let colors = palette.len() / 3;
    let lookup = VipsImage::new_from_memory_copy(
        palette,
        colors as i32,
        1,
        3,
        libvips::enums::BandFormat::Uchar,
    )
    .map_err(|_| "native GIF palette import failed")?;
    let (minimum, compressed) = frame.buffer.split_first().ok_or("missing GIF codes")?;
    let mut indices = plane(count)?;
    lzw::decode(compressed, &mut indices, *minimum)?;
    if colors < 256 && indices.iter().any(|&index| usize::from(index) >= colors) {
        return Err("GIF palette index out of bounds");
    }
    let interlaced = frame.interlaced;
    if reader
        .next_frame_info()
        .map_err(|_| "invalid GIF trailer")?
        .is_some()
    {
        return Ok(None);
    }
    drop(reader);
    if interlaced {
        let mut ordered = plane(count)?;
        let stride = usize::from(width);
        let mut rows = indices.chunks_exact(stride);
        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
            for y in (start..usize::from(height)).step_by(step) {
                ordered[y * stride..(y + 1) * stride]
                    .copy_from_slice(rows.next().ok_or("invalid GIF interlace")?);
            }
        }
        indices = ordered;
    }
    let image = VipsImage::new_from_owned_u8(indices, i32::from(width), i32::from(height), 1)
        .and_then(|image| image.maplut(&lookup))
        .and_then(|image| {
            image.copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        })
        .map_err(|_| "native GIF index import failed")?;
    Ok(Some(image))
}

#[cfg(test)]
#[path = "gif_decode_tests.rs"]
mod tests;
