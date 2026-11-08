use super::*;
pub(crate) fn decode_data(
    bits: &mut BitReader<'_>,
    width: usize,
    height: usize,
    spatial: bool,
) -> Result<Vec<u32>, Error> {
    let count = checked_pixels(width, height)?;
    let cache_bits = if bits.take(1)? == 0 {
        0
    } else {
        let n = bits.take(4)? as usize;
        if !(1..=11).contains(&n) {
            return Err("WebP: invalid color cache size");
        }
        n
    };
    let cache_size = if cache_bits == 0 {
        0
    } else {
        1usize << cache_bits
    };
    let mut cache = zeros::<u32>(cache_size)?;
    let mut meta_bits = 0usize;
    let mut meta_width = 0usize;
    let mut meta = Vec::new();
    let mut group_count = 1usize;
    if spatial && bits.take(1)? == 1 {
        meta_bits = 2 + bits.take(3)? as usize;
        meta_width = width.div_ceil(1usize << meta_bits);
        let meta_height = height.div_ceil(1usize << meta_bits);
        meta = decode_data(bits, meta_width, meta_height, false)?;
        group_count = meta
            .iter()
            .map(|p| ((p >> 8) & 0xffff) as usize)
            .max()
            .unwrap_or(0)
            + 1;
        // Bound worst-case allocations even when an attacker supplies sparse,
        // very large group indices. This is an explicit implementation limit.
        if group_count > 4096 {
            return Err("WebP: too many prefix groups");
        }
    }
    let mut groups = Vec::new();
    groups
        .try_reserve_exact(group_count)
        .map_err(|_| "WebP: allocation failed")?;
    for _ in 0..group_count {
        groups.push(CodeGroup {
            green: read_tree(bits, 280 + cache_size)?,
            red: read_tree(bits, 256)?,
            blue: read_tree(bits, 256)?,
            alpha: read_tree(bits, 256)?,
            distance: read_tree(bits, 40)?,
        });
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| "WebP: allocation failed")?;
    while out.len() < count {
        let pos = out.len();
        let group_id = if meta.is_empty() {
            0
        } else {
            let x = pos % width;
            let y = pos / width;
            ((meta[(y >> meta_bits) * meta_width + (x >> meta_bits)] >> 8) & 0xffff) as usize
        };
        let group = &groups[group_id];
        let symbol = group.green.read(bits)?;
        let (repeat, distance, value) = if symbol < 256 {
            let r = group.red.read(bits)? as u32;
            let b = group.blue.read(bits)? as u32;
            let a = group.alpha.read(bits)? as u32;
            (1, 0, (a << 24) | (r << 16) | ((symbol as u32) << 8) | b)
        } else if symbol < 280 {
            let repeat = prefix_value(bits, symbol - 256)?;
            let distance_prefix = group.distance.read(bits)?;
            let distance_code = prefix_value(bits, distance_prefix)?;
            let distance = if distance_code <= 120 {
                let (x, y) = PLANE[distance_code - 1];
                (i64::from(x) + i64::from(y) * width as i64).max(1) as usize
            } else {
                distance_code - 120
            };
            if distance > pos || repeat > count - pos {
                return Err("WebP: invalid LZ77 backward reference");
            }
            (repeat, distance, 0)
        } else {
            let index = symbol - 280;
            if index >= cache_size {
                return Err("WebP: invalid color cache index");
            }
            (1, 0, cache[index])
        };
        for _ in 0..repeat {
            let pixel = if distance == 0 {
                value
            } else {
                out[out.len() - distance]
            };
            out.push(pixel);
            if cache_bits != 0 {
                let index = (pixel.wrapping_mul(0x1e35_a7bd) >> (32 - cache_bits)) as usize;
                cache[index] = pixel;
            }
        }
    }
    Ok(out)
}
pub(crate) fn add_channels(a: u32, b: u32) -> u32 {
    let mut result = 0u32;
    for shift in [0, 8, 16, 24] {
        let value = ((a >> shift) as u8).wrapping_add((b >> shift) as u8);
        result |= u32::from(value) << shift;
    }
    result
}
#[inline(always)]
pub(crate) fn average(a: u32, b: u32) -> u32 {
    let mut result = 0u32;
    for shift in [0, 8, 16, 24] {
        let x = (a >> shift) & 255;
        let y = (b >> shift) & 255;
        result |= ((x + y) / 2) << shift;
    }
    result
}
#[inline(always)]
pub(crate) fn select(left: u32, top: u32, upper_left: u32) -> u32 {
    let mut dl = 0i32;
    let mut dt = 0i32;
    for shift in [0, 8, 16, 24] {
        let l = ((left >> shift) & 255) as i32;
        let t = ((top >> shift) & 255) as i32;
        let tl = ((upper_left >> shift) & 255) as i32;
        dl += (t - tl).abs();
        dt += (l - tl).abs();
    }
    if dl < dt {
        left
    } else {
        top
    }
}
#[inline(always)]
pub(crate) fn clamp_predict(left: u32, top: u32, upper_left: u32, half: bool) -> u32 {
    let mut result = 0u32;
    for shift in [0, 8, 16, 24] {
        let l = ((left >> shift) & 255) as i32;
        let t = ((top >> shift) & 255) as i32;
        let tl = ((upper_left >> shift) & 255) as i32;
        let value = if half {
            let a = (l + t) / 2;
            a + (a - tl) / 2
        } else {
            l + t - tl
        };
        result |= (value.clamp(0, 255) as u32) << shift;
    }
    result
}
pub(crate) fn predictor(mode: u32, l: u32, t: u32, tr: u32, tl: u32) -> Result<u32, Error> {
    Ok(match mode {
        0 => 0xff00_0000,
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => average(average(l, tr), t),
        6 => average(l, tl),
        7 => average(l, t),
        8 => average(tl, t),
        9 => average(t, tr),
        10 => average(average(l, tl), average(t, tr)),
        11 => select(l, t, tl),
        12 => clamp_predict(l, t, tl, false),
        13 => clamp_predict(l, t, tl, true),
        _ => return Err("WebP: unsupported predictor mode"),
    })
}
pub(crate) fn inverse_predictor(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    bits: usize,
    tiles: &[u32],
) -> Result<(), Error> {
    let tile_width = width.div_ceil(1usize << bits);
    for y in 0..height {
        for x in 0..width {
            let pos = y * width + x;
            let pred = if x == 0 && y == 0 {
                0xff00_0000
            } else if y == 0 {
                pixels[pos - 1]
            } else if x == 0 {
                pixels[pos - width]
            } else {
                let l = pixels[pos - 1];
                let t = pixels[pos - width];
                let tl = pixels[pos - width - 1];
                let tr = if x + 1 < width {
                    pixels[pos - width + 1]
                } else {
                    pixels[y * width]
                };
                let mode = (tiles[(y >> bits) * tile_width + (x >> bits)] >> 8) & 255;
                predictor(mode, l, t, tr, tl)?
            };
            pixels[pos] = add_channels(pixels[pos], pred);
        }
    }
    Ok(())
}
pub(crate) fn color_delta(multiplier: u8, channel: u8) -> i32 {
    (i32::from(multiplier as i8) * i32::from(channel as i8)) >> 5
}
pub(crate) fn inverse_color(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    bits: usize,
    tiles: &[u32],
) {
    let tile_width = width.div_ceil(1usize << bits);
    for y in 0..height {
        for x in 0..width {
            let pos = y * width + x;
            let pixel = pixels[pos];
            let tile = tiles[(y >> bits) * tile_width + (x >> bits)];
            let g = (pixel >> 8) as u8;
            let red = ((pixel >> 16) as u8).wrapping_add(color_delta(tile as u8, g) as u8);
            let blue = (pixel as u8)
                .wrapping_add(color_delta((tile >> 8) as u8, g) as u8)
                .wrapping_add(color_delta((tile >> 16) as u8, red) as u8);
            pixels[pos] = (pixel & 0xff00_ff00) | (u32::from(red) << 16) | u32::from(blue);
        }
    }
}
pub(crate) fn vp8l_payload(data: &[u8]) -> Result<Payload<'_>, Error> {
    if data.len() < 20 || data.len() > MAX_FILE || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP"
    {
        return Err("WebP: invalid RIFF header");
    }
    let declared = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let end = declared.checked_add(8).ok_or("WebP: RIFF overflow")?;
    if end < 20 || end > data.len() {
        return Err("WebP: truncated RIFF");
    }
    let mut pos = 12usize;
    let mut vp8l = None;
    let mut canvas = None;
    while pos < end {
        if end - pos < 8 {
            return Err("WebP: truncated chunk header");
        }
        let kind = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let stop = start.checked_add(size).ok_or("WebP: chunk overflow")?;
        let next = stop.checked_add(size & 1).ok_or("WebP: chunk overflow")?;
        if next > end {
            return Err("WebP: truncated chunk");
        }
        if size & 1 != 0 && data[stop] != 0 {
            return Err("WebP: invalid RIFF padding");
        }
        match kind {
            b"VP8L" => {
                if vp8l.is_some() {
                    return Err("WebP: duplicate lossless image");
                }
                vp8l = Some(&data[start..stop]);
            }
            b"VP8 " | b"ANIM" | b"ANMF" | b"ALPH" => {
                return Err("WebP: lossy or animated data unsupported")
            }
            b"VP8X" => {
                if canvas.is_some() || size != 10 {
                    return Err("WebP: invalid extended header");
                }
                let v = &data[start..stop];
                if v[0] & 0b1100_0011 != 0 || v[1..4] != [0, 0, 0] {
                    return Err("WebP: invalid or animated extended header");
                }
                let width =
                    1 + usize::from(v[4]) + (usize::from(v[5]) << 8) + (usize::from(v[6]) << 16);
                let height =
                    1 + usize::from(v[7]) + (usize::from(v[8]) << 8) + (usize::from(v[9]) << 16);
                canvas = Some((width, height));
            }
            _ => {} // Metadata is not color-managed by this implementation.
        }
        pos = next;
    }
    Ok((vp8l.ok_or("WebP: no lossless image chunk")?, canvas))
}
