use super::*;
pub(crate) fn decode_block(
    bits: &mut Entropy<'_>,
    block: &mut [i32],
    predictor: &mut i32,
    scan: &Scan,
    dc: &Huffman,
    ac: &Huffman,
    eob_run: &mut usize,
) -> Result<(), Error> {
    let step = 1i32 << scan.low;
    if scan.start == 0 {
        if scan.high == 0 {
            let (_, diff) = dc.token::<false>(bits)?;
            *predictor = predictor
                .checked_add(diff)
                .ok_or("JPEG progressive: invalid predictor")?;
            if !(-(1 << 20)..=(1 << 20)).contains(predictor) {
                return Err("JPEG progressive: predictor out of bounds");
            }
            block[0] = i32::try_from(*predictor as i64 * step as i64)
                .map_err(|_| "JPEG progressive: DC coefficient out of bounds")?;
        } else if bits.bit()? != 0 {
            // DC uses the low bit of the signed point-transformed value.
            block[0] |= step;
        }
        return Ok(());
    }
    if scan.high == 0 {
        if *eob_run != 0 {
            *eob_run -= 1;
            return Ok(());
        }
        let mut k = scan.start;
        while k <= scan.end {
            let (symbol, amplitude) = ac.token::<true>(bits)?;
            let run = (symbol >> 4) as usize;
            let size = symbol & 15;
            if size == 0 {
                if run == 15 {
                    k += 16;
                    if k > scan.end + 1 {
                        return Err("JPEG progressive: AC run out of bounds");
                    }
                } else {
                    *eob_run = (1usize << run) + bits.take(run as u8)? as usize - 1;
                    break;
                }
            } else {
                k += run;
                if k > scan.end {
                    return Err("JPEG progressive: AC coefficient out of bounds");
                }
                block[ZIGZAG[k]] = i32::try_from(amplitude as i64 * step as i64)
                    .map_err(|_| "JPEG progressive: AC coefficient too large")?;
                k += 1;
            }
        }
        return Ok(());
    }

    let mut k = scan.start;
    if *eob_run == 0 {
        while k <= scan.end {
            let (symbol, amplitude) = ac.token::<true>(bits)?;
            let mut zeroes = (symbol >> 4) as usize;
            let size = symbol & 15;
            if size == 0 && zeroes != 15 {
                *eob_run = (1usize << zeroes) + bits.take(zeroes as u8)? as usize;
                break;
            }
            if size > 1 {
                return Err("JPEG progressive: invalid AC refinement");
            }
            let mut inserted = false;
            if size == 0 {
                zeroes = 16;
            }
            while k <= scan.end {
                let coefficient = &mut block[ZIGZAG[k]];
                if *coefficient != 0 {
                    add_refinement(coefficient, bits.bit()?, scan.low)?;
                } else if zeroes != 0 {
                    zeroes -= 1;
                } else {
                    *coefficient = amplitude * step;
                    inserted = true;
                    k += 1;
                    break;
                }
                k += 1;
                if size == 0 && zeroes == 0 {
                    break;
                }
            }
            if zeroes != 0 || (size == 1 && !inserted) {
                return Err("JPEG progressive: refinement run out of bounds");
            }
        }
    }
    if *eob_run != 0 {
        while k <= scan.end {
            let coefficient = &mut block[ZIGZAG[k]];
            if *coefficient != 0 {
                add_refinement(coefficient, bits.bit()?, scan.low)?;
            }
            k += 1;
        }
        *eob_run -= 1;
    }
    Ok(())
}
pub(super) fn scan_geometry(frame: &Frame, scan: &Scan) -> Result<(usize, usize, usize), Error> {
    let (cols, rows, blocks_per_mcu) = if scan.count == 1 {
        let layout = frame.layout[scan.order[0]];
        (layout.blocks_w, layout.blocks_h, 1)
    } else {
        let mut blocks = 0usize;
        for &ci in &scan.order[..scan.count] {
            let component = frame.components[ci];
            blocks = component
                .h
                .checked_mul(component.v)
                .and_then(|count| blocks.checked_add(count))
                .ok_or("JPEG progressive: scan work exceeds limit")?;
        }
        (frame.mcu_cols, frame.mcu_rows, blocks)
    };
    let total = cols
        .checked_mul(rows)
        .ok_or("JPEG progressive: scan work exceeds limit")?;
    let visits = total
        .checked_mul(blocks_per_mcu)
        .ok_or("JPEG progressive: scan work exceeds limit")?;
    Ok((cols, total, visits))
}

pub(crate) fn decode_scan(
    data: &[u8],
    position: usize,
    frame: &mut Frame,
    scan: &Scan,
    quant: &[[u16; 64]; 4],
    present: &[bool; 4],
    dc: &[Huffman; 4],
    ac: &[Huffman; 4],
    restart_interval: usize,
    remaining_visits: &mut usize,
) -> Result<usize, Error> {
    let (cols, total, visits) = scan_geometry(frame, scan)?;
    *remaining_visits = remaining_visits
        .checked_sub(visits)
        .ok_or("JPEG progressive: scan work exceeds limit")?;
    for i in 0..scan.count {
        let ci = scan.order[i];
        let table = frame.components[ci].quant;
        if !present[table]
            || (scan.start == 0 && scan.high == 0 && !dc[scan.dc[i]].present)
            || (scan.start != 0 && !ac[scan.ac[i]].present)
        {
            return Err("JPEG progressive: missing required table");
        }
        if !frame.has_quant[ci] {
            frame.component_quant[ci] = quant[table];
            frame.has_quant[ci] = true;
        }
    }
    let mut bits = Entropy::new(data, position);
    let mut predictors = [0i32; MAX_COMPONENTS];
    let mut eob_run = 0usize;
    let mut restart = 0u8;
    for mcu in 0..total {
        let mx = mcu % cols;
        let my = mcu / cols;
        for position in 0..scan.count {
            let ci = scan.order[position];
            let component = frame.components[ci];
            let (bw, bh) = if scan.count == 1 {
                (1, 1)
            } else {
                (component.h, component.v)
            };
            for dy in 0..bh {
                for dx in 0..bw {
                    let bx = if scan.count == 1 {
                        mx
                    } else {
                        mx * component.h + dx
                    };
                    let by = if scan.count == 1 {
                        my
                    } else {
                        my * component.v + dy
                    };
                    let base = frame.block_base(ci, bx, by)?;
                    let all = words_mut(&mut frame.coeffs)?;
                    let block = all
                        .get_mut(base..base + 64)
                        .ok_or("JPEG progressive: missing block")?;
                    decode_block(
                        &mut bits,
                        block,
                        &mut predictors[ci],
                        scan,
                        &dc[scan.dc[position]],
                        &ac[scan.ac[position]],
                        &mut eob_run,
                    )?;
                }
            }
        }
        if restart_interval != 0 && (mcu + 1) % restart_interval == 0 && mcu + 1 < total {
            if eob_run != 0 {
                return Err("JPEG progressive: EOB crosses restart boundary");
            }
            bits.marker(0xd0 + restart)?;
            restart = (restart + 1) % 8;
            predictors.fill(0);
        }
    }
    if eob_run != 0 || bits.remaining >= 8 {
        return Err("JPEG progressive: invalid scan ending");
    }
    for &ci in &scan.order[..scan.count] {
        for k in scan.start..=scan.end {
            frame.approximation[ci][k] = scan.low as i8;
        }
    }
    Ok(bits.pos)
}
/// Decode a complete 8-bit Huffman SOF2 stream into non-premultiplied RGBA8.
/// Grayscale, YCbCr, Adobe RGB and Adobe CMYK/YCCK are supported. Arithmetic,
/// 12-bit, unknown CMYK transformations and hierarchical JPEG are rejected.
pub(crate) fn decode(data: &[u8]) -> Result<Image, Error> {
    decode_with_limits(data, MAX_BLOCK_VISITS, MAX_HUFFMAN_TABLE_DEFINITIONS)
}

pub(super) fn decode_with_limits(
    data: &[u8],
    mut remaining_visits: usize,
    mut huffman_remaining: usize,
) -> Result<Image, Error> {
    if data.len() > MAX_FILE || data.get(..2) != Some(&[0xff, 0xd8]) {
        return Err("JPEG progressive: invalid signature or size");
    }
    let mut pos = 2usize;
    let mut quant = [[0u16; 64]; 4];
    let mut quant_present = [false; 4];
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut frame: Option<Frame> = None;
    let mut restart_interval = 0usize;
    let mut adobe_transform = None;
    let mut scans = 0usize;
    loop {
        let marker = read_marker(data, &mut pos)?;
        if marker == 0xd9 {
            if scans == 0 {
                return Err("JPEG progressive: missing scan");
            }
            return frame
                .ok_or("JPEG progressive: missing frame")?
                .render(adobe_transform);
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            return Err("JPEG progressive: misplaced marker");
        }
        let length = read_u16(data, pos)?;
        let end = pos
            .checked_add(length)
            .filter(|&v| length >= 2 && v <= data.len())
            .ok_or("JPEG progressive: truncated segment")?;
        let payload = &data[pos + 2..end];
        pos = end;
        match marker {
            0xdb => parse_quantization(payload, &mut quant, &mut quant_present)?,
            0xc4 => parse_dht(payload, &mut dc, &mut ac, &mut huffman_remaining)?,
            0xc2 => {
                if frame.is_some() {
                    return Err("JPEG progressive: multiple frames unsupported");
                }
                frame = Some(Frame::parse(payload)?);
            }
            0xdd => {
                if payload.len() != 2 {
                    return Err("JPEG progressive: invalid interval");
                }
                restart_interval = read_u16(payload, 0)?;
            }
            0xee => {
                if payload.len() >= 12 && &payload[..5] == b"Adobe" {
                    adobe_transform = Some(payload[11]);
                }
            }
            0xda => {
                let f = frame.as_mut().ok_or("JPEG progressive: missing frame")?;
                scans += 1;
                if scans > MAX_SCANS {
                    return Err("JPEG progressive: too many scans");
                }
                let scan = parse_scan(payload, f)?;
                pos = decode_scan(
                    data,
                    pos,
                    f,
                    &scan,
                    &quant,
                    &quant_present,
                    &dc,
                    &ac,
                    restart_interval,
                    &mut remaining_visits,
                )?;
            }
            0xe0..=0xef | 0xfe => {}
            _ => return Err("JPEG progressive: unsupported mode or segment"),
        }
    }
}
