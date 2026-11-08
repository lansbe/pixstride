//! Baseline four-component Adobe CMYK / YCCK JPEG decoding.
//!
//! ITU-T T.872 sections 6.5.3 and 7 define the APP14 transforms and channel
//! complements. RGB conversion here is a simple device-independent fallback,
//! not ICC color management; embedded ICC profiles are not applied. To avoid
//! guessing polarity, four-component data without an Adobe marker is refused.

use super::{
    block, parse_dht, parse_dqt, read_marker, read_u16, sample_plane, store_rgb, Buffer, Component,
    Entropy, Error, Huffman, Image, MAX_FILE, MAX_PIXELS,
};

/// Converts decoded Adobe samples into RGB without applying an ICC profile.
/// Transform 0 stores complemented C, M, Y, K. Transform 2 stores YCbCr
/// derived from non-complemented CMY and a complemented K sample.
pub(crate) fn adobe_to_rgb(samples: [u8; 4], transform: u8) -> Result<[u8; 3], Error> {
    let mut complements = [0u8; 3];
    match transform {
        0 => complements.copy_from_slice(&samples[..3]),
        2 => {
            let mut cmy = [0u8; 4];
            store_rgb(&mut cmy, 0, samples[0], samples[1], samples[2]);
            for c in 0..3 {
                complements[c] = 255 - cmy[c];
            }
        }
        _ => return Err("JPEG: unsupported Adobe CMYK transform"),
    }
    let key_complement = u32::from(samples[3]);
    let mut rgb = [0u8; 3];
    for c in 0..3 {
        rgb[c] = ((u32::from(complements[c]) * key_complement + 127) / 255) as u8;
    }
    Ok(rgb)
}

/// Interpolate a subsampled component. This shares the narrow 2:1 behavior
/// with the progressive decoder, which owns its own sample planes.
#[allow(clippy::too_many_arguments)]
pub(super) fn adobe_sample_plane(
    data: &[u8],
    offset: usize,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    component: Component,
    max_h: usize,
    max_v: usize,
    _transform: u8,
    _ci: usize,
) -> u8 {
    if component.h * 2 == max_h && (component.v == max_v || component.v * 2 == max_v) && width <= 2
    {
        // Narrow 4:2:2 and 4:2:0 planes are expanded by sample replication. This
        // matches the independent decoder on this boundary; other planes
        // retain the existing centered interpolation.
        let sx = (x * component.h / max_h).min(width - 1);
        let sy = (y * component.v / max_v).min(height - 1);
        data[offset + sy * width + sx]
    } else {
        sample_plane(data, offset, width, height, x, y, component, max_h, max_v)
    }
}

fn frame(
    data: &[u8],
    components: &mut [Component; 4],
) -> Result<(usize, usize, usize, usize), Error> {
    if data.len() != 18 || data[0] != 8 || data[5] != 4 {
        return Err("JPEG: invalid baseline CMYK frame");
    }
    let height = read_u16(data, 1)?;
    let width = read_u16(data, 3)?;
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .filter(|&n| n <= MAX_PIXELS)
            .is_none()
    {
        return Err("JPEG: dimensions out of bounds");
    }
    let mut max_h = 0;
    let mut max_v = 0;
    let mut blocks = 0usize;
    for i in 0..4 {
        let pos = 6 + i * 3;
        let id = data[pos];
        let h = usize::from(data[pos + 1] >> 4);
        let v = usize::from(data[pos + 1] & 15);
        let quant = usize::from(data[pos + 2]);
        if h == 0
            || h > 4
            || v == 0
            || v > 4
            || quant >= 4
            || components[..i].iter().any(|c| c.id == id)
        {
            return Err("JPEG: invalid CMYK components");
        }
        components[i] = Component {
            id,
            h,
            v,
            quant,
            ..Component::EMPTY
        };
        max_h = max_h.max(h);
        max_v = max_v.max(v);
        blocks += h * v;
    }
    if blocks > 10
        || components
            .iter()
            .any(|c| max_h % c.h != 0 || max_v % c.v != 0)
    {
        return Err("JPEG: unsupported CMYK sampling");
    }
    Ok((width, height, max_h, max_v))
}

fn scan(data: &[u8], components: &mut [Component; 4]) -> Result<[usize; 4], Error> {
    if data.len() != 12 || data[0] != 4 || data[9..12] != [0, 63, 0] {
        return Err("JPEG: invalid baseline CMYK scan");
    }
    let mut order = [0; 4];
    let mut used = [false; 4];
    for i in 0..4 {
        let id = data[1 + 2 * i];
        let selector = data[2 + 2 * i];
        let position = components
            .iter()
            .position(|c| c.id == id)
            .ok_or("JPEG: composante absente")?;
        if used[position] || selector >> 4 >= 4 || selector & 15 >= 4 {
            return Err("JPEG: invalid CMYK table or component");
        }
        used[position] = true;
        components[position].dc = usize::from(selector >> 4);
        components[position].ac = usize::from(selector & 15);
        order[i] = position;
    }
    Ok(order)
}

#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    pos: usize,
    width: usize,
    height: usize,
    max_h: usize,
    max_v: usize,
    components: &mut [Component; 4],
    order: &[usize; 4],
    quant: &[[u16; 64]; 4],
    quant_present: &[bool; 4],
    dc: &[Huffman; 4],
    ac: &[Huffman; 4],
    restart_interval: usize,
    transform: u8,
) -> Result<Image, Error> {
    for c in components.iter() {
        if !quant_present[c.quant] || !dc[c.dc].present || !ac[c.ac].present {
            return Err("JPEG: missing required table");
        }
    }
    let mut width_by_plane = [0usize; 4];
    let mut height_by_plane = [0usize; 4];
    let mut offset_by_plane = [0usize; 4];
    let mut plane_bytes = 0usize;
    for i in 0..4 {
        let c = components[i];
        width_by_plane[i] = (width * c.h).div_ceil(max_h);
        height_by_plane[i] = (height * c.v).div_ceil(max_v);
        offset_by_plane[i] = plane_bytes;
        if c.h != max_h || c.v != max_v {
            plane_bytes = plane_bytes
                .checked_add(width_by_plane[i] * height_by_plane[i])
                .filter(|&n| n <= MAX_FILE)
                .ok_or("JPEG: planes too large")?;
        }
    }
    let mut pixels = Buffer::new(width * height * 4)?;
    let mut planes = Buffer::new(plane_bytes)?;
    let target = pixels.as_mut_slice();
    let columns = width.div_ceil(max_h * 8);
    let rows = height.div_ceil(max_v * 8);
    let total = columns * rows;
    let mut bits = Entropy::new(data, pos);
    let mut restart = 0u8;
    let mut samples = [0u8; 64];
    for index in 0..total {
        let mx = index % columns;
        let my = index / columns;
        for &ci in order {
            let c = &mut components[ci];
            for by in 0..c.v {
                for bx in 0..c.h {
                    block(
                        &mut bits,
                        c,
                        &quant[c.quant],
                        &dc[c.dc],
                        &ac[c.ac],
                        &mut samples,
                        1,
                    )?;
                    let px = (mx * c.h + bx) * 8;
                    let py = (my * c.v + by) * 8;
                    if px >= width_by_plane[ci] || py >= height_by_plane[ci] {
                        continue;
                    }
                    let cols = 8.min(width_by_plane[ci] - px);
                    let rows = 8.min(height_by_plane[ci] - py);
                    for y in 0..rows {
                        let source = &samples[y * 8..y * 8 + cols];
                        if c.h == max_h && c.v == max_v {
                            let at = ((py + y) * width + px) * 4;
                            for (pixel, &value) in target[at..at + cols * 4]
                                .as_chunks_mut::<4>()
                                .0
                                .iter_mut()
                                .zip(source)
                            {
                                pixel[ci] = value;
                            }
                        } else {
                            let at = offset_by_plane[ci] + (py + y) * width_by_plane[ci] + px;
                            planes.as_mut_slice()[at..at + cols].copy_from_slice(source);
                        }
                    }
                }
            }
        }
        if restart_interval != 0 && (index + 1) % restart_interval == 0 && index + 1 < total {
            bits.marker(0xd0 + restart)?;
            restart = (restart + 1) % 8;
            for c in components.iter_mut() {
                c.predictor = 0;
            }
        }
    }
    bits.marker(0xd9)?;
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let mut values = [0u8; 4];
            for ci in 0..4 {
                let c = components[ci];
                values[ci] = if c.h == max_h && c.v == max_v {
                    target[at + ci]
                } else {
                    adobe_sample_plane(
                        planes.as_slice(),
                        offset_by_plane[ci],
                        width_by_plane[ci],
                        height_by_plane[ci],
                        x,
                        y,
                        c,
                        max_h,
                        max_v,
                        transform,
                        ci,
                    )
                };
            }
            let rgb = adobe_to_rgb(values, transform)?;
            target[at..at + 3].copy_from_slice(&rgb);
            target[at + 3] = 255;
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Decode a complete four-component baseline JPEG with Adobe APP14 metadata.
/// ICC profiles are not applied; the RGB result is a simple CMYK fallback.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() > MAX_FILE {
        return Err("JPEG: file too large");
    }
    if !data.starts_with(&[0xff, 0xd8]) {
        return Err("JPEG: invalid signature");
    }
    let mut pos = 2usize;
    let mut quant = [[0u16; 64]; 4];
    let mut quant_present = [false; 4];
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut components = [Component::EMPTY; 4];
    let mut dimensions = None;
    let mut restart_interval = 0usize;
    let mut adobe_transform = None;
    loop {
        let marker = read_marker(data, &mut pos)?;
        if marker == 0xd9 {
            return Err("JPEG: no decoded image");
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            return Err("JPEG: misplaced marker");
        }
        let length = read_u16(data, pos)?;
        if length < 2 {
            return Err("JPEG: invalid segment length");
        }
        let end = pos
            .checked_add(length)
            .filter(|&end| end <= data.len())
            .ok_or("JPEG: truncated segment")?;
        let payload = &data[pos + 2..end];
        pos = end;
        match marker {
            0xdb => parse_dqt(payload, &mut quant, &mut quant_present)?,
            0xc4 => parse_dht(payload, &mut dc, &mut ac)?,
            0xc0 => {
                if dimensions.is_some() {
                    return Err("JPEG: multiple frames unsupported");
                }
                dimensions = Some(frame(payload, &mut components)?);
            }
            0xdd => {
                if payload.len() != 2 {
                    return Err("JPEG: invalid restart interval");
                }
                restart_interval = read_u16(payload, 0)?;
            }
            0xee if payload.starts_with(b"Adobe") => {
                if payload.len() < 12 || adobe_transform.is_some() {
                    return Err("JPEG: invalid or duplicate Adobe marker");
                }
                adobe_transform = Some(payload[11]);
            }
            0xda => {
                let (width, height, max_h, max_v) = dimensions.ok_or("JPEG: missing frame")?;
                let transform =
                    adobe_transform.ok_or("JPEG: ambiguous CMYK without Adobe marker")?;
                if transform != 0 && transform != 2 {
                    return Err("JPEG: unsupported Adobe CMYK transform");
                }
                let order = scan(payload, &mut components)?;
                return decode_scan(
                    data,
                    pos,
                    width,
                    height,
                    max_h,
                    max_v,
                    &mut components,
                    &order,
                    &quant,
                    &quant_present,
                    &dc,
                    &ac,
                    restart_interval,
                    transform,
                );
            }
            0xc1..=0xcf if marker != 0xc4 && marker != 0xc8 && marker != 0xcc => {
                return Err("JPEG: only baseline CMYK is supported");
            }
            0xcc | 0xc8 => return Err("JPEG: arithmetic coding unsupported"),
            0xe0..=0xef | 0xfe => {}
            _ => return Err("JPEG: unsupported segment"),
        }
    }
}
