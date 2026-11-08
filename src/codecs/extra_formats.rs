//! Additional bounded, first-frame image decoders implemented in Rust.
//!
//! GIF, baseline strip-oriented TIFF, and PNG or DIB icons are decoded to the
//! shared RGBA buffer. Unsupported variants are rejected rather than silently
//! interpreted as a different image. GIF animation is not implemented.

use crate::{Buffer, Error, Image, MAX_FILE};

#[path = "extra_formats/gif.rs"]
mod gif;
#[path = "extra_formats/ico.rs"]
mod ico;
#[path = "extra_formats/lzw.rs"]
mod lzw;
#[path = "extra_formats/tiff.rs"]
mod tiff;

pub fn handles(data: &[u8]) -> bool {
    data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || data.starts_with(b"II\x2a\0")
        || data.starts_with(b"MM\0\x2a")
        || data.starts_with(b"\0\0\x01\0")
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() > MAX_FILE {
        return Err("image input exceeds limit");
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return gif::decode(data);
    }
    if data.starts_with(b"II\x2a\0") || data.starts_with(b"MM\0\x2a") {
        return tiff::decode(data);
    }
    if data.starts_with(b"\0\0\x01\0") {
        return ico::decode(data);
    }
    Err("unsupported extra image format")
}

fn section(data: &[u8], offset: usize, length: usize) -> Result<&[u8], Error> {
    let end = offset.checked_add(length).ok_or("image offset overflow")?;
    data.get(offset..end).ok_or("truncated image data")
}

fn le16(data: &[u8], offset: usize) -> Result<u16, Error> {
    let p = section(data, offset, 2)?;
    Ok(u16::from_le_bytes([p[0], p[1]]))
}

fn le32(data: &[u8], offset: usize) -> Result<u32, Error> {
    let p = section(data, offset, 4)?;
    Ok(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}

fn make_image(width: usize, height: usize) -> Result<Image, Error> {
    let bytes = crate::image::dimensions(width, height)?;
    if bytes > MAX_FILE {
        return Err("decoded image exceeds limit");
    }
    Ok(Image {
        width,
        height,
        pixels: Buffer::new(bytes)?,
    })
}
