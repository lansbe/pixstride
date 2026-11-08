use super::*;
pub(crate) fn transform<'a>(
    data: &'a mut [u8],
    width: usize,
    height: usize,
    method: &[u8],
    format: &[u8],
) -> Result<Output<'a>, Error> {
    dimensions(width, height)?;
    if crate::pipeline::needs_extended_decode(data)
        || matches!(
            format,
            b"jpg" | b"jpeg" | b"webp" | b"tif" | b"tiff" | b"ico" | b"gif"
        )
    {
        return crate::pipeline::legacy_transform(data, width, height, method, format)
            .map(Output::Owned);
    }
    let fast = method == b"fast";
    let shrink_jpeg = method == b"lanczos3-shrink" || (fast && data.starts_with(b"\xff\xd8"));
    let method = if fast { &b"lanczos3"[..] } else { method };
    if shrink_jpeg && !data.starts_with(b"\xff\xd8") {
        return Err("lanczos3-shrink requires JPEG input");
    }
    // Explicit PNG output, opaque RGB8 source and identity geometry only.
    // Probe the fixed header without treating it as validated; all structure,
    // chunk CRCs, DEFLATE, Adler and scanline filters are checked below before
    // a byte becomes visible to HTTP or to the output file.
    if fast
        && format == b"png"
        && matches!(method, b"nearest" | b"bilinear" | b"lanczos3")
        && data.len() >= 33
        && &data[..16] == b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
        && u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as usize == width
        && u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as usize == height
        && data[24] == 8
        && data[25] == 2
        && data[26] == 0
        && data[27] == 0
        && data[28] == 0
    {
        let parsed = crate::png::parse_for_transform(data)?;
        if parsed.opaque_rgb_dimensions() == Some((width, height)) {
            parsed.validate_opaque_rgb_identity()?;
            return Ok(Output::Borrowed(crate::png::normalize_validated_identity(
                data,
            )));
        }
    }
    if data.starts_with(b"BM") && format == b"bmp" {
        let view = BmpView::parse(data)?;
        if width == view.width && height == view.height {
            if method != b"nearest" && method != b"bilinear" && method != b"lanczos3" {
                return Err("unsupported resize method");
            }
            if !view.top_down && view.channels == 3 && le32(data, 10)? == 54 {
                // The raster already has the requested BGR24 order. Normalize
                // only its header and padding in the privately owned input
                // buffer. Do not forward source metadata or trailing bytes.
                let stride = view.stride;
                let len = 54 + stride * height;
                data[..54].copy_from_slice(&bmp_header(width, height, len));
                if stride != width * 3 {
                    for row in data[54..len].chunks_exact_mut(stride) {
                        row[width * 3..].fill(0);
                    }
                }
                return Ok(Output::Borrowed(&data[..len]));
            }
            return view.encode_identity().map(Output::Owned);
        }
        let image = view.decode()?;
        let resized = crate::resize::resize(image, width, height, method)?;
        return encode_bmp(&resized).map(Output::Owned);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") && format == b"bmp" {
        let parsed = crate::png::parse_for_transform(data)?;
        if let Some((source_width, source_height)) = parsed.opaque_rgb_dimensions() {
            if fast && source_width == width * 4 && source_height == height * 4 {
                return prehalve_parsed(parsed, source_width, source_height, width, height)
                    .map(Output::Owned);
            }
            // Small PNGs retain the established path; the fixed streaming
            // history and plan setup are intended for larger rasters.
            let raw_size = (source_width * 3 + 1) * source_height;
            if raw_size >= 256 * 1024
                && (method == b"nearest" || method == b"bilinear" || method == b"lanczos3")
            {
                if let Ok((bmp, bmp_stride)) = bmp_output(width, height) {
                    if let Ok(Some(mut stream)) = crate::resize::StreamRgbBmp::new(
                        source_width,
                        source_height,
                        width,
                        height,
                        method,
                        bmp,
                        bmp_stride,
                        crate::png::opaque_rgb_to_rgba,
                        pack_bgr_row,
                    ) {
                        parsed.stream_rgb_rows(&mut |y, row| stream.push(y, row))?;
                        return stream.finish().map(Output::Owned);
                    }
                }
            }
        }
        let decoded = parsed.into_decoded()?;
        if let Some((rows, source_width, source_height, stride)) = decoded.opaque_rgb_rows() {
            if method != b"nearest" && method != b"bilinear" && method != b"lanczos3" {
                return Err("unsupported resize method");
            }
            let (mut bmp, bmp_stride) = bmp_output(width, height)?;
            crate::resize::opaque_rgb_to_bmp(
                rows,
                source_width,
                source_height,
                stride,
                width,
                height,
                method,
                bmp.as_mut_slice(),
                bmp_stride,
                crate::png::opaque_rgb_to_rgba,
                pack_bgr_row,
            )?;
            return Ok(Output::Owned(bmp));
        }
        let image = decoded.into_image()?;
        let resized = crate::resize::resize(image, width, height, method)?;
        return encode_bmp(&resized).map(Output::Owned);
    }
    if data.starts_with(&[0xff, 0xd8]) && format == b"bmp" {
        if shrink_jpeg {
            let (parsed, shrink) = crate::jpeg::parse_for_target(data, width, height)?;
            // No reduced grid is available: keep the established full-size
            // streaming path instead of forcing a complete RGBA raster.
            if fast && shrink == 1 {
                return full_jpeg_bmp(parsed, width, height, b"lanczos3").map(Output::Owned);
            }
            if parsed.can_direct_reduced_bmp(shrink, width, height) {
                return parsed.into_direct_reduced_bmp(shrink).map(Output::Owned);
            }
            let image = parsed.into_reduced(shrink)?;
            let resized = crate::resize::resize(image, width, height, b"lanczos3")?;
            return encode_bmp(&resized).map(Output::Owned);
        }
        let parsed = crate::jpeg::parse_for_transform(data)?;
        return full_jpeg_bmp(parsed, width, height, method).map(Output::Owned);
    }
    let image = if shrink_jpeg {
        crate::jpeg::decode_for_target(data, width, height)?
    } else {
        decode(data)?
    };
    let resize_method = if shrink_jpeg {
        &b"lanczos3"[..]
    } else {
        method
    };
    let resized = crate::resize::resize(image, width, height, resize_method)?;
    let encoded = match format {
        b"png" => crate::png::encode(&resized),
        b"bmp" => encode_bmp(&resized),
        _ => Err("unsupported output format"),
    }?;
    Ok(Output::Owned(encoded))
}
