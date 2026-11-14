//! RGBA palette fallback when the installed libvips lacks a quantizer.
use crate::{Error, MAX_FILE};
use libvips::VipsImage;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) -> Result<(), Error> {
    let size = bytes.len().checked_add(12).ok_or("palette PNG overflow")?;
    if size > MAX_FILE.saturating_sub(out.len()) {
        return Err("palette PNG exceeds output limit");
    }
    out.try_reserve(size)
        .map_err(|_| "palette PNG allocation failed")?;
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(bytes);
    let crc = crate::png::crc32(&out[out.len() - bytes.len() - 4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    Ok(())
}

pub(super) fn encode(
    image: &VipsImage,
    original: &[u8],
    quality: u8,
    colors: usize,
    interlace: bool,
) -> Result<Vec<u8>, Error> {
    let raster = super::encode::raster(image)?;
    let pixels = raster.pixels.as_slice();
    let speed = ((101 - i32::from(quality)) / 4).clamp(1, 30);
    let mut exact = std::collections::BTreeMap::<[u8; 4], u8>::new();
    let mut palette = Vec::new();
    for pixel in pixels.chunks_exact(4) {
        let color: [u8; 4] = pixel.try_into().unwrap();
        if !exact.contains_key(&color) {
            if exact.len() == colors {
                exact.clear();
                break;
            }
            exact.insert(color, exact.len() as u8);
            palette.extend_from_slice(pixel);
        }
    }
    let quantizer = if exact.is_empty() {
        Some(color_quant::NeuQuant::new(speed, colors, pixels))
    } else {
        None
    };
    if let Some(q) = &quantizer {
        palette = q.color_map_rgba();
    }
    let mut rgb = Vec::with_capacity(768);
    let mut alpha = Vec::with_capacity(256);
    for color in palette.chunks_exact(4) {
        rgb.extend_from_slice(&color[..3]);
        alpha.push(color[3]);
    }
    let mut raw = Vec::new();
    let length = (raster.width + 7)
        .checked_mul(raster.height)
        .ok_or("palette PNG overflow")?;
    if length > MAX_FILE {
        return Err("palette PNG raster exceeds limit");
    }
    raw.try_reserve_exact(length)
        .map_err(|_| "palette PNG allocation failed")?;
    let passes: &[(usize, usize, usize, usize)] = if interlace {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        &[(0, 0, 1, 1)]
    };
    for &(x0, y0, dx, dy) in passes {
        if x0 >= raster.width {
            continue;
        }
        for y in (y0..raster.height).step_by(dy) {
            raw.push(0);
            for x in (x0..raster.width).step_by(dx) {
                let at = (y * raster.width + x) * 4;
                let pixel = &pixels[at..at + 4];
                raw.push(if let Some(q) = &quantizer {
                    q.index_of(pixel) as u8
                } else {
                    exact[<&[u8; 4]>::try_from(pixel).unwrap()]
                });
            }
        }
    }
    let compressed = crate::deflate_encode::compress(&raw)?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&(raster.width as u32).to_be_bytes());
    header[4..8].copy_from_slice(&(raster.height as u32).to_be_bytes());
    header[8] = 8;
    header[9] = 3;
    header[12] = u8::from(interlace);
    chunk(&mut out, b"IHDR", &header)?;
    // Carry the already-filtered ancillary metadata from the native saver;
    // sample/colour-layout-specific chunks do not apply to indexed pixels.
    let mut at = 8usize;
    while at + 12 <= original.len() {
        let size = u32::from_be_bytes(original[at..at + 4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .filter(|&n| n <= original.len())
            .ok_or("invalid native PNG framing")?;
        let kind: &[u8; 4] = original[at + 4..at + 8].try_into().unwrap();
        if kind[0].is_ascii_lowercase() && !matches!(kind, b"tRNS" | b"bKGD" | b"sBIT" | b"hIST") {
            chunk(&mut out, kind, &original[at + 8..end - 4])?;
        }
        at = end;
    }
    chunk(&mut out, b"PLTE", &rgb)?;
    chunk(&mut out, b"tRNS", &alpha)?;
    chunk(&mut out, b"IDAT", &compressed)?;
    chunk(&mut out, b"IEND", &[])?;
    Ok(out)
}
