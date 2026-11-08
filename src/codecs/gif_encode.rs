//! Static GIF89a encoder, using Rust std and an in-process LZW dictionary.
//!
//! GIF has one transparent index: alpha < 128 is transparent, while other
//! pixels become opaque. With <= 256 available colors, RGB is preserved for
//! opaque pixels; otherwise a deterministic 6x7x6 RGB palette is used.
//! Animation, dithering, ICC conversion and continuous alpha are unsupported.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};

const HASH_SIZE: usize = 8192;

pub(crate) struct Palette {
    colors: [u32; 256],
    keys: [u32; 1024],
    values: [u8; 1024],
    count: usize,
    transparent: bool,
    exact: bool,
}

impl Palette {
    fn slot(&self, rgb: u32) -> usize {
        let mut slot = (rgb.wrapping_mul(0x9e37_79b1) >> 22) as usize;
        while self.keys[slot] != u32::MAX && self.keys[slot] != rgb {
            slot = (slot + 1) & 1023;
        }
        slot
    }

    fn new(pixels: &[u8]) -> Self {
        let transparent = pixels.as_chunks::<4>().0.iter().any(|p| p[3] < 128);
        let mut palette = Self {
            colors: [0; 256],
            keys: [u32::MAX; 1024],
            values: [0; 1024],
            count: usize::from(transparent),
            transparent,
            exact: true,
        };
        for p in pixels.as_chunks::<4>().0 {
            if p[3] < 128 {
                continue;
            }
            let rgb = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
            let slot = palette.slot(rgb);
            if palette.keys[slot] == u32::MAX {
                if palette.count == 256 {
                    palette.exact = false;
                    break;
                }
                palette.keys[slot] = rgb;
                palette.values[slot] = palette.count as u8;
                palette.colors[palette.count] = rgb;
                palette.count += 1;
            }
        }
        if !palette.exact {
            let offset = usize::from(transparent);
            for r in 0..6u32 {
                for g in 0..7u32 {
                    for b in 0..6u32 {
                        let index = offset + ((r * 7 + g) * 6 + b) as usize;
                        let red = (r * 255 + 2) / 5;
                        let green = (g * 255 + 3) / 6;
                        let blue = (b * 255 + 2) / 5;
                        palette.colors[index] = (red << 16) | (green << 8) | blue;
                    }
                }
            }
            palette.count = 252 + offset;
        }
        // Even an all-transparent image has a usable two-entry global table.
        palette.count = palette.count.max(1);
        palette
    }

    fn index(&self, pixel: &[u8]) -> u8 {
        if pixel[3] < 128 {
            return 0;
        }
        if self.exact {
            let rgb =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
            return self.values[self.slot(rgb)];
        }
        let r = (u32::from(pixel[0]) * 5 + 127) / 255;
        let g = (u32::from(pixel[1]) * 6 + 127) / 255;
        let b = (u32::from(pixel[2]) * 5 + 127) / 255;
        (usize::from(self.transparent) + ((r * 7 + g) * 6 + b) as usize) as u8
    }
}

pub(crate) struct Bits {
    bytes: Vec<u8>,
    pending: u32,
    used: u32,
}

impl Bits {
    fn new(capacity: usize) -> Result<Self, Error> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| "GIF: allocation failed")?;
        Ok(Self {
            bytes,
            pending: 0,
            used: 0,
        })
    }
    fn put(&mut self, value: u16, width: u32) {
        self.pending |= u32::from(value) << self.used;
        self.used += width;
        while self.used >= 8 {
            self.bytes.push(self.pending as u8);
            self.pending >>= 8;
            self.used -= 8;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.used != 0 {
            self.bytes.push(self.pending as u8);
        }
        self.bytes
    }
}

/// Encode the index stream using a bounded open-addressed (prefix, byte)
/// dictionary. Codes are 12-bit maximum and clear when the table fills.
fn lzw(pixels: &[u8], palette: &Palette, min_bits: u32) -> Result<Vec<u8>, Error> {
    let count = pixels.len() / 4;
    let cap = count
        .checked_mul(2)
        .and_then(|n| n.checked_add(1024))
        .ok_or("GIF: size overflow")?;
    if cap > MAX_FILE {
        return Err("GIF: output size limit");
    }
    let mut bits = Bits::new(cap)?;
    let mut keys = [0u32; HASH_SIZE];
    let mut values = [0u16; HASH_SIZE];
    let clear = 1u16 << min_bits;
    let end = clear + 1;
    let mut next = end + 1;
    let mut width = min_bits + 1;
    bits.put(clear, width);
    let mut iter = pixels.chunks_exact(4);
    let first = iter.next().ok_or("GIF: empty image")?;
    let mut prefix = u16::from(palette.index(first));
    for pixel in iter {
        let value = palette.index(pixel);
        let key = ((u32::from(prefix) << 8) | u32::from(value)) + 1;
        let mut slot = (key.wrapping_mul(0x9e37_79b1) >> 19) as usize;
        while keys[slot] != 0 && keys[slot] != key {
            slot = (slot + 1) & (HASH_SIZE - 1);
        }
        if keys[slot] == key {
            prefix = values[slot];
            continue;
        }
        bits.put(prefix, width);
        if next < 4096 {
            keys[slot] = key;
            values[slot] = next;
            next += 1;
            // The encoder's table is one entry ahead of the decoder until the
            // next emitted code; grow only after passing the current boundary.
            if width < 12 && u32::from(next) > (1 << width) {
                width += 1;
            }
        } else {
            bits.put(clear, width);
            keys.fill(0);
            next = end + 1;
            width = min_bits + 1;
        }
        prefix = u16::from(value);
    }
    bits.put(prefix, width);
    // The final data code lets the decoder insert its last dictionary entry
    // before it reads EOI, so the code width can grow at this boundary.
    if width < 12 && u32::from(next) == (1 << width) {
        width += 1;
    }
    bits.put(end, width);
    Ok(bits.finish())
}

/// Encode a single frame. Partial alpha and >256 colors are quantized as
/// described in the module documentation; transparent RGB is not preserved.
pub fn encode(image: &Image) -> Result<Buffer, Error> {
    if image.width == 0
        || image.height == 0
        || image.width > u16::MAX as usize
        || image.height > u16::MAX as usize
    {
        return Err("GIF: dimensions outside format limits");
    }
    let count = image
        .width
        .checked_mul(image.height)
        .ok_or("GIF: dimensions overflow")?;
    if count > MAX_PIXELS {
        return Err("GIF: too many pixels");
    }
    let expected = count.checked_mul(4).ok_or("GIF: size overflow")?;
    let pixels = image.pixels.as_slice();
    if pixels.len() != expected {
        return Err("GIF: RGBA buffer size mismatch");
    }
    let palette = Palette::new(pixels);
    let table_size = palette.count.max(2).next_power_of_two();
    let table_bits = table_size.trailing_zeros();
    let min_bits = table_bits.max(2);
    let compressed = lzw(pixels, &palette, min_bits)?;
    let blocks = compressed.len().div_ceil(255);
    let total = 13usize
        .checked_add(table_size * 3)
        .and_then(|n| n.checked_add(if palette.transparent { 8 } else { 0 }))
        .and_then(|n| n.checked_add(10 + 1 + compressed.len() + blocks + 1 + 1))
        .ok_or("GIF: size overflow")?;
    if total > MAX_FILE {
        return Err("GIF: output size limit");
    }
    let mut out = Buffer::new(total)?;
    let dst = out.as_mut_slice();
    let mut pos = 0usize;
    let mut write = |bytes: &[u8]| {
        dst[pos..pos + bytes.len()].copy_from_slice(bytes);
        pos += bytes.len();
    };
    write(b"GIF89a");
    write(&(image.width as u16).to_le_bytes());
    write(&(image.height as u16).to_le_bytes());
    write(&[0x80 | 0x70 | (table_bits as u8 - 1), 0, 0]);
    for &color in &palette.colors[..table_size] {
        write(&[(color >> 16) as u8, (color >> 8) as u8, color as u8]);
    }
    if palette.transparent {
        write(&[0x21, 0xf9, 4, 1, 0, 0, 0, 0]);
    }
    write(&[0x2c, 0, 0, 0, 0]);
    write(&(image.width as u16).to_le_bytes());
    write(&(image.height as u16).to_le_bytes());
    write(&[0]); // not interlaced, no local color table
    write(&[min_bits as u8]);
    for chunk in compressed.chunks(255) {
        write(&[chunk.len() as u8]);
        write(chunk);
    }
    write(&[0, 0x3b]); // image data terminator, GIF trailer
    debug_assert_eq!(pos, total);
    Ok(out)
}
