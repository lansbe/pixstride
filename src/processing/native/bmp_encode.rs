//! Native BMP preserves straight RGBA samples, including hidden RGB channels.
use crate::Error;
use libvips::{
    enums::{BandFormat, Interpretation},
    VipsImage,
};

pub(super) fn save(image: &VipsImage) -> Result<Vec<u8>, Error> {
    if let Some(bytes) = super::bmp_gray::try_save(image)? {
        return Ok(bytes);
    }
    let image = image
        .colourspace(Interpretation::Srgb)
        .and_then(|i| i.cast(BandFormat::Uchar))
        .map_err(|_| "native BMP conversion failed")?;
    let (width, height) = (image.get_width() as usize, image.get_height() as usize);
    super::engine::checked(image.get_width(), image.get_height())?;
    let memory = super::raster::export(&image)?;
    if image.get_bands() == 3 {
        return crate::image::encode_bmp_rgb(width, height, &memory);
    }
    if image.get_bands() != 4 {
        return Err("unsupported native BMP bands");
    }
    let stride = width.checked_mul(4).ok_or("BMP row overflow")?;
    let raster = stride.checked_mul(height).ok_or("BMP size overflow")?;
    let length = raster
        .checked_add(54)
        .filter(|n| *n <= crate::MAX_FILE)
        .ok_or("native output exceeds limit")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "BMP allocation failed")?;
    bytes.resize(length, 0);
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&(length as u32).to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&(width as u32).to_le_bytes());
    bytes[22..26].copy_from_slice(&(height as u32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(raster as u32).to_le_bytes());
    for (input, output) in memory
        .chunks_exact(stride)
        .rev()
        .zip(bytes[54..].chunks_exact_mut(stride))
    {
        for (rgba, bgra) in input.chunks_exact(4).zip(output.chunks_exact_mut(4)) {
            bgra.copy_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
        }
    }
    Ok(bytes)
}
