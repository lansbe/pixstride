use super::*;
/// Decode a still-image VP8L stream to RGBA. Lossy VP8 and animation are not
/// supported. Metadata such as ICC is not applied by this implementation.
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    let (payload, canvas) = vp8l_payload(data)?;
    if payload.first() != Some(&0x2f) {
        return Err("WebP: invalid VP8L signature");
    }
    let mut bits = BitReader::new(&payload[1..]);
    let width = bits.take(14)? as usize + 1;
    let height = bits.take(14)? as usize + 1;
    checked_pixels(width, height)?;
    if canvas.is_some_and(|c| c != (width, height)) {
        return Err("WebP: canvas dimensions mismatch");
    }
    let _alpha_hint = bits.take(1)?;
    if bits.take(3)? != 0 {
        return Err("WebP: unsupported VP8L version");
    }
    let mut current_width = width;
    let mut seen = 0u8;
    let mut transforms = Vec::new();
    transforms
        .try_reserve_exact(4)
        .map_err(|_| "WebP: allocation failed")?;
    while bits.take(1)? == 1 {
        let kind = bits.take(2)? as u8;
        if seen & (1 << kind) != 0 {
            return Err("WebP: duplicate transform");
        }
        seen |= 1 << kind;
        let transform = match kind {
            0 | 1 => {
                let size_bits = 2 + bits.take(3)? as usize;
                let tw = current_width.div_ceil(1usize << size_bits);
                let th = height.div_ceil(1usize << size_bits);
                let tiles = decode_data(&mut bits, tw, th, false)?;
                if kind == 0 {
                    Transform::Predictor {
                        width: current_width,
                        bits: size_bits,
                        tiles,
                    }
                } else {
                    Transform::Color {
                        width: current_width,
                        bits: size_bits,
                        tiles,
                    }
                }
            }
            2 => Transform::SubtractGreen,
            3 => {
                let count = 1 + bits.take(8)? as usize;
                let mut palette = decode_data(&mut bits, count, 1, false)?;
                for i in 1..palette.len() {
                    palette[i] = add_channels(palette[i], palette[i - 1]);
                }
                let size_bits = if count <= 2 {
                    3
                } else if count <= 4 {
                    2
                } else if count <= 16 {
                    1
                } else {
                    0
                };
                let transform = Transform::Index {
                    width: current_width,
                    bits: size_bits,
                    palette,
                };
                current_width = current_width.div_ceil(1usize << size_bits);
                transform
            }
            _ => unreachable!(),
        };
        transforms.push(transform);
    }
    let mut pixels = decode_data(&mut bits, current_width, height, true)?;
    while let Some(transform) = transforms.pop() {
        match transform {
            Transform::Predictor {
                width: transform_width,
                bits,
                tiles,
            } => {
                if current_width != transform_width {
                    return Err("WebP: predictor width mismatch");
                }
                inverse_predictor(&mut pixels, current_width, height, bits, &tiles)?;
            }
            Transform::Color {
                width: transform_width,
                bits,
                tiles,
            } => {
                if current_width != transform_width {
                    return Err("WebP: color width mismatch");
                }
                inverse_color(&mut pixels, current_width, height, bits, &tiles);
            }
            Transform::SubtractGreen => {
                for pixel in &mut pixels {
                    let g = (*pixel >> 8) as u8;
                    let r = ((*pixel >> 16) as u8).wrapping_add(g);
                    let b = (*pixel as u8).wrapping_add(g);
                    *pixel = (*pixel & 0xff00_ff00) | (u32::from(r) << 16) | u32::from(b);
                }
            }
            Transform::Index {
                width: original_width,
                bits: pack_bits,
                palette,
            } => {
                let mut expanded = zeros::<u32>(checked_pixels(original_width, height)?)?;
                let value_bits = 8 >> pack_bits;
                let mask = (1u32 << value_bits) - 1;
                for y in 0..height {
                    for x in 0..original_width {
                        let packed = (pixels[y * current_width + (x >> pack_bits)] >> 8) & 255;
                        let index = ((packed >> ((x & ((1 << pack_bits) - 1)) * value_bits)) & mask)
                            as usize;
                        expanded[y * original_width + x] = palette.get(index).copied().unwrap_or(0);
                    }
                }
                pixels = expanded;
                current_width = original_width;
            }
        }
    }
    if current_width != width || pixels.len() != width * height {
        return Err("WebP: decoded size mismatch");
    }
    let mut result = Buffer::new(pixels.len().checked_mul(4).ok_or("WebP: size overflow")?)?;
    for (out, &p) in result
        .as_mut_slice()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(&pixels)
    {
        out.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
    }
    Ok(Image {
        width,
        height,
        pixels: result,
    })
}
