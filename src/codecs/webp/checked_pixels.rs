use super::*;
pub(crate) fn checked_pixels(width: usize, height: usize) -> Result<usize, Error> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err("WebP: dimensions outside VP8L limits");
    }
    let pixels = width
        .checked_mul(height)
        .ok_or("WebP: dimension overflow")?;
    if pixels > MAX_PIXELS {
        return Err("WebP: too many pixels");
    }
    Ok(pixels)
}
impl BitWriter {
    pub(crate) fn new(capacity: usize) -> Result<Self, Error> {
        // Reserve RIFF and VP8L signature up front so the final stream can
        // become the output without a second allocation and full copy.
        let allocation = capacity
            .checked_add(22)
            .ok_or("WebP: output size overflow")?;
        let bytes = Buffer::new(allocation).map_err(|_| "WebP: allocation failed")?;
        Ok(Self {
            bytes,
            length: 21,
            pending: 0,
            used: 0,
            error: None,
        })
    }

    pub(crate) fn grow(&mut self) -> Result<(), Error> {
        // Capacity is normally sufficient from the first-pass accounting.
        // Keep the fallback bounded and fallible if accounting underestimates.
        let limit = MAX_FILE
            .checked_add(22)
            .ok_or("WebP: output size overflow")?;
        let old = self.bytes.len();
        if old >= limit {
            return Err("WebP: output size limit");
        }
        let new = old.saturating_mul(2).max(old + 1).min(limit);
        let mut replacement = Buffer::new(new).map_err(|_| "WebP: allocation failed")?;
        replacement.as_mut_slice()[..self.length]
            .copy_from_slice(&self.bytes.as_slice()[..self.length]);
        self.bytes = replacement;
        Ok(())
    }

    #[inline]
    pub(crate) fn byte(&mut self, byte: u8) {
        if self.error.is_some() {
            return;
        }
        if self.length >= MAX_FILE {
            self.error = Some("WebP: output size limit");
            return;
        }
        if self.length == self.bytes.len() {
            if let Err(error) = self.grow() {
                self.error = Some(error);
                return;
            }
        }
        self.bytes.as_mut_slice()[self.length] = byte;
        self.length += 1;
    }

    #[inline]
    pub(crate) fn put(&mut self, value: u32, count: u32) {
        if self.error.is_some() {
            return;
        }
        debug_assert!(count <= 24);
        debug_assert!(count == 0 || value < (1u32 << count));
        self.pending |= u64::from(value) << self.used;
        self.used += count;
        while self.used >= 8 {
            self.byte(self.pending as u8);
            self.pending >>= 8;
            self.used -= 8;
        }
    }

    pub(crate) fn finish(mut self) -> Result<Buffer, Error> {
        if self.used != 0 {
            self.byte(self.pending as u8);
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        let bits_len = self.length - 21;
        let chunk_len = bits_len
            .checked_add(1)
            .ok_or("WebP: output size overflow")?;
        let total = self
            .length
            .checked_add(chunk_len & 1)
            .ok_or("WebP: output size overflow")?;
        if total > MAX_FILE || chunk_len > u32::MAX as usize || total - 8 > u32::MAX as usize {
            return Err("WebP: output size limit");
        }
        if total != self.length {
            self.byte(0);
            if let Some(error) = self.error {
                return Err(error);
            }
        }
        let dst = self.bytes.as_mut_slice();
        dst[0..4].copy_from_slice(b"RIFF");
        dst[4..8].copy_from_slice(&((total - 8) as u32).to_le_bytes());
        dst[8..12].copy_from_slice(b"WEBP");
        dst[12..16].copy_from_slice(b"VP8L");
        dst[16..20].copy_from_slice(&(chunk_len as u32).to_le_bytes());
        dst[20] = 0x2f;
        self.bytes.truncate(total);
        Ok(self.bytes)
    }
}
impl EncodeTree {
    pub(crate) fn from_counts(counts: &[u32]) -> Result<Self, Error> {
        let mut used = [0usize; 280];
        let mut count = 0usize;
        for (symbol, &frequency) in counts.iter().enumerate() {
            if frequency != 0 {
                used[count] = symbol;
                count += 1;
            }
        }
        if count == 0 {
            return Ok(Self::Single(0));
        }
        if count == 1 {
            return Ok(if used[0] < 256 {
                Self::Single(used[0])
            } else {
                Self::SingleLong(used[0])
            });
        }
        if count == 2 && used[1] < 256 {
            return Ok(Self::Pair(used[0], used[1]));
        }
        let mut heap = BinaryHeap::new();
        heap.try_reserve(count * 2)
            .map_err(|_| "WebP: allocation failed")?;
        let mut children = [(usize::MAX, usize::MAX); 560];
        for &symbol in &used[..count] {
            heap.push(Reverse((u64::from(counts[symbol]), symbol)));
        }
        let mut next_node = counts.len();
        while heap.len() > 1 {
            let Reverse((a, left)) = heap.pop().ok_or("WebP: Huffman construction")?;
            let Reverse((b, right)) = heap.pop().ok_or("WebP: Huffman construction")?;
            if next_node >= children.len() {
                return Err("WebP: Huffman construction overflow");
            }
            children[next_node] = (left, right);
            heap.push(Reverse((a + b, next_node)));
            next_node += 1;
        }
        let mut lengths = zeros::<u8>(counts.len())?;
        let mut stack = [(0usize, 0u8); 560];
        let mut top = 1usize;
        stack[0] = (heap.pop().ok_or("WebP: Huffman construction")?.0 .1, 0);
        let mut too_deep = false;
        while top != 0 {
            top -= 1;
            let (node, depth) = stack[top];
            if node < counts.len() {
                if depth == 0 || depth > 15 {
                    too_deep = true;
                    break;
                }
                lengths[node] = depth;
            } else {
                let (left, right) = children[node];
                if top + 2 > stack.len() {
                    return Err("WebP: Huffman construction overflow");
                }
                stack[top] = (left, depth + 1);
                stack[top + 1] = (right, depth + 1);
                top += 2;
            }
        }
        if too_deep {
            // A complete balanced fallback keeps the longest code within the
            // format's 15-bit limit, assigning short codes to frequent values.
            used[..count].sort_unstable_by(|a, b| counts[*b].cmp(&counts[*a]).then(a.cmp(b)));
            lengths.fill(0);
            let short_bits = usize::BITS as usize - 1 - count.leading_zeros() as usize;
            let short_count = (1usize << (short_bits + 1)) - count;
            for (i, &symbol) in used[..count].iter().enumerate() {
                lengths[symbol] = (short_bits + usize::from(i >= short_count)) as u8;
            }
        }
        let mut code_counts = [0u16; 16];
        for &length in &lengths {
            if length != 0 {
                code_counts[length as usize] += 1;
            }
        }
        let mut next = [0u16; 16];
        let mut code = 0u32;
        for i in 1..=15 {
            code = (code + u32::from(code_counts[i - 1])) << 1;
            next[i] = code as u16;
        }
        let mut codes = zeros::<u16>(counts.len())?;
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let v = next[length as usize];
                next[length as usize] += 1;
                codes[symbol] = v.reverse_bits() >> (16 - length);
            }
        }
        let dynamic_bits = 63u64
            + 4 * counts.len() as u64
            + counts
                .iter()
                .zip(&lengths)
                .map(|(&f, &l)| u64::from(f) * u64::from(l))
                .sum::<u64>();
        if counts.len() >= 256 && counts[256..].iter().all(|&x| x == 0) {
            let total: u64 = counts.iter().map(|&x| u64::from(x)).sum();
            let full_bits = 42u64 + counts.len() as u64 + total * 8;
            if full_bits <= dynamic_bits {
                return Ok(Self::Full8);
            }
        }
        Ok(Self::Dynamic { lengths, codes })
    }

    pub(crate) fn description_bits(&self, alphabet: usize) -> usize {
        match self {
            Self::Single(v) => 3 + if *v > 1 { 8 } else { 1 },
            Self::SingleLong(_) => 18 + alphabet,
            Self::Pair(v, _) => 11 + if *v > 1 { 8 } else { 1 },
            Self::Full8 => 42 + alphabet,
            Self::Dynamic { .. } => 63 + 4 * alphabet,
        }
    }

    pub(crate) fn symbol_bits(&self, symbol: usize) -> usize {
        match self {
            Self::Single(_) | Self::SingleLong(_) => 0,
            Self::Pair(_, _) => 1,
            Self::Full8 => 8,
            Self::Dynamic { lengths, .. } => usize::from(lengths[symbol]),
        }
    }

    pub(crate) fn describe(&self, w: &mut BitWriter, alphabet: usize) {
        match self {
            Self::Single(first) | Self::Pair(first, _) => {
                w.put(1, 1);
                w.put(matches!(self, Self::Pair(_, _)) as u32, 1);
                let eight = *first > 1;
                w.put(eight as u32, 1);
                w.put(*first as u32, if eight { 8 } else { 1 });
                if let Self::Pair(_, second) = self {
                    w.put(*second as u32, 8);
                }
            }
            Self::SingleLong(first) => {
                w.put(0, 1);
                w.put(0, 4);
                for value in [0, 0, 1, 1] {
                    w.put(value, 3);
                }
                w.put(0, 1);
                for i in 0..alphabet {
                    w.put(u32::from(i == *first), 1);
                }
            }
            Self::Full8 => {
                w.put(0, 1);
                w.put(8, 4);
                for i in 0..12 {
                    w.put(u32::from(i == 2 || i == 11), 3);
                }
                w.put(0, 1);
                for i in 0..alphabet {
                    w.put(u32::from(i < 256), 1);
                }
            }
            Self::Dynamic { lengths, .. } => {
                w.put(0, 1);
                w.put(15, 4); // all 19 code-length entries
                              // Literal lengths 0..15 each use four bits; symbols 16..18
                              // are unused. This is a complete code-length prefix tree.
                for &symbol in &CODE_LENGTH_ORDER {
                    w.put(if symbol < 16 { 4 } else { 0 }, 3);
                }
                w.put(0, 1); // entire alphabet, no repeat codes
                for &length in lengths {
                    w.put(u32::from(length.reverse_bits() >> 4), 4);
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn write(&self, w: &mut BitWriter, symbol: usize) {
        match self {
            Self::Single(expected) | Self::SingleLong(expected) => {
                debug_assert_eq!(symbol, *expected)
            }
            Self::Pair(first, second) => {
                debug_assert!(symbol == *first || symbol == *second);
                w.put(u32::from(symbol == *second), 1);
            }
            Self::Full8 => w.put(u32::from((symbol as u8).reverse_bits()), 8),
            Self::Dynamic { lengths, codes } => {
                w.put(u32::from(codes[symbol]), u32::from(lengths[symbol]))
            }
        }
    }
}
pub(crate) fn prefix_parts(value: usize, limit: usize) -> Result<(usize, usize, u32), Error> {
    if value == 0 {
        return Err("WebP: LZ77 value outside format");
    }
    let (code, extra, remainder) = if value <= 4 {
        (value - 1, 0, 0)
    } else {
        let n = value - 1;
        let high = usize::BITS as usize - 1 - n.leading_zeros() as usize;
        let extra = high - 1;
        let code = 2 * high + ((n >> extra) & 1);
        (code, extra, (n & ((1usize << extra) - 1)) as u32)
    };
    if code >= limit {
        return Err("WebP: LZ77 value outside format");
    }
    Ok((code, extra, remainder))
}
pub(crate) fn distance_code(distance: usize, width: usize) -> usize {
    if distance == 1 {
        2
    } else if distance == width {
        1
    } else {
        distance + 120
    }
}
/// Visit the same tokens on both passes. The table contains positions only,
/// is bounded to 256 KiB and is discarded after this request.
pub(crate) fn visit_tokens<F: FnMut(usize, EncodeToken) -> Result<(), Error>>(
    pixels: &[u32],
    width: usize,
    mut emit: F,
) -> Result<(), Error> {
    let mut last = zeros::<u32>(1 << 16)?;
    let hash = |v: u32| (v.wrapping_mul(0x1e35_a7bd) >> 16) as usize;
    let mut i = 0usize;
    while i < pixels.len() {
        let slot = hash(pixels[i]);
        let previous = last[slot] as usize;
        last[slot] = (i + 1) as u32;
        let mut best_length = 0usize;
        let mut best_distance = 0usize;
        // The latest hash match and the pixel directly above are cheap,
        // independent candidates. The latter helps repeated rows even when
        // another occurrence overwrote the hash slot in the meantime.
        for candidate in [previous.checked_sub(1), i.checked_sub(width)] {
            let Some(prev) = candidate else { continue };
            if prev >= i {
                continue;
            }
            let distance = i - prev;
            if distance > 1_048_456 || pixels[prev] != pixels[i] {
                continue;
            }
            let max = (pixels.len() - i).min(4096);
            let mut length = 1usize;
            while length < max && pixels[prev + length] == pixels[i + length] {
                length += 1;
            }
            if length > best_length || (length == best_length && distance < best_distance) {
                best_length = length;
                best_distance = distance;
            }
        }
        if best_length >= 4 {
            emit(
                i,
                EncodeToken::Copy {
                    length: best_length,
                    distance: distance_code(best_distance, width),
                },
            )?;
            for k in i + 1..i + best_length {
                last[hash(pixels[k])] = (k + 1) as u32;
            }
            i += best_length;
            continue;
        }
        emit(i, EncodeToken::Literal(pixels[i]))?;
        i += 1;
    }
    Ok(())
}
impl EncodeCounts {
    pub(crate) fn new() -> Self {
        Self {
            green: [0; 280],
            red: [0; 256],
            blue: [0; 256],
            alpha: [0; 256],
            distance: [0; 40],
        }
    }
}
