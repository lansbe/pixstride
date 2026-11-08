use super::*;
pub(crate) fn webp_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    if data.len() < 20 || data.get(8..12) != Some(b"WEBP") {
        return Err("WebP: invalid RIFF header");
    }
    let end = (u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize)
        .checked_add(8)
        .ok_or("WebP: RIFF overflow")?;
    if end != data.len() || end > MAX_FILE {
        return Err("WebP: invalid RIFF length");
    }
    let mut pos = 12;
    let mut orientation = 1;
    let mut seen_exif = false;
    let mut seen_icc = false;
    let mut seen_image = false;
    let mut flags = None;
    let mut icc = None;
    while pos < end {
        let header_end = pos.checked_add(8).ok_or("WebP: chunk overflow")?;
        let header = data.get(pos..header_end).ok_or("WebP: truncated chunk")?;
        let kind = &header[..4];
        let n = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let stop = header_end.checked_add(n).ok_or("WebP: chunk overflow")?;
        let next = stop.checked_add(n & 1).ok_or("WebP: chunk overflow")?;
        if next > end || (n & 1 != 0 && data[stop] != 0) {
            return Err("WebP: invalid chunk length or padding");
        }
        let content = &data[header_end..stop];
        match kind {
            b"VP8X" => {
                if pos != 12
                    || flags.is_some()
                    || n != 10
                    || content[0] & 0xc3 != 0
                    || content[1..4] != [0, 0, 0]
                {
                    return Err("WebP: invalid extended header");
                }
                flags = Some(content[0]);
            }
            b"ICCP" => {
                if flags.is_none() || seen_icc || seen_image || n > ICC_MAX {
                    return Err("WebP: invalid ICCP placement or size");
                }
                seen_icc = true;
                if with_icc {
                    if icc.is_some() {
                        return Err("WebP: conflicting EXIF ICC profile");
                    }
                    validate_rgb_icc(content)?;
                    icc = Some(IccProfile::RawRgb(copy_buffer(content)?));
                }
            }
            b"EXIF" => {
                if flags.is_none() || seen_exif {
                    return Err("WebP: invalid EXIF chunk");
                }
                seen_exif = true;
                let tiff = content.strip_prefix(b"Exif\0\0").unwrap_or(content);
                let embedded = tiff_metadata(tiff, with_icc)?;
                orientation = embedded.orientation;
                if let Some(profile) = embedded.icc {
                    if icc.is_some() {
                        return Err("WebP: conflicting EXIF ICC profile");
                    }
                    icc = Some(profile);
                }
            }
            b"VP8L" | b"VP8 " | b"ANIM" | b"ANMF" => seen_image = true,
            _ => {}
        }
        pos = next;
    }
    if let Some(bits) = flags {
        if (bits & 0x20 != 0) != seen_icc || (bits & 0x08 != 0) != seen_exif {
            return Err("WebP: metadata flag mismatch");
        }
    }
    Ok(Metadata { orientation, icc })
}
/// Select the exact largest ICO entry using the same rank as the ICO decoder.
/// Embedded PNG metadata is inspected; embedded/linked DIB ICC is refused.
pub(crate) fn ico_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    let read_le16 = |at: usize| -> Result<u16, Error> {
        let b = data
            .get(at..at.checked_add(2).ok_or("ICO: offset overflow")?)
            .ok_or("ICO: truncated")?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    };
    let read_le32 = |at: usize| -> Result<u32, Error> {
        let b = data
            .get(at..at.checked_add(4).ok_or("ICO: offset overflow")?)
            .ok_or("ICO: truncated")?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if read_le16(0)? != 0 || read_le16(2)? != 1 {
        return Err("ICO: invalid header");
    }
    let count = usize::from(read_le16(4)?);
    if count == 0 || count > 256 {
        return Err("ICO: invalid image count");
    }
    let dir_end = 6usize
        .checked_add(count.checked_mul(16).ok_or("ICO: directory overflow")?)
        .ok_or("ICO: directory overflow")?;
    if dir_end > data.len() {
        return Err("ICO: truncated directory");
    }
    let mut selected: Option<(usize, usize, bool, &[u8])> = None;
    for index in 0..count {
        let at = 6 + index * 16;
        let width = if data[at] == 0 {
            256
        } else {
            usize::from(data[at])
        };
        let height = if data[at + 1] == 0 {
            256
        } else {
            usize::from(data[at + 1])
        };
        let bpp = usize::from(read_le16(at + 6)?);
        let size = read_le32(at + 8)? as usize;
        let offset = read_le32(at + 12)? as usize;
        if size == 0 || offset < dir_end {
            return Err("ICO: invalid image offset");
        }
        let end = offset.checked_add(size).ok_or("ICO: image overflow")?;
        let bytes = data.get(offset..end).ok_or("ICO: truncated image")?;
        let png = bytes.starts_with(PNG_SIGNATURE);
        let rank = (width * height, bpp, png);
        if selected.as_ref().is_none_or(|v| rank > (v.0, v.1, v.2)) {
            selected = Some((rank.0, bpp, png, bytes));
        }
    }
    let (_, _, png, bytes) = selected.ok_or("ICO: missing image")?;
    if png {
        return png_metadata(bytes, with_icc);
    }
    if bytes.len() < 4 {
        return Err("ICO: truncated DIB");
    }
    let header = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    if header > bytes.len() {
        return Err("ICO: truncated DIB header");
    }
    if with_icc && header >= 124 {
        let colorspace = u32::from_le_bytes(bytes[56..60].try_into().unwrap());
        let profile_size = u32::from_le_bytes(bytes[116..120].try_into().unwrap());
        if profile_size != 0 || matches!(colorspace, 0x4d42_4544 | 0x4c49_4e4b) {
            return Err("ICO: embedded or linked DIB ICC unsupported");
        }
    }
    Ok(Metadata {
        orientation: 1,
        icc: None,
    })
}
/// Inspect embedded BMP V5 ICC data without following linked profile paths.
pub(crate) fn bmp_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    if data.len() < 18 {
        return Err("BMP: truncated header");
    }
    let header = u32::from_le_bytes(data[14..18].try_into().unwrap()) as usize;
    let end = 14usize.checked_add(header).ok_or("BMP: header overflow")?;
    if end > data.len() {
        return Err("BMP: truncated header");
    }
    let mut icc = None;
    if with_icc && header >= 124 {
        let colorspace = u32::from_le_bytes(data[70..74].try_into().unwrap());
        let relative = u32::from_le_bytes(data[126..130].try_into().unwrap()) as usize;
        let len = u32::from_le_bytes(data[130..134].try_into().unwrap()) as usize;
        if colorspace == 0x4c49_4e4b {
            return Err("BMP: linked ICC profile unsupported");
        }
        if len != 0 || colorspace == 0x4d42_4544 {
            if colorspace != 0x4d42_4544 || !(132..=ICC_MAX).contains(&len) || relative < header {
                return Err("BMP: invalid embedded ICC profile");
            }
            let at = 14usize
                .checked_add(relative)
                .ok_or("BMP: ICC offset overflow")?;
            let end = at.checked_add(len).ok_or("BMP: ICC offset overflow")?;
            let profile = data.get(at..end).ok_or("BMP: ICC outside input")?;
            validate_rgb_icc(profile)?;
            icc = Some(IccProfile::RawRgb(copy_buffer(profile)?));
        }
    }
    Ok(Metadata {
        orientation: 1,
        icc,
    })
}
/// Inspect metadata without decoding image pixels or converting a color space.
pub(crate) fn parse(data: &[u8]) -> Result<Metadata, Error> {
    if data.starts_with(PNG_SIGNATURE) {
        png_metadata(data, true)
    } else if data.starts_with(b"\xff\xd8") {
        jpeg_metadata(data, true)
    } else if data.starts_with(b"RIFF") {
        webp_metadata(data, true)
    } else if data.starts_with(b"II") || data.starts_with(b"MM") {
        tiff_metadata(data, true)
    } else if data.starts_with(b"\0\0\x01\0") {
        ico_metadata(data, true)
    } else if data.starts_with(b"BM") {
        bmp_metadata(data, true)
    } else {
        Ok(Metadata {
            orientation: 1,
            icc: None,
        })
    }
}
/// Read EXIF orientation only; no ICC allocation is made.
pub(crate) fn orientation(data: &[u8]) -> Result<u8, Error> {
    if data.starts_with(PNG_SIGNATURE) {
        Ok(png_metadata(data, false)?.orientation)
    } else if data.starts_with(b"\xff\xd8") {
        Ok(jpeg_metadata(data, false)?.orientation)
    } else if data.starts_with(b"RIFF") {
        Ok(webp_metadata(data, false)?.orientation)
    } else if data.starts_with(b"II") || data.starts_with(b"MM") {
        tiff_orientation(data)
    } else if data.starts_with(b"\0\0\x01\0") {
        Ok(ico_metadata(data, false)?.orientation)
    } else {
        Ok(1)
    }
}
pub(crate) fn extract_icc(data: &[u8]) -> Result<Option<IccProfile>, Error> {
    Ok(parse(data)?.icc)
}
/// Apply EXIF orientation once, returning ordinary top-left RGBA pixels.
pub(crate) fn orient(image: Image, orientation: u8) -> Result<Image, Error> {
    if !(1..=8).contains(&orientation) {
        return Err("invalid image orientation");
    }
    let w = image.width;
    let h = image.height;
    let pixels = w.checked_mul(h).ok_or("image dimensions overflow")?;
    let bytes = pixels.checked_mul(4).ok_or("image size overflow")?;
    if w == 0
        || h == 0
        || w > 100_000
        || h > 100_000
        || pixels > MAX_PIXELS
        || image.pixels.as_slice().len() != bytes
    {
        return Err("invalid RGBA image");
    }
    if orientation == 1 {
        return Ok(image);
    }
    let (out_w, out_h) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut output = Buffer::new(bytes)?;
    let input = image.pixels.as_slice();
    let out = output.as_mut_slice();
    for y in 0..out_h {
        for x in 0..out_w {
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                8 => (w - 1 - y, x),
                _ => unreachable!(),
            };
            let si = (sy * w + sx) * 4;
            let di = (y * out_w + x) * 4;
            out[di..di + 4].copy_from_slice(&input[si..si + 4]);
        }
    }
    Ok(Image {
        width: out_w,
        height: out_h,
        pixels: output,
    })
}
pub(crate) fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for group in bytes.chunks(5552) {
        for &byte in group {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}
/// Insert an extracted RGB-compatible ICC declaration into newly encoded PNG.
/// RGB ICC is stored in a zlib/DEFLATE stream using uncompressed blocks. This
/// preserves the decoded profile bytes; it does not transform pixel colors or
/// perform a semantic ICC conversion.
pub(crate) fn preserve_icc_png(encoded: &[u8], icc: &IccProfile) -> Result<Buffer, Error> {
    if !encoded.starts_with(PNG_SIGNATURE)
        || encoded.len() < 45
        || read32(encoded, 8, false)? != 13
        || encoded.get(12..16) != Some(b"IHDR")
        || !matches!(encoded[25], 2 | 6)
    {
        return Err("ICC preservation requires RGB/RGBA PNG output");
    }
    // The encoder output is expected to have no conflicting color declaration.
    let mut pos = 8;
    let mut ended = false;
    while pos < encoded.len() {
        let n = read32(encoded, pos, false)? as usize;
        let end = pos
            .checked_add(n)
            .and_then(|x| x.checked_add(12))
            .ok_or("PNG: size overflow")?;
        let kind = encoded
            .get(pos + 4..pos + 8)
            .ok_or("PNG: truncated chunk")?;
        if end > encoded.len() {
            return Err("PNG: truncated chunk");
        }
        if matches!(kind, b"iCCP" | b"sRGB" | b"cICP") {
            return Err("PNG: conflicting color metadata");
        }
        if kind == b"IEND" {
            if n != 0 || end != encoded.len() {
                return Err("PNG: invalid IEND");
            }
            ended = true;
            break;
        }
        pos = end;
    }
    if !ended {
        return Err("PNG: missing IEND");
    }
    let raw = raw_rgb_icc(icc)?;
    let blocks = raw.len().checked_add(65534).ok_or("ICC: size overflow")? / 65535;
    let zlib_len = raw
        .len()
        .checked_add(blocks.checked_mul(5).ok_or("ICC: size overflow")?)
        .and_then(|x| x.checked_add(6))
        .ok_or("ICC: size overflow")?;
    let content_len = 5usize.checked_add(zlib_len).ok_or("ICC: size overflow")?;
    let size = encoded
        .len()
        .checked_add(12)
        .and_then(|n| n.checked_add(content_len))
        .filter(|&n| n <= MAX_FILE && content_len <= u32::MAX as usize)
        .ok_or("PNG: output too large")?;
    let mut result = Buffer::new(size)?;
    let output = result.as_mut_slice();
    output[..33].copy_from_slice(&encoded[..33]);
    output[33..37].copy_from_slice(&(content_len as u32).to_be_bytes());
    output[37..41].copy_from_slice(b"iCCP");
    let mut p = 41;
    output[p..p + 5].copy_from_slice(b"ICC\0\0");
    p += 5;
    output[p..p + 2].copy_from_slice(&[0x78, 0x01]);
    p += 2;
    for (index, block) in raw.chunks(65535).enumerate() {
        let n = block.len() as u16;
        output[p] = u8::from(index * 65535 + block.len() == raw.len());
        output[p + 1..p + 3].copy_from_slice(&n.to_le_bytes());
        output[p + 3..p + 5].copy_from_slice(&(!n).to_le_bytes());
        p += 5;
        output[p..p + block.len()].copy_from_slice(block);
        p += block.len();
    }
    output[p..p + 4].copy_from_slice(&adler32(raw).to_be_bytes());
    p += 4;
    if p != 41 + content_len {
        return Err("ICC: internal size mismatch");
    }
    let crc = png_crc(b"iCCP", &output[41..p]);
    output[p..p + 4].copy_from_slice(&crc.to_be_bytes());
    p += 4;
    output[p..].copy_from_slice(&encoded[33..]);
    Ok(result)
}
pub(crate) fn raw_rgb_icc(icc: &IccProfile) -> Result<&[u8], Error> {
    match icc {
        IccProfile::RawRgb(profile) => {
            validate_rgb_icc(profile.as_slice())?;
            Ok(profile.as_slice())
        }
    }
}
