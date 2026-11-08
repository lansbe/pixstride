use super::*;
pub(crate) fn parse_quantization(
    bytes: &[u8],
    tables: &mut [[u16; 64]; 4],
    present: &mut [bool; 4],
) -> Result<(), Error> {
    let mut pos = 0usize;
    while pos < bytes.len() {
        let tag = bytes[pos];
        pos += 1;
        let precision = tag >> 4;
        let id = (tag & 15) as usize;
        if precision > 1 || id >= 4 {
            return Err("JPEG progressive: invalid quantization table");
        }
        let width = usize::from(precision) + 1;
        let end = pos
            .checked_add(64 * width)
            .filter(|&end| end <= bytes.len())
            .ok_or("JPEG progressive: truncated quantization table")?;
        for k in 0..64 {
            let value = if width == 1 {
                bytes[pos + k] as u16
            } else {
                u16::from_be_bytes([bytes[pos + 2 * k], bytes[pos + 2 * k + 1]])
            };
            if value == 0 {
                return Err("JPEG progressive: zero quantization");
            }
            tables[id][ZIGZAG[k]] = value;
        }
        present[id] = true;
        pos = end;
    }
    Ok(())
}
impl Layout {
    pub(crate) const EMPTY: Self = Self {
        block_offset: 0,
        padded_blocks_w: 0,
        padded_blocks_h: 0,
        blocks_w: 0,
        blocks_h: 0,
        width: 0,
        height: 0,
        plane_offset: 0,
    };
}
pub(crate) fn ceil_div(n: usize, d: usize) -> Result<usize, Error> {
    n.checked_add(d - 1)
        .map(|v| v / d)
        .ok_or("JPEG progressive: dimensions out of bounds")
}
pub(crate) fn words_mut(buffer: &mut Buffer) -> Result<&mut [i32], Error> {
    // Buffer's two backends allocate at least 16-byte aligned memory and
    // initialize it to zero. Check the alignment and complete length before
    // exposing the exclusive i32 view to coefficient operations.
    let (prefix, words, suffix) = unsafe { buffer.as_mut_slice().align_to_mut::<i32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return Err("JPEG progressive: invalid coefficient alignment");
    }
    Ok(words)
}
pub(crate) fn words(buffer: &Buffer) -> Result<&[i32], Error> {
    // As above, but retain a read-only view while rendering the final image.
    let (prefix, words, suffix) = unsafe { buffer.as_slice().align_to::<i32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return Err("JPEG progressive: invalid coefficient alignment");
    }
    Ok(words)
}
impl Frame {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 6 || bytes[0] != 8 {
            return Err("JPEG progressive: only 8-bit precision is supported");
        }
        let height = read_u16(bytes, 1)?;
        let width = read_u16(bytes, 3)?;
        let count = bytes[5] as usize;
        if !matches!(count, 1 | 3 | 4) || bytes.len() != 6 + count * 3 {
            return Err("JPEG progressive: invalid frame components");
        }
        if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS) {
            return Err("JPEG progressive: dimensions out of bounds");
        }
        let mut components = [Component::EMPTY; MAX_COMPONENTS];
        let mut max_h = 0usize;
        let mut max_v = 0usize;
        let mut sampling_blocks = 0usize;
        for i in 0..count {
            let p = 6 + i * 3;
            let h = (bytes[p + 1] >> 4) as usize;
            let v = (bytes[p + 1] & 15) as usize;
            let quant = bytes[p + 2] as usize;
            if h == 0
                || h > 4
                || v == 0
                || v > 4
                || quant >= 4
                || components[..i]
                    .iter()
                    .any(|component| component.id == bytes[p])
            {
                return Err("JPEG progressive: invalid sampling");
            }
            components[i] = Component {
                id: bytes[p],
                h,
                v,
                quant,
                ..Component::EMPTY
            };
            max_h = max_h.max(h);
            max_v = max_v.max(v);
            sampling_blocks += h * v;
        }
        if sampling_blocks > 10
            || (count == 1 && (max_h != 1 || max_v != 1))
            || components[..count]
                .iter()
                .any(|c| !max_h.is_multiple_of(c.h) || !max_v.is_multiple_of(c.v))
        {
            return Err("JPEG progressive: unsupported sampling");
        }
        let mcu_cols = ceil_div(width, max_h * 8)?;
        let mcu_rows = ceil_div(height, max_v * 8)?;
        let mut layout = [Layout::EMPTY; MAX_COMPONENTS];
        let mut total_blocks = 0usize;
        let mut plane_bytes = 0usize;
        for i in 0..count {
            let c = components[i];
            let actual_w = ceil_div(width * c.h, max_h)?;
            let actual_h = ceil_div(height * c.v, max_v)?;
            let padded_blocks_w = mcu_cols * c.h;
            let padded_blocks_h = mcu_rows * c.v;
            let blocks_w = ceil_div(actual_w, 8)?;
            let blocks_h = ceil_div(actual_h, 8)?;
            let blocks = padded_blocks_w
                .checked_mul(padded_blocks_h)
                .ok_or("JPEG progressive: too many blocks")?;
            layout[i] = Layout {
                block_offset: total_blocks,
                padded_blocks_w,
                padded_blocks_h,
                blocks_w,
                blocks_h,
                width: actual_w,
                height: actual_h,
                plane_offset: plane_bytes,
            };
            total_blocks = total_blocks
                .checked_add(blocks)
                .ok_or("JPEG progressive: too many blocks")?;
            plane_bytes = plane_bytes
                .checked_add(
                    actual_w
                        .checked_mul(actual_h)
                        .ok_or("JPEG progressive: plane too large")?,
                )
                .ok_or("JPEG progressive: plane too large")?;
        }
        let coeff_bytes = total_blocks
            .checked_mul(64 * core::mem::size_of::<i32>())
            .ok_or("JPEG progressive: coefficients too large")?;
        let pixels = width
            .checked_mul(height)
            .ok_or("JPEG progressive: image too large")?;
        if pixels > MAX_PIXELS
            || coeff_bytes
                .checked_add(plane_bytes)
                .and_then(|n| n.checked_add(pixels * 4))
                .is_none_or(|n| n > MAX_FILE)
        {
            return Err("JPEG progressive: memory budget exceeded");
        }
        Ok(Self {
            width,
            height,
            count,
            max_h,
            max_v,
            mcu_cols,
            mcu_rows,
            components,
            layout,
            approximation: [[-1; 64]; MAX_COMPONENTS],
            component_quant: [[0; 64]; MAX_COMPONENTS],
            has_quant: [false; MAX_COMPONENTS],
            coeffs: Buffer::new(coeff_bytes)?,
            plane_bytes,
        })
    }

    pub(crate) fn block_base(&self, ci: usize, x: usize, y: usize) -> Result<usize, Error> {
        let l = self.layout[ci];
        if x >= l.padded_blocks_w || y >= l.padded_blocks_h {
            return Err("JPEG progressive: block out of bounds");
        }
        Ok((l.block_offset + y * l.padded_blocks_w + x) * 64)
    }

    pub(crate) fn render(self, adobe_transform: Option<u8>) -> Result<Image, Error> {
        for i in 0..self.count {
            if self.approximation[i][0] < 0 || !self.has_quant[i] {
                return Err("JPEG progressive: missing DC component");
            }
        }
        if self.count == 3 && adobe_transform.is_some_and(|v| v > 1) {
            return Err("JPEG progressive: unsupported Adobe transform");
        }
        if self.count == 4 && !matches!(adobe_transform, Some(0 | 2)) {
            return Err("JPEG progressive: CMYK without a recognized Adobe transform");
        }
        let mut planes = Buffer::new(self.plane_bytes)?;
        let coefficients = words(&self.coeffs)?;
        for ci in 0..self.count {
            let l = self.layout[ci];
            for by in 0..l.blocks_h {
                for bx in 0..l.blocks_w {
                    let base = self.block_base(ci, bx, by)?;
                    let mut dequantized = [0f64; 64];
                    let mut row_mask = 1u8;
                    let mut has_ac = false;
                    for k in 0..64 {
                        let value = coefficients[base + k];
                        dequantized[k] = value as f64 * self.component_quant[ci][k] as f64;
                        if k != 0 && value != 0 {
                            row_mask |= 1 << (k / 8);
                            has_ac = true;
                        }
                    }
                    let mut block = [0u8; 64];
                    inverse_dct(&dequantized, row_mask, has_ac, &mut block);
                    let w = 8.min(l.width - bx * 8);
                    let h = 8.min(l.height - by * 8);
                    for row in 0..h {
                        let dest = l.plane_offset + (by * 8 + row) * l.width + bx * 8;
                        planes.as_mut_slice()[dest..dest + w]
                            .copy_from_slice(&block[row * 8..row * 8 + w]);
                    }
                }
            }
        }

        // Frame identifiers 1,2,3 specify Y,Cb,Cr independently of frame order.
        let mut order = [0usize, 1, 2, 3];
        if self.count == 3 {
            let mut id_order = [3usize; 3];
            for i in 0..3 {
                let id = self.components[i].id;
                if (1..=3).contains(&id) {
                    id_order[id as usize - 1] = i;
                }
            }
            if id_order.iter().all(|&v| v < 3) {
                order[..3].copy_from_slice(&id_order);
            }
        }
        let rgb_components = self.count == 3
            && (adobe_transform == Some(0)
                || (self.components[0].id == b'R'
                    && self.components[1].id == b'G'
                    && self.components[2].id == b'B'));
        let mut pixels = Buffer::new(self.width * self.height * 4)?;
        for y in 0..self.height {
            for x in 0..self.width {
                let mut values = [0u8; MAX_COMPONENTS];
                for channel in 0..self.count {
                    let ci = order[channel];
                    let l = self.layout[ci];
                    values[channel] = if self.count == 4 || (!rgb_components && channel != 0) {
                        crate::jpeg::cmyk::adobe_sample_plane(
                            planes.as_slice(),
                            l.plane_offset,
                            l.width,
                            l.height,
                            x,
                            y,
                            self.components[ci],
                            self.max_h,
                            self.max_v,
                            adobe_transform.unwrap_or(1),
                            channel,
                        )
                    } else {
                        sample_plane(
                            planes.as_slice(),
                            l.plane_offset,
                            l.width,
                            l.height,
                            x,
                            y,
                            self.components[ci],
                            self.max_h,
                            self.max_v,
                        )
                    };
                }
                let offset = (y * self.width + x) * 4;
                let target = pixels.as_mut_slice();
                if self.count == 1 {
                    target[offset..offset + 4]
                        .copy_from_slice(&[values[0], values[0], values[0], 255]);
                } else if rgb_components {
                    target[offset..offset + 4]
                        .copy_from_slice(&[values[0], values[1], values[2], 255]);
                } else if self.count == 4 {
                    let rgb = crate::jpeg::cmyk::adobe_to_rgb(values, adobe_transform.unwrap())?;
                    target[offset..offset + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                } else {
                    store_rgb(target, offset, values[0], values[1], values[2]);
                }
            }
        }
        Ok(Image {
            width: self.width,
            height: self.height,
            pixels,
        })
    }
}
pub(crate) fn parse_scan(bytes: &[u8], frame: &Frame) -> Result<Scan, Error> {
    let count = *bytes.first().ok_or("JPEG progressive: truncated scan")? as usize;
    if count == 0 || count > frame.count || bytes.len() != 1 + count * 2 + 3 {
        return Err("JPEG progressive: invalid scan header");
    }
    let mut order = [0usize; MAX_COMPONENTS];
    let mut dc = [0usize; MAX_COMPONENTS];
    let mut ac = [0usize; MAX_COMPONENTS];
    let mut used = [false; MAX_COMPONENTS];
    for i in 0..count {
        let id = bytes[1 + i * 2];
        let table = bytes[2 + i * 2];
        let ci = frame.components[..frame.count]
            .iter()
            .position(|c| c.id == id)
            .ok_or("JPEG progressive: unknown scan component")?;
        if used[ci] || table >> 4 >= 4 || table & 15 >= 4 {
            return Err("JPEG progressive: invalid scan selector");
        }
        used[ci] = true;
        order[i] = ci;
        dc[i] = (table >> 4) as usize;
        ac[i] = (table & 15) as usize;
    }
    let start = bytes[1 + count * 2] as usize;
    let end = bytes[2 + count * 2] as usize;
    let high = bytes[3 + count * 2] >> 4;
    let low = bytes[3 + count * 2] & 15;
    if start > end
        || end > 63
        || (start == 0 && end != 0)
        || (start != 0 && count != 1)
        || high > 13
        || low > 13
        || (high != 0 && high != low + 1)
    {
        return Err("JPEG progressive: invalid progression");
    }
    for &ci in &order[..count] {
        if start != 0 && frame.approximation[ci][0] < 0 {
            return Err("JPEG progressive: AC scan before DC");
        }
        for k in start..=end {
            let previous = frame.approximation[ci][k];
            if (high == 0 && previous != -1) || (high != 0 && previous != high as i8) {
                return Err("JPEG progressive: invalid refinement order");
            }
        }
    }
    Ok(Scan {
        count,
        order,
        dc,
        ac,
        start,
        end,
        high,
        low,
    })
}
pub(crate) fn add_refinement(value: &mut i32, bit: u8, low: u8) -> Result<(), Error> {
    if bit != 0 {
        let step = 1i32 << low;
        *value = if *value > 0 {
            value.checked_add(step)
        } else {
            value.checked_sub(step)
        }
        .ok_or("JPEG progressive: coefficient out of bounds")?;
    }
    Ok(())
}
