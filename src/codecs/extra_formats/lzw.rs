//! Bounded LZW reader used by GIF and TIFF. GIF packs codes least-significant
//! bit first; TIFF packs most-significant first and advances widths one code
//! earlier. Both formats cap the dictionary at 4096 entries.

use crate::Error;

#[derive(Clone, Copy)]
pub(super) enum Order {
    Least,
    Most,
}

pub(crate) struct Bits<'a> {
    input: &'a [u8],
    at: usize,
    bits: u32,
    count: u8,
    order: Order,
}

impl Bits<'_> {
    fn read(&mut self, width: u8) -> Result<u16, Error> {
        while self.count < width {
            let byte = *self.input.get(self.at).ok_or("truncated LZW stream")? as u32;
            self.at += 1;
            match self.order {
                Order::Least => self.bits |= byte << self.count,
                Order::Most => self.bits = (self.bits << 8) | byte,
            }
            self.count += 8;
        }
        let mask = (1u32 << width) - 1;
        let value = match self.order {
            Order::Least => {
                let value = self.bits & mask;
                self.bits >>= width;
                value
            }
            Order::Most => {
                let shift = self.count - width;
                let value = (self.bits >> shift) & mask;
                self.bits &= (1u32 << shift) - 1;
                value
            }
        };
        self.count -= width;
        Ok(value as u16)
    }
}

pub(super) fn decode(
    input: &[u8],
    output: &mut [u8],
    min_bits: u8,
    order: Order,
) -> Result<(), Error> {
    if !(2..=8).contains(&min_bits) {
        return Err("unsupported LZW code size");
    }
    let clear = 1u16 << min_bits;
    let end = clear + 1;
    let mut next = end + 1;
    let mut width = min_bits + 1;
    let mut previous: Option<u16> = None;
    let mut prefixes = [0u16; 4096];
    let mut suffixes = [0u8; 4096];
    let mut stack = [0u8; 4097];
    let mut out_at = 0usize;
    let mut bits = Bits {
        input,
        at: 0,
        bits: 0,
        count: 0,
        order,
    };
    let mut previous_first = 0u8;
    loop {
        let code = bits.read(width)?;
        if code == clear {
            next = end + 1;
            width = min_bits + 1;
            previous = None;
            continue;
        }
        if code == end {
            return if out_at == output.len() {
                Ok(())
            } else {
                Err("LZW output length mismatch")
            };
        }
        if code > next || (code == next && previous.is_none()) || code >= 4096 {
            return Err("invalid LZW code");
        }
        let mut len = 0usize;
        let mut current = if code == next {
            stack[len] = previous_first;
            len += 1;
            previous.ok_or("invalid LZW initial code")?
        } else {
            code
        };
        while current >= clear {
            if current < end + 1 || current >= next || len >= 4096 {
                return Err("invalid LZW dictionary reference");
            }
            stack[len] = suffixes[current as usize];
            len += 1;
            let parent = prefixes[current as usize];
            if parent >= current {
                return Err("cyclic LZW dictionary");
            }
            current = parent;
        }
        let first = current as u8;
        stack[len] = first;
        len += 1;
        let out_end = out_at.checked_add(len).ok_or("LZW size overflow")?;
        if out_end > output.len() {
            return Err("LZW output exceeds image size");
        }
        for i in 0..len {
            output[out_at + i] = stack[len - i - 1];
        }
        out_at = out_end;
        if let Some(old) = previous {
            if next < 4096 {
                prefixes[next as usize] = old;
                suffixes[next as usize] = first;
                next += 1;
                let early = matches!(order, Order::Most) as u16;
                if width < 12 && next == (1u16 << width) - early {
                    width += 1;
                }
            }
        }
        previous = Some(code);
        previous_first = first;
    }
}
