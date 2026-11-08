use super::*;
pub(crate) fn encode_prepared(prepared: Prepared) -> Result<Buffer, Error> {
    let Prepared {
        width,
        height,
        has_alpha,
        modes,
        transformed,
    } = prepared;
    let words = transformed.words();
    let mut predictor_freq = [0u32; 280];
    for &mode in &modes {
        predictor_freq[usize::from(mode)] += 1;
    }
    let predictor_tree = EncodeTree::from_counts(&predictor_freq)?;
    let zones = EncodeZones::new(width, height);
    let mut frequencies = Vec::new();
    frequencies
        .try_reserve_exact(zones.count())
        .map_err(|_| "WebP: allocation failed")?;
    for _ in 0..zones.count() {
        frequencies.push(EncodeCounts::new());
    }
    let mut extra_bits = 0usize;
    visit_tokens(words, width, |position, token| {
        let freq = &mut frequencies[zones.at(position, width)];
        match token {
            EncodeToken::Literal(p) => {
                freq.green[((p >> 8) & 255) as usize] += 1;
                freq.red[((p >> 16) & 255) as usize] += 1;
                freq.blue[(p & 255) as usize] += 1;
                freq.alpha[(p >> 24) as usize] += 1;
            }
            EncodeToken::Copy { length, distance } => {
                let (lp, le, _) = prefix_parts(length, 24)?;
                let (dp, de, _) = prefix_parts(distance, 40)?;
                freq.green[256 + lp] += 1;
                freq.distance[dp] += 1;
                extra_bits += le + de;
            }
        }
        Ok(())
    })?;
    let mut groups = Vec::new();
    groups
        .try_reserve_exact(zones.count())
        .map_err(|_| "WebP: allocation failed")?;
    for f in &frequencies {
        groups.push(EncodeGroup::from_counts(f)?);
    }
    let mut meta_freq = [0u32; 280];
    if zones.count() > 1 {
        for y in 0..zones.tile_height {
            for x in 0..zones.tile_width {
                meta_freq[zones.group(x, y)] += 1;
            }
        }
    }
    let meta_tree = EncodeTree::from_counts(&meta_freq)?;
    let mut encoded_bits = extra_bits;
    encoded_bits = encoded_bits
        .checked_add(predictor_tree.description_bits(280))
        .ok_or("WebP: output size overflow")?;
    for (symbol, &frequency) in predictor_freq.iter().enumerate() {
        encoded_bits = encoded_bits
            .checked_add(
                predictor_tree
                    .symbol_bits(symbol)
                    .checked_mul(frequency as usize)
                    .ok_or("WebP: output size overflow")?,
            )
            .ok_or("WebP: output size overflow")?;
    }
    if zones.count() > 1 {
        encoded_bits = encoded_bits
            .checked_add(meta_tree.description_bits(280))
            .ok_or("WebP: output size overflow")?;
        for (symbol, &frequency) in meta_freq.iter().enumerate() {
            encoded_bits = encoded_bits
                .checked_add(
                    meta_tree
                        .symbol_bits(symbol)
                        .checked_mul(frequency as usize)
                        .ok_or("WebP: output size overflow")?,
                )
                .ok_or("WebP: output size overflow")?;
        }
    }
    for (group, freq) in groups.iter().zip(&frequencies) {
        for (tree, counts) in group.entries(freq) {
            encoded_bits = encoded_bits
                .checked_add(tree.description_bits(counts.len()))
                .ok_or("WebP: output size overflow")?;
            for (symbol, &frequency) in counts.iter().enumerate() {
                encoded_bits = encoded_bits
                    .checked_add(
                        tree.symbol_bits(symbol)
                            .checked_mul(frequency as usize)
                            .ok_or("WebP: output size overflow")?,
                    )
                    .ok_or("WebP: output size overflow")?;
            }
        }
    }
    // Above accounts for each code and extra bit. The fixed overhead (image
    // header and the predictor image's constant auxiliary trees) is below 2 KiB.
    let capacity = encoded_bits
        .checked_add(7)
        .and_then(|n| (n / 8).checked_add(2048))
        .ok_or("WebP: output size overflow")?;
    if capacity > MAX_FILE {
        return Err("WebP: output size limit");
    }
    let mut w = BitWriter::new(capacity)?;
    w.put((width - 1) as u32, 14);
    w.put((height - 1) as u32, 14);
    w.put(u32::from(has_alpha), 1);
    w.put(0, 3); // version
    w.put(1, 1);
    w.put(0, 2);
    w.put(2, 3); // predictor transform, 16x16 tiles
    w.put(0, 1); // predictor image has no color cache or meta-prefix flag
    predictor_tree.describe(&mut w, 280);
    for value in [0usize, 0, 255, 0] {
        EncodeTree::Single(value).describe(&mut w, 256);
    }
    for &mode in &modes {
        predictor_tree.write(&mut w, usize::from(mode));
    }
    drop(modes);
    drop(predictor_tree);
    w.put(1, 1);
    w.put(2, 2); // subtract-green transform
    w.put(0, 1); // end of transforms
    w.put(0, 1); // no color cache
    if zones.count() > 1 {
        w.put(1, 1);
        w.put(6, 3); // 256x256 entropy tiles
        w.put(0, 1); // entropy image's color cache
        meta_tree.describe(&mut w, 280);
        for value in [0usize, 0, 255, 0] {
            EncodeTree::Single(value).describe(&mut w, 256);
        }
        for y in 0..zones.tile_height {
            for x in 0..zones.tile_width {
                meta_tree.write(&mut w, zones.group(x, y));
            }
        }
    } else {
        w.put(0, 1);
    }
    drop(meta_tree);
    for (group, freq) in groups.iter().zip(&frequencies) {
        for (tree, counts) in group.entries(freq) {
            tree.describe(&mut w, counts.len());
        }
    }
    drop(frequencies);
    visit_tokens(words, width, |position, token| {
        let group = &groups[zones.at(position, width)];
        match token {
            EncodeToken::Literal(p) => {
                group.green.write(&mut w, ((p >> 8) & 255) as usize);
                group.red.write(&mut w, ((p >> 16) & 255) as usize);
                group.blue.write(&mut w, (p & 255) as usize);
                group.alpha.write(&mut w, (p >> 24) as usize);
            }
            EncodeToken::Copy {
                length,
                distance: d,
            } => {
                let (lp, le, lv) = prefix_parts(length, 24)?;
                let (dp, de, dv) = prefix_parts(d, 40)?;
                group.green.write(&mut w, 256 + lp);
                w.put(lv, le as u32);
                group.distance.write(&mut w, dp);
                w.put(dv, de as u32);
            }
        }
        Ok(())
    })?;
    drop(groups);
    drop(transformed);
    w.finish()
}
impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    pub(crate) fn take(&mut self, count: usize) -> Result<u32, Error> {
        if count > 24
            || self.offset.checked_add(count).ok_or("WebP: bit overflow")? > self.data.len() * 8
        {
            return Err("WebP: truncated bitstream");
        }
        let mut value = 0u32;
        for i in 0..count {
            value |= u32::from((self.data[self.offset >> 3] >> (self.offset & 7)) & 1) << i;
            self.offset += 1;
        }
        Ok(value)
    }
}
pub(crate) fn zeros<T: Clone + Default>(len: usize) -> Result<Vec<T>, Error> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| "WebP: allocation failed")?;
    v.resize(len, T::default());
    Ok(v)
}
impl Huffman {
    pub(crate) fn build(lengths: &[u8]) -> Result<Self, Error> {
        let mut counts = [0u16; 16];
        let mut used = 0usize;
        let mut last = 0u16;
        for (symbol, &len) in lengths.iter().enumerate() {
            if len > 15 {
                return Err("WebP: invalid Huffman length");
            }
            if len != 0 {
                counts[len as usize] = counts[len as usize]
                    .checked_add(1)
                    .ok_or("WebP: Huffman overflow")?;
                used += 1;
                last = symbol as u16;
            }
        }
        if used == 0 {
            return Err("WebP: empty Huffman tree");
        }
        let single = if used == 1 {
            if counts[1] != 1 {
                return Err("WebP: invalid one-symbol Huffman tree");
            }
            Some(last)
        } else {
            let mut left = 1i32;
            for &count in &counts[1..] {
                left = left * 2 - i32::from(count);
                if left < 0 {
                    return Err("WebP: oversubscribed Huffman tree");
                }
            }
            if left != 0 {
                return Err("WebP: incomplete Huffman tree");
            }
            None
        };
        let mut first = [0u16; 16];
        let mut offsets = [0usize; 16];
        let mut code = 0u32;
        let mut offset = 0usize;
        for n in 1..=15 {
            code = (code + u32::from(counts[n - 1])) << 1;
            first[n] = code as u16;
            offsets[n] = offset;
            offset += usize::from(counts[n]);
        }
        let mut symbols = zeros::<u16>(used)?;
        let mut next = offsets;
        for (symbol, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let index = next[len as usize];
                symbols[index] = symbol as u16;
                next[len as usize] += 1;
            }
        }
        Ok(Self {
            counts,
            first,
            offsets,
            symbols,
            single,
        })
    }

    pub(crate) fn read(&self, bits: &mut BitReader<'_>) -> Result<usize, Error> {
        if let Some(symbol) = self.single {
            return Ok(usize::from(symbol));
        }
        let mut code = 0u32;
        for n in 1..=15 {
            code = (code << 1) | bits.take(1)?;
            let first = u32::from(self.first[n]);
            if code >= first && code - first < u32::from(self.counts[n]) {
                return Ok(usize::from(
                    self.symbols[self.offsets[n] + (code - first) as usize],
                ));
            }
        }
        Err("WebP: invalid Huffman code")
    }
}
pub(crate) fn read_tree(bits: &mut BitReader<'_>, size: usize) -> Result<Huffman, Error> {
    let mut lengths = zeros::<u8>(size)?;
    if bits.take(1)? == 1 {
        let count = bits.take(1)? as usize + 1;
        let first_bits = if bits.take(1)? == 0 { 1 } else { 8 };
        let first = bits.take(first_bits)? as usize;
        if first >= size {
            return Err("WebP: Huffman symbol outside alphabet");
        }
        lengths[first] = 1;
        if count == 2 {
            let second = bits.take(8)? as usize;
            if second >= size {
                return Err("WebP: Huffman symbol outside alphabet");
            }
            lengths[second] = 1;
        }
    } else {
        let num_lengths = bits.take(4)? as usize + 4;
        let mut length_lengths = [0u8; 19];
        for &index in &CODE_LENGTH_ORDER[..num_lengths] {
            length_lengths[index] = bits.take(3)? as u8;
        }
        let decoder = Huffman::build(&length_lengths)?;
        let count = if bits.take(1)? == 0 {
            size
        } else {
            let n = 2 + 2 * bits.take(3)? as usize;
            let count = 2 + bits.take(n)? as usize;
            if count > size {
                return Err("WebP: Huffman length count outside alphabet");
            }
            count
        };
        let mut pos = 0usize;
        let mut previous = 8u8;
        for _ in 0..count {
            if pos == size {
                break;
            }
            let symbol = decoder.read(bits)?;
            if symbol <= 15 {
                lengths[pos] = symbol as u8;
                if symbol != 0 {
                    previous = symbol as u8;
                }
                pos += 1;
            } else {
                let (repeat, value) = match symbol {
                    16 => (3 + bits.take(2)? as usize, previous),
                    17 => (3 + bits.take(3)? as usize, 0),
                    18 => (11 + bits.take(7)? as usize, 0),
                    _ => return Err("WebP: invalid code-length symbol"),
                };
                if pos.checked_add(repeat).ok_or("WebP: length overflow")? > size {
                    return Err("WebP: code-length repeat exceeds alphabet");
                }
                lengths[pos..pos + repeat].fill(value);
                pos += repeat;
            }
        }
    }
    Huffman::build(&lengths)
}
pub(crate) fn prefix_value(bits: &mut BitReader<'_>, prefix: usize) -> Result<usize, Error> {
    if prefix < 4 {
        return Ok(prefix + 1);
    }
    if prefix >= 40 {
        return Err("WebP: invalid LZ77 prefix");
    }
    let extra = (prefix - 2) >> 1;
    Ok(((2 + (prefix & 1)) << extra) + bits.take(extra)? as usize + 1)
}
