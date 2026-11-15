//! Validate unchanged ordinary RGB PNGs inside the isolated worker.
use super::{engine::NativeResult, scale::Plan};
use crate::{
    ops,
    request::{OutputFormat, Request},
    Error,
};

pub(super) fn try_process(
    data: &mut Vec<u8>,
    r: &Request,
    mark: &[u8],
) -> Result<Option<NativeResult>, Error> {
    let o = &r.options;
    let e = &r.extra;
    if r.format != Some(OutputFormat::Png)
        || !r.strip_metadata
        || !r.strip_color_profile
        || e.keep_copyright
        || e.preserve_hdr
        || e.raw
        || !e.skip.is_empty()
        || !e.force_formats.is_empty()
        || e.base_opacity != 1.0
        || e.encoding.png_quantize
        || e.encoding.png_interlaced
        || e.encoding.linear
        || o.crop.is_some()
        || o.rotation != ops::Rotation::R0
        || o.flip_horizontal
        || o.flip_vertical
        || o.padding != Default::default()
        || o.background.is_some()
        || o.watermark.is_some()
        || r.watermark.is_some()
        || !mark.is_empty()
        || e.extend
        || e.extend_aspect
        || e.fill_down
        || e.smart
        || e.enforce_thumbnail
        || r.effects.blur_sigma != 0.0
        || r.effects.sharpen_sigma != 0.0
        || r.effects.pixelate > 1
        || r.effects.trim.is_some()
        || data.len() < 33
        || &data[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"
        || data[24..29] != [8, 2, 0, 0, 0]
    {
        return Ok(None);
    }
    if data.len() > e.max_source_bytes.min(crate::MAX_FILE) {
        return Err("PNG source exceeds byte limit");
    }
    let dims = [
        u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize,
        u32::from_be_bytes(data[20..24].try_into().unwrap()) as usize,
    ];
    crate::image::dimensions(dims[0], dims[1])?;
    let pixels = dims[0]
        .checked_mul(dims[1])
        .ok_or("PNG dimensions overflow")?;
    if pixels > e.max_source_pixels || pixels > e.max_frame_pixels {
        return Err("PNG source pixel budget exceeded");
    }
    if pixels * 3 + dims[1] > crate::MAX_FILE {
        return Ok(None);
    }
    let p = Plan::new(dims[0] as i32, dims[1] as i32, r, false)?;
    if p.scale != [1.0; 2] || (0..2).any(|a| p.target[a] > 0 && (p.target[a] as usize) < dims[a]) {
        return Ok(None);
    }
    let mut pos = 8;
    let end = loop {
        let header = data.get(pos..pos + 8).ok_or("truncated PNG chunk")?;
        let len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = pos
            .checked_add(len)
            .and_then(|n| n.checked_add(12))
            .filter(|n| *n <= data.len())
            .ok_or("truncated PNG chunk")?;
        let kind = &header[4..8];
        if !matches!(kind, b"IHDR" | b"IDAT" | b"IEND") {
            return Ok(None);
        }
        if kind == b"IEND" {
            break end;
        }
        pos = end;
    };
    if end != data.len() || (e.max_bytes > 0 && end > e.max_bytes) {
        return Ok(None);
    }
    {
        let parsed = crate::png::parse_for_transform(data)?;
        if parsed.opaque_rgb_dimensions() != Some((dims[0], dims[1])) {
            return Ok(None);
        }
        parsed.validate_opaque_rgb_identity()?;
    }
    // Full structure, CRC, DEFLATE, Adler and every filter have now passed.
    // The owned private frame can move to the response without a raster or copy.
    data.truncate(end);
    Ok(Some(NativeResult {
        body: std::mem::take(data),
        dimensions: dims,
        origin: dims,
        format: OutputFormat::Png,
    }))
}

#[cfg(test)]
mod tests {
    use crate::request::{self, Config};

    fn source() -> Vec<u8> {
        crate::png_encode::encode_pixels(2, 1, [1, 2, 3, 4, 5, 6], 3)
            .unwrap()
            .as_slice()
            .to_vec()
    }
    fn request(options: &str) -> request::Request {
        request::parse(
            &format!("/unsafe/{options}/plain/local:///fixture.png@png"),
            &Config::unsafe_local(),
        )
        .unwrap()
    }
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32fast::hash(&out[4..]).to_be_bytes());
        out
    }
    #[test]
    fn identity_moves_validated_frame_and_trailing_bytes_keep_native_decoder() {
        let expected = source();
        let mut input = expected.clone();
        let pointer = input.as_ptr();
        let result = super::try_process(&mut input, &request(""), &[])
            .unwrap()
            .unwrap();
        assert_eq!(result.body, expected);
        assert_eq!(result.body.as_ptr(), pointer);
        assert!(input.is_empty());
        assert_eq!(result.dimensions, [2, 1]);
        let mut trailing = expected;
        trailing.extend_from_slice(b"private trailing bytes");
        assert!(super::try_process(&mut trailing, &request(""), &[])
            .unwrap()
            .is_none());
    }
    #[test]
    fn complete_stream_crc_adler_filter_and_source_limits_are_mandatory() {
        let original = source();
        let mut bad = original.clone();
        let end = 41 + u32::from_be_bytes(bad[33..37].try_into().unwrap()) as usize;
        bad[end - 1] ^= 1;
        let crc = crc32fast::hash(&bad[37..end]);
        bad[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        let mut wrong_crc = original.clone();
        *wrong_crc.last_mut().unwrap() ^= 1;
        let raw = [5u8, 1, 2, 3, 4, 5, 6];
        let mut zlib = vec![0x78, 1, 1, 7, 0, !7u8, 255];
        zlib.extend_from_slice(&raw);
        let (mut a, mut b) = (1u32, 0u32);
        for n in raw {
            a = (a + u32::from(n)) % 65521;
            b = (b + a) % 65521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut invalid_filter = original[..33].to_vec();
        invalid_filter.extend(chunk(b"IDAT", &zlib));
        invalid_filter.extend(chunk(b"IEND", &[]));
        for mut input in [
            bad,
            wrong_crc,
            invalid_filter,
            original[..original.len() - 1].to_vec(),
        ] {
            assert!(super::try_process(&mut input, &request(""), &[]).is_err());
        }
        for frame_limit in [false, true] {
            let mut r = request("");
            if frame_limit {
                r.extra.max_frame_pixels = 1;
            } else {
                r.extra.max_source_pixels = 1;
            }
            assert!(super::try_process(&mut source(), &r, &[]).is_err());
        }
    }
    #[test]
    fn metadata_alpha_geometry_and_encoding_options_keep_native_processing() {
        for kind in [b"tEXt", b"gAMA", b"iCCP", b"eXIf", b"acTL"] {
            let src = source();
            let mut input = src[..33].to_vec();
            input.extend(chunk(kind, b"synthetic"));
            input.extend_from_slice(&src[33..]);
            assert!(super::try_process(&mut input, &request(""), &[])
                .unwrap()
                .is_none());
        }
        let mut alpha = crate::png_encode::encode_pixels(2, 1, [1, 2, 3, 128, 4, 5, 6, 0], 4)
            .unwrap()
            .as_slice()
            .to_vec();
        assert!(super::try_process(&mut alpha, &request(""), &[])
            .unwrap()
            .is_none());
        for options in [
            "rs:fit:1:1",
            "rot:90",
            "bl:1",
            "sh:1",
            "bg:ffffff",
            "c:1:1:ce",
            "ph:1",
        ] {
            assert!(
                super::try_process(&mut source(), &request(options), &[])
                    .unwrap()
                    .is_none(),
                "{options}"
            );
        }
        let mut r = request("");
        r.strip_metadata = false;
        assert!(super::try_process(&mut source(), &r, &[])
            .unwrap()
            .is_none());
        r = request("");
        r.extra.encoding.png_interlaced = true;
        assert!(super::try_process(&mut source(), &r, &[])
            .unwrap()
            .is_none());
        assert!(super::try_process(&mut source(), &request(""), &[1])
            .unwrap()
            .is_none());
    }
    #[test]
    fn byte_target_equal_to_original_succeeds_and_smaller_retains_encoder() {
        let src = source();
        let mut r = request("");
        r.extra.max_bytes = src.len();
        assert_eq!(
            super::try_process(&mut src.clone(), &r, &[])
                .unwrap()
                .unwrap()
                .body,
            src
        );
        r.extra.max_bytes -= 1;
        let mut input = src.clone();
        assert!(super::try_process(&mut input, &r, &[]).unwrap().is_none());
        assert_eq!(input, src);
    }
}
