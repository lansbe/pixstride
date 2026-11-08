use super::*;
impl ParsedJpeg<'_> {
    /// This initial streaming path covers the common full-resolution 4:2:0
    /// scan. Other sampling geometries retain the established decoder.
    pub(crate) fn opaque_rgb_dimensions(&self) -> Option<(usize, usize)> {
        if self.width > 4
            && self.count == 3
            && self.max_h == 2
            && self.max_v == 2
            && self.components[0].h == 2
            && self.components[0].v == 2
            && self.components[1].h == 1
            && self.components[1].v == 1
            && self.components[2].h == 1
            && self.components[2].v == 1
        {
            Some((self.width, self.height))
        } else {
            None
        }
    }

    pub(crate) fn stream_rgb_rows<F: FnMut(usize, &[u8])>(
        mut self,
        on_row: &mut F,
    ) -> Result<(), Error> {
        if self.opaque_rgb_dimensions().is_none() {
            return Err("JPEG: streamed sampling unsupported");
        }
        for c in &self.components[..self.count] {
            if !self.quant_present[c.quant] || !self.dc[c.dc].present || !self.ac[c.ac].present {
                return Err("JPEG: missing required table");
            }
        }
        let width = self.width;
        let height = self.height;
        let stride = width.div_ceil(2);
        let chroma_height = height.div_ceil(2);
        // Each completed MCU row needs only 16 Y rows, eight chroma rows,
        // a previous/next chroma halo, one deferred Y row and one RGB row.
        let mut band = Buffer::new(width * 16)?;
        let mut chroma = Buffer::new(stride * 20)?;
        let mut pending_luma = Buffer::new(width)?;
        let mut rgb = Buffer::new(width * 3)?;
        let luma = band.as_mut_slice();
        let planes = chroma.as_mut_slice();
        let pending = pending_luma.as_mut_slice();
        let rgb = rgb.as_mut_slice();
        let plane_offset = stride * 10;
        let columns = width.div_ceil(16);
        let rows = height.div_ceil(16);
        let total = columns * rows;
        let mut bits = Entropy::new(self.data, self.pos);
        let mut restart = 0u8;
        let mut block_pixels = [0u8; 64];
        let mut pending_row = false;
        for my in 0..rows {
            let band_start = my * 16;
            let band_rows = 16.min(height - band_start);
            let chroma_rows = 8.min(chroma_height - my * 8);
            for mx in 0..columns {
                for &ci in &self.order[..self.count] {
                    let component = &mut self.components[ci];
                    for by in 0..component.v {
                        for bx in 0..component.h {
                            block(
                                &mut bits,
                                component,
                                &self.quant[component.quant],
                                &self.dc[component.dc],
                                &self.ac[component.ac],
                                &mut block_pixels,
                                1,
                            )?;
                            let px = (mx * component.h + bx) * 8;
                            let py = by * 8;
                            let (plane_width, plane_rows) = if ci == 0 {
                                (width, band_rows)
                            } else {
                                (stride, chroma_rows)
                            };
                            if px >= plane_width || py >= plane_rows {
                                continue;
                            }
                            let cols = 8.min(plane_width - px);
                            let actual_rows = 8.min(plane_rows - py);
                            for yy in 0..actual_rows {
                                let src = &block_pixels[yy * 8..yy * 8 + cols];
                                if ci == 0 {
                                    let start = (py + yy) * width + px;
                                    luma[start..start + cols].copy_from_slice(src);
                                } else {
                                    let offset = if ci == 1 { 0 } else { plane_offset };
                                    let start = offset + (1 + py + yy) * stride + px;
                                    planes[start..start + cols].copy_from_slice(src);
                                }
                            }
                        }
                    }
                }
                let index = my * columns + mx;
                if self.restart_interval != 0
                    && (index + 1).is_multiple_of(self.restart_interval)
                    && index + 1 < total
                {
                    bits.marker(0xd0 + restart)?;
                    restart = (restart + 1) % 8;
                    for c in &mut self.components[..self.count] {
                        c.predictor = 0;
                    }
                }
            }
            if my + 1 == rows {
                bits.marker(0xd9)?;
            }
            if my == 0 {
                planes.copy_within(stride..2 * stride, 0);
                planes.copy_within(
                    plane_offset + stride..plane_offset + 2 * stride,
                    plane_offset,
                );
            }
            let last = chroma_rows * stride;
            planes.copy_within(last..last + stride, last + stride);
            planes.copy_within(
                plane_offset + last..plane_offset + last + stride,
                plane_offset + last + stride,
            );
            if pending_row {
                render_420_row(pending, planes, stride, 1, rgb);
                on_row(band_start - 1, rgb);
            }
            let emit = if my + 1 < rows {
                band_rows - 1
            } else {
                band_rows
            };
            for y in 0..emit {
                render_420_row(
                    &luma[y * width..(y + 1) * width],
                    planes,
                    stride,
                    y + 2,
                    rgb,
                );
                on_row(band_start + y, rgb);
            }
            pending_row = my + 1 < rows;
            if pending_row {
                pending.copy_from_slice(&luma[(band_rows - 1) * width..band_rows * width]);
                planes.copy_within(last..last + stride, 0);
                planes.copy_within(
                    plane_offset + last..plane_offset + last + stride,
                    plane_offset,
                );
            }
        }
        Ok(())
    }

    pub(crate) fn decode_at_mode(
        mut self,
        shrink: usize,
        defer_full_ycbcr: bool,
    ) -> Result<Image, Error> {
        decode_scan(
            self.data,
            self.pos,
            self.width,
            self.height,
            self.count,
            self.max_h,
            self.max_v,
            &mut self.components,
            &self.order,
            &self.quant,
            &self.quant_present,
            &self.dc,
            &self.ac,
            self.restart_interval,
            shrink,
            defer_full_ycbcr,
        )
    }

    pub(crate) fn decode_at(self, shrink: usize) -> Result<Image, Error> {
        self.decode_at_mode(shrink, false)
    }

    pub(crate) fn can_direct_reduced_bmp(
        &self,
        shrink: usize,
        width: usize,
        height: usize,
    ) -> bool {
        shrink > 1
            && self.count == 3
            && self.width.is_multiple_of(shrink)
            && self.height.is_multiple_of(shrink)
            && self.width / shrink == width
            && self.height / shrink == height
            && self.components.iter().all(|c| {
                let factor = component_shrink(shrink, c.h, self.max_h)
                    .max(component_shrink(shrink, c.v, self.max_v));
                c.h * shrink / factor == self.max_h && c.v * shrink / factor == self.max_v
            })
    }

    pub(crate) fn into_direct_reduced_bmp(self, shrink: usize) -> Result<Buffer, Error> {
        // Restrict deferred conversion to the all-full-grid case. A caller
        // cannot accidentally expose raw YCbCr through an Image result.
        if shrink <= 1
            || !self.can_direct_reduced_bmp(shrink, self.width / shrink, self.height / shrink)
        {
            return Err("JPEG: grille BMP directe indisponible");
        }
        let image = self.decode_at_mode(shrink, true)?;
        full_ycbcr_to_bmp(&image)
    }

    pub(crate) fn into_reduced(self, shrink: usize) -> Result<Image, Error> {
        let cropped_width = self.width / shrink;
        let cropped_height = self.height / shrink;
        let decoded = self.decode_at(shrink)?;
        if decoded.width == cropped_width && decoded.height == cropped_height {
            return Ok(decoded);
        }
        // Keep the original grid correction for all fallback paths.
        let row_bytes = cropped_width * 4;
        let mut pixels = Buffer::new(row_bytes * cropped_height)?;
        for y in 0..cropped_height {
            let source = y * decoded.width * 4;
            let dest = y * row_bytes;
            pixels.as_mut_slice()[dest..dest + row_bytes]
                .copy_from_slice(&decoded.pixels.as_slice()[source..source + row_bytes]);
        }
        Ok(Image {
            width: cropped_width,
            height: cropped_height,
            pixels,
        })
    }

    pub(crate) fn into_image(self) -> Result<Image, Error> {
        self.decode_at(1)
    }
}
pub(crate) fn parse_for_transform(data: &[u8]) -> Result<ParsedJpeg<'_>, Error> {
    parse_jpeg(data, None).map(|(parsed, _)| parsed)
}
pub(crate) fn decode_impl(data: &[u8], target: Option<(usize, usize)>) -> Result<Image, Error> {
    let (parsed, shrink) = parse_jpeg(data, target)?;
    parsed.decode_at(shrink)
}
pub(crate) fn parse_for_target(
    data: &[u8],
    width: usize,
    height: usize,
) -> Result<(ParsedJpeg<'_>, usize), Error> {
    if width == 0 || height == 0 {
        return Err("JPEG: invalid output dimensions");
    }
    parse_jpeg(data, Some((width, height)))
}
pub(crate) fn parse_jpeg(
    data: &[u8],
    target: Option<(usize, usize)>,
) -> Result<(ParsedJpeg<'_>, usize), Error> {
    if data.len() > MAX_FILE {
        return Err("JPEG: file too large");
    }
    if data.len() < 4 || data[0..2] != [0xff, 0xd8] {
        return Err("JPEG: invalid signature");
    }
    let mut pos = 2usize;
    let mut quant = [[0u16; 64]; 4];
    let mut quant_present = [false; 4];
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut components = [Component::EMPTY; 3];
    let mut frame = None;
    let mut restart_interval = 0usize;
    let mut adobe_transform = None;
    loop {
        let marker = read_marker(data, &mut pos)?;
        if marker == 0xd9 {
            return Err("JPEG: no decoded image");
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            return Err("JPEG: misplaced marker");
        }
        let length = read_u16(data, pos)?;
        if length < 2 {
            return Err("JPEG: invalid segment length");
        }
        let end = pos
            .checked_add(length)
            .ok_or("JPEG: invalid segment length")?;
        if end > data.len() {
            return Err("JPEG: truncated segment");
        }
        let payload = &data[pos + 2..end];
        pos = end;
        match marker {
            0xdb => parse_dqt(payload, &mut quant, &mut quant_present)?,
            0xc4 => parse_dht(payload, &mut dc, &mut ac)?,
            0xc0 => {
                if frame.is_some() {
                    return Err("JPEG: multiple frames unsupported");
                }
                frame = Some(parse_frame(payload, &mut components)?);
            }
            0xdd => {
                if payload.len() != 2 {
                    return Err("JPEG: invalid restart interval");
                }
                restart_interval = read_u16(payload, 0)?;
            }
            0xee => {
                if payload.len() >= 12 && &payload[..5] == b"Adobe" {
                    adobe_transform = Some(payload[11]);
                }
            }
            0xda => {
                let (width, height, count, max_h, max_v) = frame.ok_or("JPEG: missing frame")?;
                let shrink = match target {
                    Some((target_width, target_height)) => {
                        let ratio = (width / target_width).min(height / target_height);
                        if ratio >= 8 {
                            8
                        } else if ratio >= 4 {
                            4
                        } else if ratio >= 2 {
                            2
                        } else {
                            1
                        }
                    }
                    None => 1,
                };
                if count == 3
                    && (adobe_transform == Some(0)
                        || adobe_transform == Some(2)
                        || (components[0].id == b'R'
                            && components[1].id == b'G'
                            && components[2].id == b'B'))
                {
                    return Err("JPEG: unsupported RGB/CMYK color space");
                }
                if count == 3 {
                    // JFIF component identifiers 1,2,3 define Y,Cb,Cr even
                    // when a legal file lists them in another frame order.
                    let old = components;
                    let mut indices = [3usize; 3];
                    for (index, component) in old.iter().enumerate() {
                        if (1..=3).contains(&component.id) {
                            indices[component.id as usize - 1] = index;
                        }
                    }
                    if indices.iter().all(|&i| i < 3) {
                        components = [old[indices[0]], old[indices[1]], old[indices[2]]];
                    }
                }
                if shrink > 1 && count == 3 {
                    let y = components[0];
                    let cb = components[1];
                    let cr = components[2];
                    let equal_chroma = cb.h == cr.h && cb.v == cr.v;
                    let full_luma = y.h == max_h && y.v == max_v;
                    let ycbcr_444 = cb.h == max_h && cb.v == max_v;
                    let ycbcr_422_or_420 =
                        cb.h * 2 == max_h && (cb.v == max_v || cb.v * 2 == max_v);
                    if !full_luma || !equal_chroma || !(ycbcr_444 || ycbcr_422_or_420) {
                        return Err("JPEG: reduced subsampling unsupported");
                    }
                }
                let order = parse_scan(payload, &mut components, count)?;
                return Ok((
                    ParsedJpeg {
                        data,
                        pos,
                        width,
                        height,
                        count,
                        max_h,
                        max_v,
                        quant,
                        quant_present,
                        dc,
                        ac,
                        components,
                        order,
                        restart_interval,
                    },
                    shrink,
                ));
            }
            0xc1..=0xcf if marker != 0xc4 && marker != 0xc8 && marker != 0xcc => {
                return Err("JPEG: only baseline mode is supported");
            }
            0xcc | 0xc8 => return Err("JPEG: arithmetic coding or extension unsupported"),
            0xe0..=0xef | 0xfe => {}
            _ => return Err("JPEG: unsupported segment"),
        }
    }
}
