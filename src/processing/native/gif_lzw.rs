//! GIF LZW stores each dictionary phrase as the preceding decoded phrase
//! followed by the first byte of the next one. Its start and end therefore
//! already exist in the output. Reuse those spans without walking prefix chains.
use crate::Error;

pub(super) fn decode(input: &[u8], output: &mut [u8], minimum: u8) -> Result<(), Error> {
    if !(2..=8).contains(&minimum) || output.len() > u32::MAX as usize {
        return Err("invalid GIF LZW dimensions");
    }
    let clear = 1u16 << minimum;
    let end = clear + 1;
    let mut offsets = [0u32; 4097];
    let mut written = 0;
    let mut bit = 0;
    // 'next' is the next dictionary code available before reading this phrase.
    // The first phrase after a reset supplies only an offset, no entry.
    let mut next = end;
    let mut width = minimum + 1;
    loop {
        let bytes = &input[bit / 8..];
        let word = if let Some(eight) = bytes.first_chunk::<8>() {
            u64::from_le_bytes(*eight)
        } else {
            let mut tail = [0; 8];
            tail[..bytes.len()].copy_from_slice(bytes);
            u64::from_le_bytes(tail)
        } >> (bit % 8);
        let mask = (1u64 << width) - 1;
        let code = (word & mask) as u16;
        let width_limit = 1u16 << width;

        // Four literals need neither dictionary copies nor control handling.
        // Leave width transitions, the full dictionary, and short ends to the
        // scalar path. Literal runs are common even inside compressed images.
        if bytes.len() >= 8 && written + 4 <= output.len() && next + 4 < width_limit {
            let b = ((word >> width) & mask) as u16;
            let c = ((word >> (2 * width)) & mask) as u16;
            let d = ((word >> (3 * width)) & mask) as u16;
            if (code | b | c | d) < clear {
                output[written..written + 4]
                    .copy_from_slice(&[code as u8, b as u8, c as u8, d as u8]);
                let start = written as u32;
                let entry = usize::from(next) + 1;
                offsets[entry..entry + 4].copy_from_slice(&[
                    start,
                    start + 1,
                    start + 2,
                    start + 3,
                ]);
                written += 4;
                next += 4;
                bit += 4 * usize::from(width);
                continue;
            }
        }
        bit += usize::from(width);
        if bit > input.len() * 8 {
            return Err("truncated GIF LZW stream");
        }
        if code == end {
            return if written == output.len() {
                Ok(())
            } else {
                Err("GIF LZW output length mismatch")
            };
        }
        if code == clear {
            next = end;
            width = minimum + 1;
            continue;
        }
        let start = written;
        if code < clear {
            *output
                .get_mut(written)
                .ok_or("GIF LZW output exceeds dimensions")? = code as u8;
            written += 1;
        } else {
            if code > next {
                return Err("invalid GIF LZW reference");
            }
            let index = usize::from(code);
            let from = offsets[index] as usize;
            // At the next unused code the suffix is the first byte of the
            // previous phrase. For an established code it follows that phrase.
            let to = if code == next {
                written
            } else {
                offsets[index + 1] as usize
            };
            let length = to - from;
            if written + length >= output.len() {
                return Err("GIF LZW output exceeds dimensions");
            }
            output.copy_within(from..to, written);
            written += length;
            output[written] = output[if code == next { from } else { to }];
            written += 1;
        }
        if next < 4096 {
            next += 1;
            offsets[usize::from(next)] = start as u32;
            if next == width_limit && width < 12 {
                width += 1;
            }
        }
    }
}
