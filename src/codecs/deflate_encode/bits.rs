use super::*;
impl Bits {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bits: 0,
            used: 0,
        }
    }

    #[inline]
    pub(crate) fn byte(&mut self, byte: u8) -> Result<(), Error> {
        if self.bytes.len() >= MAX_ZLIB {
            return Err("PNG: compressed output too large");
        }
        if self.bytes.len() == self.bytes.capacity() {
            let extra = self
                .bytes
                .capacity()
                .max(4096)
                .min(MAX_ZLIB - self.bytes.len());
            self.bytes
                .try_reserve_exact(extra)
                .map_err(|_| "PNG: allocation failed")?;
        }
        self.bytes.push(byte);
        Ok(())
    }

    #[inline]
    pub(crate) fn put(&mut self, value: u32, count: u8) -> Result<(), Error> {
        self.bits |= u64::from(value) << self.used;
        self.used += count;
        while self.used >= 8 {
            self.byte(self.bits as u8)?;
            self.bits >>= 8;
            self.used -= 8;
        }
        Ok(())
    }

    pub(crate) fn align(&mut self) -> Result<(), Error> {
        if self.used != 0 {
            self.byte(self.bits as u8)?;
            self.bits = 0;
            self.used = 0;
        }
        Ok(())
    }
}
#[inline]
pub(crate) fn reverse(code: u16, length: u8) -> u32 {
    u32::from(code.reverse_bits() >> (16 - length))
}
#[inline]
pub(crate) fn fixed_symbol(out: &mut Bits, symbol: u16) -> Result<(), Error> {
    let (code, length) = match symbol {
        0..=143 => (0x30 + symbol, 8),
        144..=255 => (0x190 + symbol - 144, 9),
        256..=279 => (symbol - 256, 7),
        280..=287 => (0xc0 + symbol - 280, 8),
        _ => return Err("DEFLATE: invalid symbol"),
    };
    out.put(reverse(code, length), length)
}
// The bounded length alphabet is a compile-time table. Distance codes follow
// pairs of powers of two. These select exactly the original largest base.
pub(crate) const fn length_codes() -> [u8; 259] {
    let mut out = [0; 259];
    let mut length = 3;
    while length <= 258 {
        let mut i = 0;
        while i + 1 < LENGTH_BASE.len() && LENGTH_BASE[i + 1] as usize <= length {
            i += 1;
        }
        out[length] = i as u8;
        length += 1;
    }
    out
}
#[inline(always)]
pub(crate) fn match_codes(length: usize, distance: usize) -> Result<(usize, usize), Error> {
    if !(3..=258).contains(&length) || !(1..=WINDOW).contains(&distance) {
        return Err("DEFLATE: invalid reference");
    }
    let d = distance - 1;
    let di = if d < 4 {
        d
    } else {
        let log = usize::BITS as usize - 1 - d.leading_zeros() as usize;
        2 * log + ((d >> (log - 1)) & 1)
    };
    Ok((LENGTH_CODES[length] as usize, di))
}
pub(crate) fn emit_match(out: &mut Bits, length: usize, distance: usize) -> Result<(), Error> {
    let (li, di) = match_codes(length, distance)?;
    fixed_symbol(out, 257 + li as u16)?;
    out.put(
        (length - usize::from(LENGTH_BASE[li])) as u32,
        LENGTH_EXTRA[li],
    )?;
    out.put(reverse(di as u16, 5), 5)?;
    out.put(
        (distance - usize::from(DIST_BASE[di])) as u32,
        DIST_EXTRA[di],
    )
}
#[inline]
pub(crate) fn hash(input: &[u8], pos: usize) -> usize {
    let key = (u32::from(input[pos]) << 16)
        | (u32::from(input[pos + 1]) << 8)
        | u32::from(input[pos + 2]);
    (key.wrapping_mul(0x1e35_a7bd) >> 16) as usize
}
// The first three bytes have already been checked by the caller. Comparing
// complete words is safe even when the two ranges overlap: both borrow the
// same immutable input and the caller bounds `max_len` by its remaining tail.
// Little-endian conversion makes trailing_zeros identify the earliest unequal
// byte on every architecture, without changing the chosen match or its ties.
#[inline]
pub(crate) fn match_length(input: &[u8], at: usize, pos: usize, max_len: usize) -> usize {
    debug_assert!(at < pos && max_len >= 3 && max_len <= input.len() - pos);
    let left = &input[at + 3..at + max_len];
    let right = &input[pos + 3..pos + max_len];
    let mut left_chunks = left.chunks_exact(8);
    let mut right_chunks = right.chunks_exact(8);
    let mut count = 3;
    for (a, b) in left_chunks.by_ref().zip(right_chunks.by_ref()) {
        let delta =
            u64::from_le_bytes(a.try_into().unwrap()) ^ u64::from_le_bytes(b.try_into().unwrap());
        if delta != 0 {
            return count + delta.trailing_zeros() as usize / 8;
        }
        count += 8;
    }
    for (a, b) in left_chunks.remainder().iter().zip(right_chunks.remainder()) {
        if a != b {
            break;
        }
        count += 1;
    }
    count
}
pub(crate) fn table(size: usize) -> Result<Vec<u32>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(size)
        .map_err(|_| "PNG: allocation failed")?;
    values.resize(size, EMPTY);
    Ok(values)
}
pub(crate) fn fixed(input: &[u8], stored_len: Option<usize>) -> Result<Option<Vec<u8>>, Error> {
    let mut out = Bits::new();
    out.byte(0x78)?;
    out.byte(0x01)?;
    out.put(0b011, 3)?; // BFINAL=1, BTYPE=01: fixed Huffman.
    let mut head = table(HASH_SIZE)?;
    let mut previous = table(WINDOW)?;
    let mut tokens = Some(Vec::<u32>::new());
    let mut pos = 0usize;
    while pos < input.len() {
        if let Some(limit) = stored_len {
            // Include space for finishing bits and Adler; abandon a fixed block
            // once it cannot offer a meaningful reduction over a stored block.
            if out.bytes.len().saturating_add(8) >= limit {
                return Ok(None);
            }
        }
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if pos + 2 < input.len() {
            let h = hash(input, pos);
            let mut candidate = head[h];
            let max_len = (input.len() - pos).min(258);
            let mut steps = 0;
            while candidate != EMPTY && steps < MAX_CHAIN {
                let at = candidate as usize;
                if at >= pos || pos - at > WINDOW {
                    break;
                }
                if input[at] == input[pos]
                    && input[at + 1] == input[pos + 1]
                    && input[at + 2] == input[pos + 2]
                {
                    let count = match_length(input, at, pos, max_len);
                    if count > best_len {
                        best_len = count;
                        best_dist = pos - at;
                        if count == max_len {
                            break;
                        }
                    }
                }
                let next = previous[at & (WINDOW - 1)];
                if next != EMPTY && next >= candidate {
                    break;
                }
                candidate = next;
                steps += 1;
            }
            previous[pos & (WINDOW - 1)] = head[h];
            head[h] = pos as u32;
        }
        if best_len >= 3 {
            emit_match(&mut out, best_len, best_dist)?;
            record_token(
                &mut tokens,
                0x8000_0000 | (((best_len - 3) as u32) << 15) | (best_dist - 1) as u32,
            );
            let end = pos + best_len;
            let mut inside = pos + 1;
            while inside < end && inside + 2 < input.len() {
                let h = hash(input, inside);
                previous[inside & (WINDOW - 1)] = head[h];
                head[h] = inside as u32;
                inside += 1;
            }
            pos = end;
        } else {
            fixed_symbol(&mut out, u16::from(input[pos]))?;
            record_token(&mut tokens, u32::from(input[pos]));
            pos += 1;
        }
    }
    fixed_symbol(&mut out, 256)?;
    out.align()?;
    let checksum = adler32(input);
    for byte in checksum.to_be_bytes() {
        out.byte(byte)?;
    }
    if stored_len.is_some_and(|n| out.bytes.len() >= n) {
        return Ok(None);
    }
    drop(head);
    drop(previous);
    if let Some(tokens) = tokens {
        // Dynamic coding is an optional size optimization: allocation failure
        // or a candidate too large for the output limit retains the fixed block.
        match dynamic(&tokens, out.bytes.len(), checksum) {
            Ok(Some(candidate)) => return Ok(Some(candidate)),
            Ok(None) | Err("PNG: allocation failed") | Err("PNG: compressed output too large") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(Some(out.bytes))
}
#[inline]
pub(crate) fn record_token(tokens: &mut Option<Vec<u32>>, token: u32) {
    if let Some(values) = tokens {
        if values.len() == MAX_TOKENS
            || (values.len() == values.capacity()
                && values
                    .try_reserve_exact(values.capacity().max(4096).min(MAX_TOKENS - values.len()))
                    .is_err())
        {
            *tokens = None;
            return;
        }
        values.push(token);
    }
}
/// Construct a deterministic, complete prefix tree capped at 15 bits. Symbols
/// are partitioned by weight while each child keeps enough remaining leaves
/// for the depth budget. This avoids an unbounded tree for skewed inputs.
pub(crate) fn huffman_lengths(frequencies: &[u64]) -> Option<[u8; 286]> {
    if frequencies.is_empty() || frequencies.len() > 286 {
        return None;
    }
    let mut symbols = [0usize; 286];
    let mut count = 0usize;
    for (symbol, &frequency) in frequencies.iter().enumerate() {
        if frequency != 0 {
            symbols[count] = symbol;
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let mut lengths = [0u8; 286];
    if count == 1 {
        lengths[symbols[0]] = 1;
        return Some(lengths);
    }
    symbols[..count].sort_unstable_by(|&a, &b| frequencies[b].cmp(&frequencies[a]).then(a.cmp(&b)));
    let mut stack = [(0usize, 0usize, 0u8); 572];
    let mut used = 1usize;
    stack[0] = (0, count, 0);
    while used != 0 {
        used -= 1;
        let (start, end, depth) = stack[used];
        let n = end - start;
        if n == 1 {
            lengths[symbols[start]] = depth.max(1);
            continue;
        }
        if depth >= 15 {
            return None;
        }
        let capacity = 1usize << (14 - depth);
        let min_left = n.saturating_sub(capacity).max(1);
        let max_left = (n - 1).min(capacity);
        if min_left > max_left {
            return None;
        }
        let total = symbols[start..end]
            .iter()
            .try_fold(0u64, |sum, &s| sum.checked_add(frequencies[s]))?;
        let mut left_weight = 0u64;
        let mut best = min_left;
        let mut best_distance = u64::MAX;
        for k in 1..=max_left {
            left_weight = left_weight.checked_add(frequencies[symbols[start + k - 1]])?;
            if k >= min_left {
                let distance = left_weight.abs_diff(total - left_weight);
                if distance < best_distance {
                    best_distance = distance;
                    best = k;
                }
            }
        }
        stack[used] = (start, start + best, depth + 1);
        stack[used + 1] = (start + best, end, depth + 1);
        used += 2;
    }
    Some(lengths)
}
pub(crate) fn huffman_codes(lengths: &[u8]) -> Option<[u32; 286]> {
    let mut counts = [0u16; 16];
    for &length in lengths {
        if length > 15 {
            return None;
        }
        if length != 0 {
            counts[usize::from(length)] += 1;
        }
    }
    let mut next = [0u16; 16];
    let mut code = 0u16;
    for bits in 1..=15 {
        code = (code + counts[bits - 1]) << 1;
        next[bits] = code;
    }
    let mut result = [0u32; 286];
    for (symbol, &length) in lengths.iter().enumerate() {
        if length != 0 {
            result[symbol] = reverse(next[usize::from(length)], length);
            next[usize::from(length)] += 1;
        }
    }
    Some(result)
}
