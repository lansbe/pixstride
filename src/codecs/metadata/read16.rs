use super::*;
pub(crate) fn read16(data: &[u8], at: usize, little: bool) -> Result<u16, Error> {
    let bytes = data
        .get(at..at.checked_add(2).ok_or("metadata offset overflow")?)
        .ok_or("truncated TIFF metadata")?;
    Ok(if little {
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_be_bytes([bytes[0], bytes[1]])
    })
}
pub(crate) fn read32(data: &[u8], at: usize, little: bool) -> Result<u32, Error> {
    let bytes = data
        .get(at..at.checked_add(4).ok_or("metadata offset overflow")?)
        .ok_or("truncated metadata")?;
    let a = [bytes[0], bytes[1], bytes[2], bytes[3]];
    Ok(if little {
        u32::from_le_bytes(a)
    } else {
        u32::from_be_bytes(a)
    })
}
/// Read IFD0 orientation/ICC and validate the linked IFD directory chain.
pub(crate) fn tiff_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    let little = match data.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err("invalid TIFF byte order"),
    };
    if read16(data, 2, little)? != 42 {
        return Err("unsupported TIFF header");
    }
    let mut offset = read32(data, 4, little)? as usize;
    if offset == 0 {
        return Ok(Metadata {
            orientation: 1,
            icc: None,
        });
    }
    let mut visited = [0usize; MAX_IFDS];
    let mut visited_count = 0;
    let mut value = None;
    let mut icc = None;
    while offset != 0 {
        if offset < 8 || visited_count == MAX_IFDS || visited[..visited_count].contains(&offset) {
            return Err("invalid or cyclic TIFF IFD chain");
        }
        visited[visited_count] = offset;
        visited_count += 1;
        let count = usize::from(read16(data, offset, little)?);
        let entries = offset.checked_add(2).ok_or("metadata offset overflow")?;
        let end = entries
            .checked_add(count.checked_mul(12).ok_or("TIFF entry overflow")?)
            .ok_or("metadata offset overflow")?;
        let next = read32(data, end, little)? as usize;
        if visited_count == 1 {
            for i in 0..count {
                let entry = entries + i * 12; // bounded by checked directory end
                if read16(data, entry, little)? == 0x0112 {
                    if value.is_some()
                        || read16(data, entry + 2, little)? != 3
                        || read32(data, entry + 4, little)? != 1
                    {
                        return Err("invalid TIFF orientation tag");
                    }
                    let orientation = read16(data, entry + 8, little)?;
                    if !(1..=8).contains(&orientation) {
                        return Err("invalid TIFF orientation value");
                    }
                    value = Some(orientation as u8);
                } else if with_icc && read16(data, entry, little)? == 0x8773 {
                    if icc.is_some() || !matches!(read16(data, entry + 2, little)?, 1 | 7) {
                        return Err("TIFF: invalid or duplicate ICC tag");
                    }
                    let len = read32(data, entry + 4, little)? as usize;
                    if !(132..=ICC_MAX).contains(&len) {
                        return Err("TIFF: ICC size outside limit");
                    }
                    let at = read32(data, entry + 8, little)? as usize;
                    let end = at.checked_add(len).ok_or("TIFF: ICC offset overflow")?;
                    let profile = data.get(at..end).ok_or("TIFF: ICC outside input")?;
                    validate_rgb_icc(profile)?;
                    icc = Some(IccProfile::RawRgb(copy_buffer(profile)?));
                }
            }
        }
        offset = next;
    }
    Ok(Metadata {
        orientation: value.unwrap_or(1),
        icc,
    })
}
pub(crate) fn tiff_orientation(data: &[u8]) -> Result<u8, Error> {
    Ok(tiff_metadata(data, false)?.orientation)
}
pub(crate) fn crc_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}
pub(crate) fn png_crc(kind: &[u8], content: &[u8]) -> u32 {
    !crc_update(crc_update(!0, kind), content)
}
pub(crate) fn copy_buffer(data: &[u8]) -> Result<Buffer, Error> {
    let mut output = Buffer::new(data.len())?;
    output.as_mut_slice().copy_from_slice(data);
    Ok(output)
}
pub(crate) fn valid_iccp(content: &[u8]) -> Result<(), Error> {
    let end = content
        .iter()
        .position(|&x| x == 0)
        .ok_or("PNG: invalid iCCP name")?;
    if end == 0
        || end > 79
        || content.len() < end + 8
        || content[0] == b' '
        || content[end - 1] == b' '
        || content[..end].windows(2).any(|pair| pair == b"  ")
        || content[..end]
            .iter()
            .any(|&b| !(32..=126).contains(&b) && !(161..=255).contains(&b))
        || content[end + 1] != 0
    {
        return Err("PNG: invalid iCCP header");
    }
    let cmf = content[end + 2];
    let flg = content[end + 3];
    if cmf & 15 != 8
        || cmf >> 4 > 7
        || flg & 32 != 0
        || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0
    {
        return Err("PNG: invalid iCCP zlib header");
    }
    Ok(())
}
pub(crate) fn png_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    let mut pos = 8usize;
    let mut orientation = 1;
    let mut seen_exif = false;
    let mut icc = None;
    let mut rgb = false;
    let mut first = true;
    let mut seen_idat = false;
    loop {
        let len = read32(data, pos, false)? as usize;
        if len > 0x7fff_ffff {
            return Err("PNG: invalid chunk length");
        }
        let start = pos.checked_add(8).ok_or("PNG: chunk overflow")?;
        let end = start.checked_add(len).ok_or("PNG: chunk overflow")?;
        let next = end.checked_add(4).ok_or("PNG: chunk overflow")?;
        let kind = data
            .get(pos + 4..start)
            .ok_or("PNG: truncated chunk type")?;
        let content = data.get(start..end).ok_or("PNG: truncated chunk")?;
        let expected_crc = read32(data, end, false)?;
        if first {
            if kind != b"IHDR" || len != 13 {
                return Err("PNG: invalid IHDR");
            }
            rgb = matches!(content[9], 2 | 3 | 6);
            first = false;
        }
        if matches!(kind, b"IHDR" | b"eXIf" | b"iCCP") && png_crc(kind, content) != expected_crc {
            return Err("PNG: metadata CRC mismatch");
        }
        match kind {
            b"eXIf" => {
                if seen_exif {
                    return Err("PNG: duplicate eXIf");
                }
                seen_exif = true;
                let embedded = tiff_metadata(content, with_icc)?;
                orientation = embedded.orientation;
                if let Some(profile) = embedded.icc {
                    if !rgb || icc.is_some() {
                        return Err("PNG: conflicting EXIF ICC profile");
                    }
                    icc = Some(profile);
                }
            }
            b"iCCP" if with_icc => {
                if icc.is_some() || seen_idat || len > ICC_MAX {
                    return Err("PNG: invalid iCCP placement or size");
                }
                valid_iccp(content)?;
                if !rgb {
                    return Err("PNG: grayscale ICC cannot be preserved on RGB output");
                }
                let name_end = content
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or("PNG: invalid iCCP name")?;
                let profile =
                    crate::png::inflate_icc_bounded(&content[name_end + 2..], 4 * 1024 * 1024)?;
                validate_rgb_icc(profile.as_slice())?;
                icc = Some(IccProfile::RawRgb(profile));
            }
            b"IDAT" => seen_idat = true,
            b"IEND" => {
                if len != 0 || next != data.len() {
                    return Err("PNG: invalid IEND");
                }
                return Ok(Metadata { orientation, icc });
            }
            _ => {}
        }
        pos = next;
    }
}
pub(crate) fn validate_rgb_icc(profile: &[u8]) -> Result<(), Error> {
    if profile.len() < 132
        || profile.len() > ICC_MAX
        || read32(profile, 0, false)? as usize != profile.len()
        || profile.get(16..20) != Some(b"RGB ")
        || profile.get(36..40) != Some(b"acsp")
    {
        return Err("unsupported or malformed RGB ICC profile");
    }
    // Header/tag-table framing only. Full profile semantics are not interpreted.
    let count = read32(profile, 128, false)? as usize;
    let table_end = 132usize
        .checked_add(count.checked_mul(12).ok_or("ICC tag table overflow")?)
        .ok_or("ICC tag table overflow")?;
    if table_end > profile.len() {
        return Err("truncated ICC tag table");
    }
    for i in 0..count {
        let at = 132 + i * 12;
        let offset = read32(profile, at + 4, false)? as usize;
        let len = read32(profile, at + 8, false)? as usize;
        if offset
            .checked_add(len)
            .filter(|&end| end <= profile.len())
            .is_none()
        {
            return Err("ICC tag outside profile");
        }
    }
    Ok(())
}
pub(crate) fn jpeg_metadata(data: &[u8], with_icc: bool) -> Result<Metadata, Error> {
    let mut pos = 2usize;
    let mut orientation = 1;
    let mut seen_exif = false;
    let mut exif_icc = None;
    let mut count = 0usize;
    let mut parts = [(0usize, 0usize); 255];
    let mut total = 0usize;
    loop {
        if data.get(pos) != Some(&0xff) {
            return Err("JPEG: invalid metadata marker");
        }
        while data.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *data.get(pos).ok_or("JPEG: truncated metadata marker")?;
        pos += 1;
        if marker == 0xd9 {
            break;
        }
        if marker == 0x00 {
            return Err("JPEG: invalid marker");
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xd8 {
            return Err("JPEG: unexpected SOI");
        }
        let len = usize::from(read16(data, pos, false)?);
        if len < 2 {
            return Err("JPEG: invalid marker length");
        }
        let end = pos.checked_add(len).ok_or("JPEG: metadata size overflow")?;
        let payload = data
            .get(pos + 2..end)
            .ok_or("JPEG: truncated metadata segment")?;
        if marker == 0xe1 && payload.starts_with(b"Exif\0\0") {
            if seen_exif {
                return Err("JPEG: duplicate EXIF");
            }
            seen_exif = true;
            let embedded = tiff_metadata(&payload[6..], with_icc)?;
            orientation = embedded.orientation;
            exif_icc = embedded.icc;
        }
        if with_icc && marker == 0xe2 && payload.starts_with(b"ICC_PROFILE\0") {
            if payload.len() < 14 {
                return Err("JPEG: truncated ICC marker");
            }
            let seq = usize::from(payload[12]);
            let current_count = usize::from(payload[13]);
            if seq == 0
                || current_count == 0
                || seq > current_count
                || (count != 0 && count != current_count)
                || parts[seq - 1].1 != 0
            {
                return Err("JPEG: invalid ICC sequence");
            }
            let begin = pos + 2 + 14;
            let n = payload.len() - 14;
            if n == 0 {
                return Err("JPEG: empty ICC segment");
            }
            total = total
                .checked_add(n)
                .filter(|&n| n <= ICC_MAX)
                .ok_or("JPEG: ICC too large")?;
            count = current_count;
            parts[seq - 1] = (begin, n);
        }
        pos = end;
        if marker == 0xda {
            break;
        } // later scan metadata is outside this implementation
    }
    let icc = if count == 0 {
        exif_icc
    } else {
        if exif_icc.is_some() {
            return Err("JPEG: conflicting EXIF ICC profile");
        }
        let mut assembled = Buffer::new(total)?;
        let mut at = 0;
        for &(begin, n) in &parts[..count] {
            if n == 0 {
                return Err("JPEG: missing ICC segment");
            }
            assembled.as_mut_slice()[at..at + n].copy_from_slice(&data[begin..begin + n]);
            at += n;
        }
        validate_rgb_icc(assembled.as_slice())?;
        Some(IccProfile::RawRgb(assembled))
    };
    Ok(Metadata { orientation, icc })
}
