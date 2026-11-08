//! Static GIF89a encoder, using Rust std and an in-process LZW dictionary.
//!
//! GIF has one transparent index: alpha < 128 is transparent, while other
//! pixels become opaque. With <= 256 available colors, RGB is preserved for
//! opaque pixels; otherwise a deterministic 6x7x6 RGB palette is used.
//! Animation, dithering, ICC conversion and continuous alpha are unsupported.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};

const HASH_SIZE: usize = 8192;
const PALETTE_PROBES: usize = 8;

pub(crate) struct Palette {
    colors: [u32; 256],
    keys: [u32; 1024],
    values: [u8; 1024],
    overflow: Vec<u8>,
    count: usize,
    transparent: bool,
    exact: bool,
}

impl Palette {
    fn slot(&self, rgb: u32) -> Option<usize> {
        let mut slot = (rgb.wrapping_mul(0x9e37_79b1) >> 22) as usize;
        let mut probes = 1;
        while self.keys[slot] != u32::MAX && self.keys[slot] != rgb {
            if probes == PALETTE_PROBES {
                return None;
            }
            slot = (slot + 1) & 1023;
            probes += 1;
        }
        Some(slot)
    }

    fn new(pixels: &[u8]) -> Result<Self, Error> {
        let transparent = pixels.as_chunks::<4>().0.iter().any(|p| p[3] < 128);
        let mut palette = Self {
            colors: [0; 256],
            keys: [u32::MAX; 1024],
            values: [0; 1024],
            overflow: Vec::new(),
            count: usize::from(transparent),
            transparent,
            exact: true,
        };
        for p in pixels.as_chunks::<4>().0 {
            if p[3] < 128 {
                continue;
            }
            let rgb = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
            let (slot, overflow) = if let Some(slot) = palette.slot(rgb) {
                if palette.keys[slot] == rgb {
                    continue;
                }
                (slot, false)
            } else {
                match palette
                    .overflow
                    .binary_search_by_key(&rgb, |&index| palette.colors[usize::from(index)])
                {
                    Ok(_) => continue,
                    Err(slot) => (slot, true),
                }
            };
            if palette.count == 256 {
                palette.exact = false;
                break;
            }
            if overflow {
                // The sorted fallback bounds long collision chains. Palette
                // colors still follow first appearance, exactly as before.
                if palette.overflow.capacity() == 0 {
                    palette
                        .overflow
                        .try_reserve_exact(256)
                        .map_err(|_| "GIF: allocation failed")?;
                }
                palette.overflow.insert(slot, palette.count as u8);
            } else {
                palette.keys[slot] = rgb;
                palette.values[slot] = palette.count as u8;
            }
            palette.colors[palette.count] = rgb;
            palette.count += 1;
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
        Ok(palette)
    }

    fn index(&self, pixel: &[u8]) -> u8 {
        if pixel[3] < 128 {
            return 0;
        }
        if self.exact {
            let rgb =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
            return match self.slot(rgb) {
                Some(slot) => self.values[slot],
                None => self
                    .overflow
                    .binary_search_by_key(&rgb, |&index| self.colors[usize::from(index)])
                    .map_or(0, |slot| self.overflow[slot]),
            };
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
    let palette = Palette::new(pixels)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(rgb: u32, alpha: u8) -> [u8; 4] {
        [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha]
    }

    #[test]
    fn exact_palette_keeps_order_for_colliding_colors() {
        // All of these keys shared one bucket in the former hash table.
        let colors: [u32; 12] = [
            10_946, 0, 610, 1597, 3194, 4181, 4791, 5778, 7375, 8362, 8972, 9959,
        ];
        let mut pixels = Vec::from(rgba(0x123456, 127));
        for &color in &colors {
            assert_eq!(color.wrapping_mul(0x9e37_79b1) >> 22, 0);
            pixels.extend_from_slice(&rgba(color, 128));
        }
        for &color in colors.iter().rev() {
            pixels.extend_from_slice(&rgba(color, 255));
        }
        let palette = Palette::new(&pixels).unwrap();
        assert!(palette.exact && palette.transparent);
        assert_eq!(palette.count, colors.len() + 1);
        assert_eq!(&palette.colors[1..=colors.len()], &colors);
        assert_eq!(palette.overflow.len(), colors.len() - PALETTE_PROBES);
        assert!(palette
            .overflow
            .windows(2)
            .all(|w| palette.colors[usize::from(w[0])] < palette.colors[usize::from(w[1])]));
        assert_eq!(palette.index(&rgba(0xabcdef, 127)), 0);
        for (index, &color) in colors.iter().enumerate() {
            assert_eq!(palette.index(&rgba(color, 128)), index as u8 + 1);
        }
    }

    #[test]
    fn palette_falls_back_only_after_its_exact_capacity() {
        let mut pixels = Vec::new();
        for color in (0..256).rev() {
            pixels.extend_from_slice(&rgba(color, 255));
        }
        let exact = Palette::new(&pixels).unwrap();
        assert!(exact.exact);
        assert!(exact.overflow.is_empty());
        assert_eq!(exact.overflow.capacity(), 0);
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            assert_eq!(exact.index(pixel), index as u8);
        }
        pixels.extend_from_slice(&rgba(256, 255));
        let quantized = Palette::new(&pixels).unwrap();
        assert!(!quantized.exact);
        assert_eq!(quantized.count, 252);
        assert_eq!(quantized.index(&rgba(0, 255)), 0);
        assert_eq!(quantized.index(&rgba(0xffffff, 255)), 251);

        let mut with_transparency = Vec::from(rgba(0x123456, 127));
        with_transparency.extend_from_slice(&pixels);
        let transparent = Palette::new(&with_transparency).unwrap();
        assert!(!transparent.exact && transparent.transparent);
        assert_eq!(transparent.count, 253);
        assert_eq!(transparent.index(&rgba(0x123456, 127)), 0);
        assert_eq!(transparent.index(&rgba(0, 255)), 1);
        assert_eq!(transparent.index(&rgba(0xffffff, 255)), 252);
    }

    #[test]
    fn encoded_bytes_keep_palette_and_transparency() {
        let mut pixels = Buffer::new(8).unwrap();
        pixels
            .as_mut_slice()
            .copy_from_slice(&[255, 0, 0, 128, 1, 2, 3, 127]);
        let image = Image {
            width: 2,
            height: 1,
            pixels,
        };
        let encoded = encode(&image).unwrap();
        assert_eq!(
            encoded.as_slice(),
            &[
                b'G', b'I', b'F', b'8', b'9', b'a', 2, 0, 1, 0, 0xf0, 0, 0, 0, 0, 0, 255, 0, 0,
                0x21, 0xf9, 4, 1, 0, 0, 0, 0, 0x2c, 0, 0, 0, 0, 2, 0, 1, 0, 0, 2, 2, 0x0c, 0x0a, 0,
                0x3b,
            ]
        );
    }
}
