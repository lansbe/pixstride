//! Bounded complete BMP8 palettes and grayscale output without RGB expansion.
use crate::Error;
use libvips::{
    enums::{BandFormat, Interpretation},
    voption::{Setter, VOption},
    VipsImage,
};

const OFFSET: usize = 54 + 256 * 4;

struct View<'a> {
    width: usize,
    height: usize,
    top_down: bool,
    stride: usize,
    raster: &'a [u8],
    palette: [[u8; 3]; 256],
    gray: bool,
}

fn parse(data: &[u8]) -> Result<Option<View<'_>>, Error> {
    if !data.starts_with(b"BM") || data.get(28..30) != Some(b"\x08\0") {
        return Ok(None);
    }
    use crate::image::{le16, le32};
    if data.len() > crate::MAX_FILE
        || le32(data, 14)? != 40
        || le16(data, 26)? != 1
        || le32(data, 30)? != 0
        || !matches!(le32(data, 46)?, 0 | 256)
    {
        return Err("unsupported BMP8 header");
    }
    let width = le32(data, 18)? as i32;
    let height = le32(data, 22)? as i32;
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err("invalid BMP8 dimensions");
    }
    let top_down = height < 0;
    let (width, height) = (width as usize, height.unsigned_abs() as usize);
    crate::image::dimensions(width, height)?;
    let stride = width.checked_add(3).ok_or("BMP row overflow")? & !3;
    let offset = le32(data, 10)? as usize;
    let end = stride
        .checked_mul(height)
        .and_then(|n| n.checked_add(offset))
        .filter(|n| offset >= OFFSET && *n <= data.len())
        .ok_or("truncated BMP8 raster")?;
    let mut palette = [[0; 3]; 256];
    let mut gray = true;
    for (value, entry) in palette.iter_mut().zip(data[54..OFFSET].chunks_exact(4)) {
        // RGBQUAD's reserved byte is padding, not alpha.
        *value = [entry[2], entry[1], entry[0]];
        gray &= entry[0] == entry[1] && entry[1] == entry[2];
    }
    Ok(Some(View {
        width,
        height,
        top_down,
        stride,
        raster: &data[offset..end],
        palette,
        gray,
    }))
}

/// Header/layout validation without allocation, for per-request source admission.
pub(super) fn source_dimensions(data: &[u8]) -> Result<Option<[usize; 2]>, Error> {
    Ok(parse(data)?.map(|view| [view.width, view.height]))
}

pub(super) fn try_load(data: &[u8]) -> Result<Option<VipsImage>, Error> {
    let Some(view) = parse(data)? else {
        return Ok(None);
    };
    let bands = if view.gray { 1 } else { 3 };
    let size = view
        .width
        .checked_mul(view.height)
        .and_then(|n| n.checked_mul(bands))
        .filter(|n| *n <= crate::MAX_FILE)
        .ok_or("BMP8 raster exceeds limit")?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(size)
        .map_err(|_| "BMP8 allocation failed")?;
    pixels.resize(size, 0);
    let identity = view.gray
        && view
            .palette
            .iter()
            .enumerate()
            .all(|(index, value)| usize::from(value[0]) == index);
    for (row, output) in pixels.chunks_exact_mut(view.width * bands).enumerate() {
        let source_row = if view.top_down {
            row
        } else {
            view.height - 1 - row
        };
        let input = &view.raster[source_row * view.stride..source_row * view.stride + view.width];
        if identity {
            output.copy_from_slice(input);
        } else if view.gray {
            for (out, index) in output.iter_mut().zip(input) {
                *out = view.palette[usize::from(*index)][0];
            }
        } else {
            for (out, index) in output.chunks_exact_mut(3).zip(input) {
                out.copy_from_slice(&view.palette[usize::from(*index)]);
            }
        }
    }
    VipsImage::new_from_owned_u8(pixels, view.width as i32, view.height as i32, bands as i32)
        .and_then(|i| {
            i.copy_with_opts(VOption::new().set(
                "interpretation",
                if view.gray {
                    Interpretation::BW
                } else {
                    Interpretation::Srgb
                } as i32,
            ))
        })
        .map(Some)
        .map_err(|_| "native BMP8 import failed")
}

pub(super) fn try_save(image: &VipsImage) -> Result<Option<Vec<u8>>, Error> {
    if image.get_bands() != 1
        || image.get_format().ok() != Some(BandFormat::Uchar)
        || image.get_interpretation().ok() != Some(Interpretation::BW)
    {
        return Ok(None);
    }
    super::engine::checked(image.get_width(), image.get_height())?;
    let (width, height) = (image.get_width() as usize, image.get_height() as usize);
    let stride = width.checked_add(3).ok_or("BMP row overflow")? & !3;
    let raster_size = stride.checked_mul(height).ok_or("BMP size overflow")?;
    let length = OFFSET
        .checked_add(raster_size)
        .filter(|n| *n <= crate::MAX_FILE)
        .ok_or("native output exceeds limit")?;
    let rgb_length = width
        .checked_mul(3)
        .and_then(|n| n.checked_add(3))
        .map(|n| n & !3)
        .and_then(|n| n.checked_mul(height))
        .and_then(|n| n.checked_add(54))
        .ok_or("BMP size overflow")?;
    // The palette is worthwhile only when the complete BMP8 is no larger.
    // Keep small outputs and their requested byte-target successes unchanged.
    if length > rgb_length {
        return Ok(None);
    }
    let memory = super::raster::export(image)?;
    if memory.len() != width * height {
        return Err("inconsistent BMP gray raster");
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "BMP allocation failed")?;
    bytes.resize(length, 0);
    bytes[..2].copy_from_slice(b"BM");
    for (at, value) in [
        (2, length as u32),
        (10, OFFSET as u32),
        (14, 40),
        (18, width as u32),
        (22, height as u32),
        (34, raster_size as u32),
        (46, 256),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[26] = 1;
    bytes[28] = 8;
    for (index, entry) in bytes[54..OFFSET].chunks_exact_mut(4).enumerate() {
        entry.copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
    }
    for (input, output) in memory
        .chunks_exact(width)
        .rev()
        .zip(bytes[OFFSET..].chunks_exact_mut(stride))
    {
        output[..width].copy_from_slice(input);
    }
    Ok(Some(bytes))
}

#[cfg(test)]
#[path = "bmp_gray_tests.rs"]
mod tests;
