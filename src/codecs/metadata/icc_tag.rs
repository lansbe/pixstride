use super::*;
/// Find a bounded, unique ICC tag. Overlapping tag payloads are allowed by ICC;
/// pointing into the header or tag table is not accepted for conversion.
pub(crate) fn icc_tag<'a>(profile: &'a [u8], name: &[u8; 4]) -> Result<Option<&'a [u8]>, Error> {
    let count = read32(profile, 128, false)? as usize;
    if count > 4096 {
        return Err("ICC: too many tags for conversion");
    }
    let table_end = 132usize
        .checked_add(count.checked_mul(12).ok_or("ICC tag table overflow")?)
        .ok_or("ICC tag table overflow")?;
    if table_end > profile.len() {
        return Err("truncated ICC tag table");
    }
    let mut found = None;
    for i in 0..count {
        let at = 132 + i * 12;
        if &profile[at..at + 4] != name {
            continue;
        }
        if found.is_some() {
            return Err("ICC: duplicate conversion tag");
        }
        let offset = read32(profile, at + 4, false)? as usize;
        let len = read32(profile, at + 8, false)? as usize;
        let end = offset
            .checked_add(len)
            .filter(|&v| v <= profile.len())
            .ok_or("ICC: tag outside profile")?;
        if offset < table_end {
            return Err("ICC: conversion tag overlaps header");
        }
        found = Some(&profile[offset..end]);
    }
    Ok(found)
}
pub(crate) fn icc_fixed(data: &[u8], at: usize) -> Result<f64, Error> {
    Ok((read32(data, at, false)? as i32) as f64 / 65536.0)
}
pub(crate) fn icc_xyz(tag: &[u8]) -> Result<[f64; 3], Error> {
    if tag.len() < 20 || tag.get(..8) != Some(b"XYZ \0\0\0\0") {
        return Err("ICC: invalid XYZ colorant");
    }
    Ok([icc_fixed(tag, 8)?, icc_fixed(tag, 12)?, icc_fixed(tag, 16)?])
}
/// Build an 8-bit input table from ICC curveType or parametricCurveType.
/// The  image pipeline decodes 8-bit RGBA; float/HDR input is not supported.
pub(crate) fn icc_curve_table(tag: &[u8]) -> Result<[f64; 256], Error> {
    if tag.len() < 12 || tag[4..8] != [0, 0, 0, 0] {
        return Err("ICC: invalid tone curve");
    }
    let mut table = [0f64; 256];
    if tag.get(..4) == Some(b"curv") {
        let count = read32(tag, 8, false)? as usize;
        let end = 12usize
            .checked_add(count.checked_mul(2).ok_or("ICC: tone curve overflow")?)
            .ok_or("ICC: tone curve overflow")?;
        if end > tag.len() {
            return Err("ICC: truncated tone curve");
        }
        let gamma = if count == 1 {
            f64::from(read16(tag, 12, false)?) / 256.0
        } else {
            1.0
        };
        if gamma <= 0.0 {
            return Err("ICC: invalid curve gamma");
        }
        for (i, v) in table.iter_mut().enumerate() {
            let x = i as f64 / 255.0;
            *v = match count {
                0 => x,
                1 => x.powf(gamma),
                _ => {
                    let at = x * (count - 1) as f64;
                    let lo = at.floor() as usize;
                    let hi = (lo + 1).min(count - 1);
                    let a = f64::from(read16(tag, 12 + lo * 2, false)?) / 65535.0;
                    let b = f64::from(read16(tag, 12 + hi * 2, false)?) / 65535.0;
                    a + (b - a) * (at - lo as f64)
                }
            };
        }
    } else if tag.get(..4) == Some(b"para") {
        if tag[10..12] != [0, 0] {
            return Err("ICC: invalid parametric curve");
        }
        let kind = read16(tag, 8, false)?;
        let count = match kind {
            0 => 1,
            1 => 3,
            2 => 4,
            3 => 5,
            4 => 7,
            _ => return Err("ICC: unsupported parametric curve"),
        };
        if tag.len() < 12 + 4 * count {
            return Err("ICC: truncated parametric curve");
        }
        let mut p = [0f64; 7];
        for (i, value) in p.iter_mut().take(count).enumerate() {
            *value = icc_fixed(tag, 12 + i * 4)?;
        }
        let [g, a, b, c, d, e, f] = p;
        if g <= 0.0 || (kind != 0 && a <= 0.0) {
            return Err("ICC: invalid parametric curve coefficients");
        }
        for (i, value) in table.iter_mut().enumerate() {
            let x = i as f64 / 255.0;
            *value = match kind {
                0 => x.powf(g),
                1 => {
                    if x >= -b / a {
                        (a * x + b).powf(g)
                    } else {
                        0.0
                    }
                }
                2 => {
                    if x >= -b / a {
                        (a * x + b).powf(g) + c
                    } else {
                        c
                    }
                }
                3 => {
                    if x >= d {
                        (a * x + b).powf(g)
                    } else {
                        c * x
                    }
                }
                4 => {
                    if x >= d {
                        (a * x + b).powf(g) + e
                    } else {
                        c * x + f
                    }
                }
                _ => unreachable!(),
            };
        }
    } else {
        return Err("ICC: unsupported tone curve type");
    }
    for i in 0..table.len() {
        if !table[i].is_finite() {
            return Err("ICC: invalid tone curve value");
        }
        table[i] = table[i].clamp(0.0, 1.0);
        if i != 0 && table[i] + 1e-6 < table[i - 1] {
            return Err("ICC: non-monotonic tone curve");
        }
    }
    Ok(table)
}
/// Convert RGB matrix/TRC ICC profiles to 8-bit sRGB, preserving alpha.
/// Colorants are already expressed in the ICC D50 PCS, including any profile
/// chromatic adaptation; applying `chad` again would adapt twice. This handles
/// only XYZ PCS matrix profiles with curv/para TRCs and relative-colorimetric
/// clipping. ICC LUT, Lab PCS, HDR, CMYK, black-point compensation and other
/// rendering intents are explicitly unsupported. Unknown profiles are errors.
pub(crate) fn convert_to_srgb(image: &mut Image, icc: &IccProfile) -> Result<(), Error> {
    let profile = raw_rgb_icc(icc)?;
    if !matches!(profile.get(12..16), Some(b"mntr" | b"scnr" | b"spac"))
        || profile.get(20..24) != Some(b"XYZ ")
    {
        return Err("ICC: unsupported profile class or PCS");
    }
    for name in [
        b"A2B0", b"A2B1", b"A2B2", b"D2B0", b"D2B1", b"D2B2", b"D2B3", b"cicp",
    ] {
        if icc_tag(profile, name)?.is_some() {
            return Err("ICC: LUT or HDR profile unsupported");
        }
    }
    if let Some(chad) = icc_tag(profile, b"chad")? {
        if chad.len() < 44 || chad.get(..8) != Some(b"sf32\0\0\0\0") {
            return Err("ICC: invalid chromatic adaptation tag");
        }
    }
    let red = icc_xyz(icc_tag(profile, b"rXYZ")?.ok_or("ICC: missing red colorant")?)?;
    let green = icc_xyz(icc_tag(profile, b"gXYZ")?.ok_or("ICC: missing green colorant")?)?;
    let blue = icc_xyz(icc_tag(profile, b"bXYZ")?.ok_or("ICC: missing blue colorant")?)?;
    let determinant = red[0] * (green[1] * blue[2] - blue[1] * green[2])
        - green[0] * (red[1] * blue[2] - blue[1] * red[2])
        + blue[0] * (red[1] * green[2] - green[1] * red[2]);
    if determinant.abs() < 1e-8 {
        return Err("ICC: singular RGB colorants");
    }
    let curves = [
        icc_curve_table(icc_tag(profile, b"rTRC")?.ok_or("ICC: missing red tone curve")?)?,
        icc_curve_table(icc_tag(profile, b"gTRC")?.ok_or("ICC: missing green tone curve")?)?,
        icc_curve_table(icc_tag(profile, b"bTRC")?.ok_or("ICC: missing blue tone curve")?)?,
    ];
    let count = image
        .width
        .checked_mul(image.height)
        .ok_or("image dimensions overflow")?;
    if image.width == 0
        || image.height == 0
        || image.width > 100_000
        || image.height > 100_000
        || count > MAX_PIXELS
        || count.checked_mul(4) != Some(image.pixels.as_slice().len())
    {
        return Err("invalid RGBA image");
    }
    // ICC sRGB transform: PCS D50 XYZ -> linear-light sRGB (D65 adapted).
    const XYZ_D50_TO_SRGB: [[f64; 3]; 3] = [
        [3.1338561, -1.6168667, -0.4906146],
        [-0.9787684, 1.9161415, 0.0334540],
        [0.0719453, -0.2289914, 1.4052427],
    ];
    let mut matrix = [[0f64; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            let c = [red, green, blue][col];
            matrix[row][col] = XYZ_D50_TO_SRGB[row][0] * c[0]
                + XYZ_D50_TO_SRGB[row][1] * c[1]
                + XYZ_D50_TO_SRGB[row][2] * c[2];
        }
    }
    let mut encode = [0u8; 4097];
    for (i, value) in encode.iter_mut().enumerate() {
        let x = i as f64 / 4096.0;
        let y = if x <= 0.0031308 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        *value = (y * 255.0 + 0.5) as u8;
    }
    for pixel in image.pixels.as_mut_slice().as_chunks_mut::<4>().0 {
        let rgb = [
            curves[0][usize::from(pixel[0])],
            curves[1][usize::from(pixel[1])],
            curves[2][usize::from(pixel[2])],
        ];
        for channel in 0..3 {
            let linear = (matrix[channel][0] * rgb[0]
                + matrix[channel][1] * rgb[1]
                + matrix[channel][2] * rgb[2])
                .clamp(0.0, 1.0);
            pixel[channel] = encode[(linear * 4096.0 + 0.5) as usize];
        }
    }
    Ok(())
}
/// Insert an RGB ICC profile into a newly encoded JPEG using ordered APP2
/// segments. Profiles from PNG and WebP are handled after bounded extraction.
pub(crate) fn preserve_icc_jpeg(encoded: &[u8], icc: &IccProfile) -> Result<Buffer, Error> {
    if !encoded.starts_with(b"\xff\xd8") {
        return Err("ICC preservation requires JPEG output");
    }
    if jpeg_metadata(encoded, true)?.icc.is_some() {
        return Err("JPEG: ICC already present");
    }
    let raw = raw_rgb_icc(icc)?;
    const PART: usize = 65_519;
    let count = raw
        .len()
        .checked_add(PART - 1)
        .ok_or("ICC: size overflow")?
        / PART;
    if count == 0 || count > 255 {
        return Err("JPEG: ICC too large");
    }
    let extra = raw
        .len()
        .checked_add(count.checked_mul(18).ok_or("ICC: size overflow")?)
        .ok_or("ICC: size overflow")?;
    let total = encoded
        .len()
        .checked_add(extra)
        .filter(|&n| n <= MAX_FILE)
        .ok_or("JPEG: output too large")?;
    let mut result = Buffer::new(total)?;
    let dst = result.as_mut_slice();
    dst[..2].copy_from_slice(&encoded[..2]);
    let mut pos = 2;
    for (i, part) in raw.chunks(PART).enumerate() {
        dst[pos..pos + 2].copy_from_slice(b"\xff\xe2");
        dst[pos + 2..pos + 4].copy_from_slice(&((part.len() + 16) as u16).to_be_bytes());
        dst[pos + 4..pos + 16].copy_from_slice(b"ICC_PROFILE\0");
        dst[pos + 16] = (i + 1) as u8;
        dst[pos + 17] = count as u8;
        dst[pos + 18..pos + 18 + part.len()].copy_from_slice(part);
        pos += 18 + part.len();
    }
    dst[pos..].copy_from_slice(&encoded[2..]);
    Ok(result)
}
/// Add ICCP and VP8X to the simple VP8L output produced by this implementation.
/// Existing extended/animated WebP output is refused.
pub(crate) fn preserve_icc_webp(encoded: &[u8], icc: &IccProfile) -> Result<Buffer, Error> {
    if encoded.len() < 26
        || encoded.get(..4) != Some(b"RIFF")
        || encoded.get(8..16) != Some(b"WEBPVP8L")
        || encoded[20] != 0x2f
    {
        return Err("ICC preservation requires simple VP8L WebP output");
    }
    let size = u32::from_le_bytes(encoded[4..8].try_into().unwrap()) as usize;
    let chunk = u32::from_le_bytes(encoded[16..20].try_into().unwrap()) as usize;
    let expected = 20usize
        .checked_add(chunk)
        .and_then(|x| x.checked_add(chunk & 1))
        .ok_or("WebP: size overflow")?;
    if size.checked_add(8) != Some(encoded.len()) || expected != encoded.len() || chunk < 5 {
        return Err("WebP: invalid VP8L container");
    }
    let bits = u32::from_le_bytes(encoded[21..25].try_into().unwrap());
    if bits >> 29 != 0 {
        return Err("WebP: unsupported VP8L version");
    }
    let raw = raw_rgb_icc(icc)?;
    let extra = 26usize
        .checked_add(raw.len())
        .and_then(|x| x.checked_add(raw.len() & 1))
        .ok_or("WebP: size overflow")?;
    let total = encoded
        .len()
        .checked_add(extra)
        .filter(|&n| n <= MAX_FILE && n - 8 <= u32::MAX as usize)
        .ok_or("WebP: output too large")?;
    let mut output = Buffer::new(total)?;
    let dst = output.as_mut_slice();
    dst[..12].copy_from_slice(&encoded[..12]);
    dst[4..8].copy_from_slice(&((total - 8) as u32).to_le_bytes());
    dst[12..16].copy_from_slice(b"VP8X");
    dst[16..20].copy_from_slice(&10u32.to_le_bytes());
    dst[20] = 0x20 | if bits & (1 << 28) != 0 { 0x10 } else { 0 };
    dst[21..24].fill(0);
    let width_minus_one = bits & 0x3fff;
    let height_minus_one = (bits >> 14) & 0x3fff;
    dst[24..27].copy_from_slice(&width_minus_one.to_le_bytes()[..3]);
    dst[27..30].copy_from_slice(&height_minus_one.to_le_bytes()[..3]);
    dst[30..34].copy_from_slice(b"ICCP");
    dst[34..38].copy_from_slice(&(raw.len() as u32).to_le_bytes());
    dst[38..38 + raw.len()].copy_from_slice(raw);
    let end = 38 + raw.len() + (raw.len() & 1);
    if raw.len() & 1 != 0 {
        dst[end - 1] = 0;
    }
    dst[end..].copy_from_slice(&encoded[12..]);
    Ok(output)
}
