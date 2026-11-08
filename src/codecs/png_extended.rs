//! PNG sample depths and Adam7. The fast 8-bit, non-interlaced path lives in
//! `png`. Pixel output is unassociated RGBA8; 16-bit channels are truncated
//! to their high byte after comparisons with any full-depth tRNS key.
//! Color profiles are not applied; APNG is rejected rather than flattened.

use crate::{png, Buffer, Error, Image, MAX_FILE, MAX_PIXELS};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}
fn mul(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or("PNG: size overflow")
}
fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or("PNG: size overflow")
}
fn extent(size: usize, start: usize, step: usize) -> usize {
    if size <= start {
        0
    } else {
        1 + (size - 1 - start) / step
    }
}

fn pass_size(
    width: usize,
    height: usize,
    pass: (usize, usize, usize, usize),
    bits: usize,
) -> Result<(usize, usize, usize), Error> {
    let w = extent(width, pass.0, pass.2);
    let h = extent(height, pass.1, pass.3);
    if w == 0 || h == 0 {
        return Ok((w, h, 0));
    }
    let row = add(mul(w, bits)?, 7)? / 8;
    Ok((w, h, row))
}

fn sample(row: &[u8], index: usize, depth: u8) -> u16 {
    match depth {
        16 => be16(&row[index * 2..index * 2 + 2]),
        8 => u16::from(row[index]),
        _ => {
            let bit = index * usize::from(depth);
            let shift = 8 - usize::from(depth) - (bit % 8);
            u16::from((row[bit / 8] >> shift) & ((1u8 << depth) - 1))
        }
    }
}
fn to_u8(value: u16, depth: u8) -> u8 {
    match depth {
        16 => (value >> 8) as u8,
        8 => value as u8,
        _ => ((u32::from(value) * 255) / ((1u32 << depth) - 1)) as u8,
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

fn unfilter(
    raw: &mut [u8],
    offset: usize,
    row_len: usize,
    bpp: usize,
    previous: bool,
) -> Result<(), Error> {
    let filter = raw[offset - 1];
    if filter > 4 {
        return Err("PNG: invalid row filter");
    }
    let (before, after) = raw.split_at_mut(offset);
    let row = &mut after[..row_len];
    let prior = if previous {
        Some(&before[offset - row_len - 1..offset - 1])
    } else {
        None
    };
    for i in 0..row_len {
        let left = if i >= bpp { row[i - bpp] } else { 0 };
        let up = prior.map_or(0, |p| p[i]);
        let upper_left = if i >= bpp {
            prior.map_or(0, |p| p[i - bpp])
        } else {
            0
        };
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, upper_left),
            _ => unreachable!(),
        };
        row[i] = row[i].wrapping_add(predictor);
    }
    Ok(())
}

/// Decode a single, complete PNG into RGBA8, validating every chunk and the
/// entire compressed stream before exposing any pixels to the caller.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if data.len() > MAX_FILE || !data.starts_with(SIGNATURE) {
        return Err("PNG: invalid signature or file too large");
    }
    let (mut width, mut height, mut depth, mut color, mut channels, mut interlace) =
        (0usize, 0usize, 0u8, 0u8, 0usize, 0u8);
    let (mut ihdr, mut plte, mut trns, mut idat, mut idat_done, mut iend) =
        (false, false, false, false, false, false);
    let mut palette = [0u8; 768];
    let mut palette_alpha = [255u8; 256];
    let mut palette_count = 0usize;
    let mut transparent_gray: Option<u16> = None;
    let mut transparent_rgb: Option<[u16; 3]> = None;
    let mut compressed_len = 0usize;
    let mut offset = SIGNATURE.len();

    while offset < data.len() {
        if data.len() - offset < 12 {
            return Err("PNG: truncated chunk");
        }
        let len = be32(&data[offset..offset + 4]) as usize;
        if len > 0x7fff_ffff {
            return Err("PNG: chunk too large");
        }
        let end = add(offset, add(len, 12)?)?;
        if end > data.len() {
            return Err("PNG: truncated chunk");
        }
        let kind = &data[offset + 4..offset + 8];
        let body = &data[offset + 8..end - 4];
        if !kind.iter().all(|b| b.is_ascii_alphabetic()) {
            return Err("PNG: invalid chunk type");
        }
        if png::crc32(&data[offset + 4..end - 4]) != be32(&data[end - 4..end]) {
            return Err("PNG: invalid chunk CRC");
        }
        if !ihdr && kind != b"IHDR" {
            return Err("PNG: IHDR must be first");
        }
        if idat && kind != b"IDAT" {
            idat_done = true;
        }
        match kind {
            b"IHDR" => {
                if ihdr || len != 13 {
                    return Err("PNG: invalid IHDR");
                }
                ihdr = true;
                width = be32(&body[0..4]) as usize;
                height = be32(&body[4..8]) as usize;
                depth = body[8];
                color = body[9];
                interlace = body[12];
                channels = match (color, depth) {
                    (0, 1 | 2 | 4 | 8 | 16) => 1,
                    (2, 8 | 16) => 3,
                    (3, 1 | 2 | 4 | 8) => 1,
                    (4, 8 | 16) => 2,
                    (6, 8 | 16) => 4,
                    _ => return Err("PNG: illegal color and bit depth"),
                };
                if width == 0
                    || height == 0
                    || width > 100_000
                    || height > 100_000
                    || mul(width, height)? > MAX_PIXELS
                {
                    return Err("PNG: invalid dimensions");
                }
                if body[10] != 0 || body[11] != 0 || interlace > 1 {
                    return Err("PNG: unsupported compression, filter, or interlace");
                }
            }
            b"PLTE" => {
                if plte
                    || idat
                    || trns
                    || len == 0
                    || len > 768
                    || !len.is_multiple_of(3)
                    || color == 0
                    || color == 4
                {
                    return Err("PNG: invalid PLTE");
                }
                palette_count = len / 3;
                if color == 3 && palette_count > 1usize << depth {
                    return Err("PNG: palette exceeds bit depth");
                }
                palette[..len].copy_from_slice(body);
                plte = true;
            }
            b"tRNS" => {
                if trns || idat {
                    return Err("PNG: invalid tRNS ordering");
                }
                match color {
                    0 if len == 2 => {
                        // PNG requires decoders to ignore unused high bits.
                        let mask = if depth == 16 {
                            u16::MAX
                        } else {
                            (1u16 << depth) - 1
                        };
                        let gray = be16(body) & mask;
                        transparent_gray = Some(gray);
                    }
                    2 if len == 6 => {
                        let mask = if depth == 16 { u16::MAX } else { 0xff };
                        let rgb = [
                            be16(&body[0..2]) & mask,
                            be16(&body[2..4]) & mask,
                            be16(&body[4..6]) & mask,
                        ];
                        transparent_rgb = Some(rgb);
                    }
                    3 if plte && len <= palette_count => {
                        palette_alpha[..len].copy_from_slice(body);
                    }
                    _ => return Err("PNG: invalid tRNS"),
                }
                trns = true;
            }
            b"IDAT" => {
                if idat_done || (color == 3 && !plte) {
                    return Err("PNG: invalid IDAT ordering");
                }
                idat = true;
                compressed_len = add(compressed_len, len)?;
                if compressed_len > MAX_FILE {
                    return Err("PNG: compressed stream too large");
                }
            }
            b"IEND" => {
                if len != 0 || !idat || end != data.len() {
                    return Err("PNG: invalid IEND");
                }
                iend = true;
                break;
            }
            b"acTL" | b"fcTL" | b"fdAT" => return Err("PNG: animation unsupported"),
            _ if kind[0].is_ascii_uppercase() => return Err("PNG: unknown critical chunk"),
            _ => (),
        }
        offset = end;
    }
    if !iend || compressed_len < 6 {
        return Err("PNG: incomplete image");
    }

    let bits_per_pixel = mul(channels, usize::from(depth))?;
    let noninterlaced = [(0usize, 0usize, 1usize, 1usize)];
    let passes: &[(usize, usize, usize, usize)] = if interlace == 0 {
        &noninterlaced
    } else {
        &ADAM7
    };
    let mut raw_len = 0usize;
    for &pass in passes {
        let (w, h, row) = pass_size(width, height, pass, bits_per_pixel)?;
        if w > 0 && h > 0 {
            raw_len = add(raw_len, mul(add(row, 1)?, h)?)?;
        }
    }
    let pixel_len = mul(mul(width, height)?, 4)?;
    if raw_len > MAX_FILE || pixel_len > MAX_FILE {
        return Err("PNG: decompressed image too large");
    }

    let mut compressed = Buffer::new(compressed_len)?;
    let mut written = 0usize;
    offset = SIGNATURE.len();
    while offset < data.len() {
        let len = be32(&data[offset..offset + 4]) as usize;
        let end = offset + len + 12;
        if &data[offset + 4..offset + 8] == b"IDAT" {
            compressed.as_mut_slice()[written..written + len]
                .copy_from_slice(&data[offset + 8..end - 4]);
            written += len;
        }
        offset = end;
    }
    if written != compressed_len {
        return Err("PNG: IDAT length mismatch");
    }
    let mut raw = Buffer::new(raw_len)?;
    png::inflate(compressed.as_slice(), raw.as_mut_slice())?;
    drop(compressed);
    let mut pixels = Buffer::new(pixel_len)?;
    let bpp = bits_per_pixel.div_ceil(8).max(1);
    offset = 0;
    for &pass in passes {
        let (w, h, row_len) = pass_size(width, height, pass, bits_per_pixel)?;
        if w == 0 || h == 0 {
            continue;
        }
        for py in 0..h {
            let row_start = add(offset, 1)?;
            unfilter(raw.as_mut_slice(), row_start, row_len, bpp, py > 0)?;
            let row = &raw.as_slice()[row_start..row_start + row_len];
            for px in 0..w {
                let i = px * channels;
                let rgba = match color {
                    0 => {
                        let g = sample(row, i, depth);
                        let c = to_u8(g, depth);
                        [c, c, c, if transparent_gray == Some(g) { 0 } else { 255 }]
                    }
                    2 => {
                        let rgb = [
                            sample(row, i, depth),
                            sample(row, i + 1, depth),
                            sample(row, i + 2, depth),
                        ];
                        [
                            to_u8(rgb[0], depth),
                            to_u8(rgb[1], depth),
                            to_u8(rgb[2], depth),
                            if transparent_rgb == Some(rgb) { 0 } else { 255 },
                        ]
                    }
                    3 => {
                        let k = sample(row, i, depth) as usize;
                        if k >= palette_count {
                            return Err("PNG: palette index out of range");
                        }
                        [
                            palette[3 * k],
                            palette[3 * k + 1],
                            palette[3 * k + 2],
                            palette_alpha[k],
                        ]
                    }
                    4 => {
                        let g = to_u8(sample(row, i, depth), depth);
                        [g, g, g, to_u8(sample(row, i + 1, depth), depth)]
                    }
                    6 => [
                        to_u8(sample(row, i, depth), depth),
                        to_u8(sample(row, i + 1, depth), depth),
                        to_u8(sample(row, i + 2, depth), depth),
                        to_u8(sample(row, i + 3, depth), depth),
                    ],
                    _ => return Err("PNG: invalid color"),
                };
                let x = pass.0 + px * pass.2;
                let y = pass.1 + py * pass.3;
                let dest = (y * width + x) * 4;
                pixels.as_mut_slice()[dest..dest + 4].copy_from_slice(&rgba);
            }
            offset = row_start + row_len;
        }
    }
    if offset != raw_len {
        return Err("PNG: decompressed length mismatch");
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
