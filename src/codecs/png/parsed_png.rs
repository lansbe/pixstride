use super::*;
impl ParsedPng<'_> {
    pub(crate) fn compressed(&self) -> Result<Compressed<'_>, Error> {
        if let Some(content) = self.single_idat {
            return Ok(Compressed::Borrowed(content));
        }
        let mut compressed = Buffer::new(self.idat_size)?;
        let mut dst = 0usize;
        let mut pos = 8;
        while pos < self.data.len() {
            let len = be32(&self.data[pos..]) as usize;
            let kind = &self.data[pos + 4..pos + 8];
            if kind == b"IDAT" {
                compressed.as_mut_slice()[dst..dst + len]
                    .copy_from_slice(&self.data[pos + 8..pos + 8 + len]);
                dst += len;
            }
            if kind == b"IEND" {
                break;
            }
            pos += 12 + len;
        }
        Ok(Compressed::Owned(compressed))
    }

    pub(crate) fn opaque_rgb_dimensions(&self) -> Option<(usize, usize)> {
        if self.color == 2 && self.transparent_rgb.is_none() {
            Some((self.width, self.height))
        } else {
            None
        }
    }

    /// Validate all compressed bytes, Adler and filter tags before the caller
    /// normalizes and returns the private source buffer as a PNG. This path is
    /// only legal for RGB8 without tRNS and leaves source pixels unmodified.
    pub(crate) fn validate_opaque_rgb_identity(&self) -> Result<(), Error> {
        debug_assert!(self.opaque_rgb_dimensions().is_some());
        validate_identity_color_chunks(self.data)?;
        let compressed = self.compressed()?;
        let mut unused = |_: usize, _: &[u8]| {};
        let mut target = RingInflate::<_, true>::new(self.stride, self.height, &mut unused)?;
        inflate_to(compressed.as_slice(), &mut target)
    }

    /// No row reaches the HTTP client here. The caller retains a private
    /// output until inflate, Adler and every filter have been verified.
    pub(crate) fn stream_rgb_rows<F: FnMut(usize, &[u8])>(
        &self,
        on_row: &mut F,
    ) -> Result<(), Error> {
        debug_assert_eq!(self.color, 2);
        debug_assert!(self.transparent_rgb.is_none());
        let compressed = self.compressed()?;
        let mut target = RingInflate::<_, false>::new(self.stride, self.height, on_row)?;
        inflate_to(compressed.as_slice(), &mut target)
    }

    pub(crate) fn decode_raw(self) -> Result<DecodedPng, Error> {
        let mut raw = Buffer::new(self.raw_size)?;
        {
            let compressed = self.compressed()?;
            inflate(compressed.as_slice(), raw.as_mut_slice())?;
        }
        Ok(DecodedPng {
            width: self.width,
            height: self.height,
            color: self.color,
            channels: self.channels,
            stride: self.stride,
            pixel_size: self.pixel_size,
            raw,
            palette: self.palette,
            pal_alpha: self.pal_alpha,
            palette_count: self.palette_count,
            transparent_gray: self.transparent_gray,
            transparent_rgb: self.transparent_rgb,
        })
    }

    pub(crate) fn into_decoded(self) -> Result<DecodedPng, Error> {
        let mut decoded = self.decode_raw()?;
        reconstruct(
            decoded.raw.as_mut_slice(),
            decoded.height,
            decoded.stride,
            decoded.channels,
        )?;
        Ok(decoded)
    }
}
/// Check the structure of color chunks that the RGB identity path preserves.
/// The ordinary PNG parser has already checked all bounds, CRCs and critical
/// chunk ordering. This extra pass applies only to the explicit fast identity;
/// the ICC profile's compressed contents and color semantics are not decoded.
/// PNG 3: https://www.w3.org/TR/png-3/#11Chunks
pub(crate) fn validate_identity_color_chunks(data: &[u8]) -> Result<(), Error> {
    let mut pos = SIGNATURE.len();
    let mut seen = 0u16;
    let mut past_color_position = false;
    let mut gamma = None;
    let mut chromaticity = None;
    loop {
        let len = be32(&data[pos..]) as usize;
        let end = pos + len + 12;
        let kind = &data[pos + 4..pos + 8];
        let content = &data[pos + 8..pos + 8 + len];
        if kind == b"PLTE" || kind == b"IDAT" {
            past_color_position = true;
        }
        let bit = match kind {
            b"iCCP" => 1,
            b"sRGB" => 2,
            b"gAMA" => 4,
            b"cHRM" => 8,
            b"cICP" => 16,
            b"mDCV" => 32,
            b"cLLI" => 64,
            // These names occurred in an earlier PNG 3 draft and are still
            // read by deployed decoders. Preserve their exact spelling.
            b"mDCv" => 128,
            b"cLLi" => 256,
            _ => 0,
        };
        if bit != 0 {
            if past_color_position || seen & bit != 0 {
                return Err("PNG: declaration de couleur mal placee ou dupliquee");
            }
            seen |= bit;
            match kind {
                b"iCCP" => {
                    let name_end = content
                        .iter()
                        .position(|&byte| byte == 0)
                        .ok_or("PNG: invalid ICC profile name")?;
                    if name_end == 0
                        || name_end > 79
                        || content.len() < name_end + 8
                        || content[name_end + 1] != 0
                        || content[0] == b' '
                        || content[name_end - 1] == b' '
                    {
                        return Err("PNG: invalid ICC profile");
                    }
                    let name = &content[..name_end];
                    if name
                        .iter()
                        .any(|&byte| !(32..=126).contains(&byte) && !(161..=255).contains(&byte))
                        || name.windows(2).any(|pair| pair == b"  ")
                    {
                        return Err("PNG: invalid ICC profile name");
                    }
                    validate_zlib_header(content[name_end + 2], content[name_end + 3])?;
                }
                b"sRGB" if len != 1 || content[0] > 3 => {
                    return Err("PNG: invalid sRGB declaration");
                }
                b"gAMA" => {
                    if len != 4 {
                        return Err("PNG: invalid gAMA declaration");
                    }
                    gamma = Some(be32(content));
                }
                b"cHRM" => {
                    if len != 32 {
                        return Err("PNG: invalid cHRM declaration");
                    }
                    chromaticity = Some(content);
                }
                b"cICP" if len != 4 || content[2] != 0 || content[3] > 1 => {
                    return Err("PNG: invalid cICP declaration");
                }
                b"mDCV" | b"mDCv" if len != 24 => {
                    return Err("PNG: invalid mDCV declaration");
                }
                b"cLLI" | b"cLLi" if len != 8 => {
                    return Err("PNG: invalid cLLI declaration");
                }
                _ => {}
            }
        }
        pos = end;
        if kind == b"IEND" {
            break;
        }
    }
    if seen & 32 != 0 && seen & 16 == 0 {
        return Err("PNG: mDCV requiert cICP");
    }
    if seen & 2 != 0 {
        if gamma.is_some_and(|value| value != 45455) {
            return Err("PNG: gAMA incompatible avec sRGB");
        }
        if let Some(values) = chromaticity {
            let srgb = [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000];
            if srgb
                .iter()
                .enumerate()
                .any(|(index, &expected)| be32(&values[index * 4..]) != expected)
            {
                return Err("PNG: cHRM incompatible avec sRGB");
            }
        }
    }
    Ok(())
}
/// Retain the image data and its color declarations for a byte-preserving
/// RGB identity. Color chunks remain in their original order with their
/// original bytes and CRCs, so the viewer can interpret unchanged samples.
/// The input is the private memory snapshot already fully CRC/inflate/Adler
/// validated by `parse_for_transform` and `validate_opaque_rgb_identity`.
/// Color-profile contents and metadata semantics are not interpreted here.
/// Destinations always precede sources, including when ranges overlap.
pub(crate) fn normalize_validated_identity(data: &mut [u8]) -> &[u8] {
    let mut source = SIGNATURE.len();
    let mut dest = source;
    loop {
        let len = be32(&data[source..]) as usize;
        let end = source + len + 12;
        let keep = matches!(
            &data[source + 4..source + 8],
            b"IHDR"
                | b"IDAT"
                | b"IEND"
                | b"iCCP"
                | b"sRGB"
                | b"gAMA"
                | b"cHRM"
                | b"cICP"
                | b"mDCV"
                | b"cLLI"
                | b"mDCv"
                | b"cLLi"
        );
        let finish = &data[source + 4..source + 8] == b"IEND";
        if keep {
            data.copy_within(source..end, dest);
            dest += end - source;
        }
        source = end;
        if finish {
            break;
        }
    }
    &data[..dest]
}
/// Undo filters in place, retaining the previous reconstructed row.
pub(crate) fn reconstruct(
    bytes: &mut [u8],
    height: usize,
    stride: usize,
    channels: usize,
) -> Result<(), Error> {
    let full_stride = stride + 1;
    for y in 0..height {
        let start = y * full_stride + 1;
        let filter = bytes[start - 1];
        if filter > 4 {
            return Err("PNG: filtre inconnu");
        }
        // Filter None is already reconstructed. Avoid an entire byte-wise
        // predictor loop without touching the following row's validation.
        if filter == 0 || (filter == 2 && y == 0) {
            continue;
        }
        if filter == 2 {
            // Up has no dependency between bytes in the current row. Split at
            // its start so the reconstructed previous row and destination are
            // provably disjoint, including each row's filter byte.
            let (previous, current) = bytes.split_at_mut(start);
            let above = &previous[start - full_stride..start - full_stride + stride];
            let row = &mut current[..stride];
            #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
            let mut x = 0;
            #[cfg(target_arch = "aarch64")]
            unsafe {
                use core::arch::aarch64::{vaddq_u8, vld1q_u8, vst1q_u8};
                while x + 16 <= stride {
                    // The complete 16-byte vectors stay within both rows;
                    // u8 vector addition wraps exactly like PNG reconstruction.
                    let source = vld1q_u8(row.as_ptr().add(x));
                    let upper = vld1q_u8(above.as_ptr().add(x));
                    vst1q_u8(row.as_mut_ptr().add(x), vaddq_u8(source, upper));
                    x += 16;
                }
            }
            for (value, &upper) in row[x..].iter_mut().zip(&above[x..]) {
                *value = value.wrapping_add(upper);
            }
            continue;
        }
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        if filter == 4 && y != 0 && (channels == 3 || channels == 4) {
            let (previous, current) = bytes.split_at_mut(start);
            let above = &previous[start - full_stride..start - full_stride + stride];
            let row = &mut current[..stride];
            // SAFETY: the rows are disjoint, equally sized, and the helper
            // checks every four-byte load and store. The scalar tail keeps
            // all narrow RGB rows and the final RGB pixel in bounds.
            let x = if channels == 3 {
                unsafe { paeth_row_neon::<3>(row, above) }
            } else {
                unsafe { paeth_row_neon::<4>(row, above) }
            };
            for i in x..stride {
                let left = if i >= channels { row[i - channels] } else { 0 };
                let upper_left = if i >= channels {
                    above[i - channels]
                } else {
                    0
                };
                row[i] = row[i].wrapping_add(paeth(left, above[i], upper_left));
            }
            continue;
        }
        for x in 0..stride {
            let left = if x >= channels {
                bytes[start + x - channels]
            } else {
                0
            };
            let above = if y != 0 {
                bytes[start + x - full_stride]
            } else {
                0
            };
            let upper_left = if y != 0 && x >= channels {
                bytes[start + x - full_stride - channels]
            } else {
                0
            };
            let predictor = match filter {
                1 => left,
                3 => ((u16::from(left) + u16::from(above)) / 2) as u8,
                4 => paeth(left, above, upper_left),
                _ => unreachable!(),
            };
            bytes[start + x] = bytes[start + x].wrapping_add(predictor);
        }
    }

    Ok(())
}
impl DecodedPng {
    pub(crate) fn into_image(self) -> Result<Image, Error> {
        let DecodedPng {
            width,
            height,
            color,
            channels,
            stride,
            pixel_size,
            raw,
            palette,
            pal_alpha,
            palette_count,
            transparent_gray,
            transparent_rgb,
        } = self;
        let bytes = raw.as_slice();
        let full_stride = stride + 1;
        let mut pixels = Buffer::new(pixel_size)?;
        let out = pixels.as_mut_slice();
        for y in 0..height {
            let row = &bytes[y * full_stride + 1..y * full_stride + 1 + stride];
            let row_out = &mut out[y * width * 4..(y + 1) * width * 4];
            if color == 2 && transparent_rgb.is_none() {
                opaque_rgb_to_rgba(row, row_out);
                continue;
            }
            if color == 6 {
                row_out.copy_from_slice(row);
                continue;
            }
            for x in 0..width {
                let src = x * channels;
                let dst = x * 4;
                match color {
                    0 => {
                        let g = row[src];
                        row_out[dst..dst + 4].copy_from_slice(&[
                            g,
                            g,
                            g,
                            if transparent_gray == Some(g) { 0 } else { 255 },
                        ]);
                    }
                    2 => {
                        let rgb = [row[src], row[src + 1], row[src + 2]];
                        row_out[dst..dst + 3].copy_from_slice(&rgb);
                        row_out[dst + 3] = if transparent_rgb == Some(rgb) { 0 } else { 255 };
                    }
                    3 => {
                        let index = row[src] as usize;
                        if index >= palette_count {
                            return Err("PNG: invalid palette index");
                        }
                        row_out[dst..dst + 3].copy_from_slice(&palette[index * 3..index * 3 + 3]);
                        row_out[dst + 3] = pal_alpha[index];
                    }
                    4 => {
                        let g = row[src];
                        row_out[dst..dst + 4].copy_from_slice(&[g, g, g, row[src + 1]]);
                    }
                    _ => return Err("PNG: invalid color"),
                }
            }
        }
        Ok(Image {
            width,
            height,
            pixels,
        })
    }
}
