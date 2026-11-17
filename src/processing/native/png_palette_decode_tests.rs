use super::*;
use crate::request::{self, Config};

fn request() -> Request {
    request::parse(
        "/unsafe/plain/local:///indexed.png@png",
        &Config::unsafe_local(),
    )
    .unwrap()
}

fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let crc = crc32fast::hash(&output[output.len() - data.len() - 4..]);
    output.extend_from_slice(&crc.to_be_bytes());
}

fn fixture(depth: u8, entries: usize, transparency: bool) -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = [0; 13];
    ihdr[..4].copy_from_slice(&3u32.to_be_bytes());
    ihdr[4..8].copy_from_slice(&2u32.to_be_bytes());
    ihdr[8] = depth;
    ihdr[9] = 3;
    chunk(&mut output, b"IHDR", &ihdr);
    let palette: Vec<u8> = (0..entries)
        .flat_map(|i| [i as u8, 255 - i as u8, i as u8 ^ 0x55])
        .collect();
    chunk(&mut output, b"PLTE", &palette);
    if transparency {
        chunk(&mut output, b"tRNS", &[0]);
    }
    let scanlines: &[u8] = if depth == 8 {
        &[0, 0, 1, 255, 0, 128, 64, 2]
    } else {
        &[0, 0x01, 0x20, 0, 0x34, 0x50]
    };
    chunk(
        &mut output,
        b"IDAT",
        &crate::deflate_encode::compress(scanlines).unwrap(),
    );
    chunk(&mut output, b"IEND", &[]);
    output
}

fn insert_before(input: &[u8], before: &[u8; 4], kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut at = 8;
    loop {
        let n = u32::from_be_bytes(input[at..at + 4].try_into().unwrap()) as usize;
        if &input[at + 4..at + 8] == before {
            let mut output = input[..at].to_vec();
            chunk(&mut output, kind, data);
            output.extend_from_slice(&input[at..]);
            return output;
        }
        at += n + 12;
    }
}

const EXPECTED_RGB: [u8; 18] = [
    0, 255, 85, 1, 254, 84, 255, 0, 170, 128, 127, 213, 64, 191, 21, 2, 253, 87,
];

#[test]
fn complete_opaque_indexed8_expands_to_exact_three_band_rgb() {
    let data = fixture(8, 256, false);
    let decoded = decode(&data, &request())
        .unwrap()
        .expect("complete opaque palette should use Rust decoding");
    assert_eq!(decoded.dimensions, [3, 2]);
    assert_eq!(decoded.bands, 3);
    assert_eq!(decoded.pixels, EXPECTED_RGB);
}

#[test]
fn palette_structure_survives_owned_import_and_metadata_stripping() {
    super::super::engine::initialize().unwrap();
    let data = fixture(8, 256, false);
    let (image, plan) = try_load(&data, &request())
        .unwrap()
        .expect("palette import should retain encoding policy");
    drop(data);
    assert!(plan.is_none());
    assert_eq!(image.get_bands(), 3);
    assert_eq!(
        &*super::super::raster::export(&image).unwrap(),
        EXPECTED_RGB
    );
    assert_eq!(image.get_int("palette").unwrap(), 1);
    assert_eq!(image.get_int("bits-per-sample").unwrap(), 8);
    let image = super::super::metadata::strip(&image, &request()).unwrap();
    assert_eq!(image.get_int("palette").unwrap(), 1);
    assert_eq!(image.get_int("bits-per-sample").unwrap(), 8);
    // Without a quantizer native libvips can return RGB. With one, inherited
    // palette policy must bypass the truecolor Rust PNG saver.
    let probe = image
        .write_to_buffer_with_opts(
            ".png",
            VOption::new().set("palette", true).set("bitdepth", 8),
        )
        .unwrap();
    let output =
        super::super::encode::save(image, &request(), crate::request::OutputFormat::Png, 2)
            .unwrap();
    assert_eq!(output[25], probe[25]);
}

#[test]
fn indexed_source_bounds_are_checked_before_expansion() {
    let data = fixture(8, 256, false);
    let mut r = request();
    r.extra.max_source_pixels = 5;
    assert!(decode(&data, &r).is_err());
    r.extra.max_source_pixels = 6;
    r.extra.max_frame_pixels = 5;
    assert!(decode(&data, &r).is_err());
    r.extra.max_frame_pixels = 6;
    r.extra.max_source_bytes = data.len() - 1;
    assert!(decode(&data, &r).is_err());
    r.extra.max_source_bytes = data.len();
    assert_eq!(decode(&data, &r).unwrap().unwrap().pixels, EXPECTED_RGB);
}

#[test]
fn palette_decode_rejects_adler_crc_truncated_iend_and_late_crc() {
    let data = fixture(8, 256, false);
    let mut bad_header = data.clone();
    bad_header[29] ^= 1;
    assert!(decode(&bad_header, &request()).is_err());
    let mut bad_palette = data.clone();
    bad_palette[809] ^= 1;
    assert!(decode(&bad_palette, &request()).is_err());
    let mut at = 8;
    while &data[at + 4..at + 8] != b"IDAT" {
        at += u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize + 12;
    }
    let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let mut bad_crc = data.clone();
    bad_crc[at + 8 + len] ^= 1;
    assert!(decode(&bad_crc, &request()).is_err());
    let mut bad_adler = data.clone();
    bad_adler[at + 7 + len] ^= 1;
    let crc = crc32fast::hash(&bad_adler[at + 4..at + 8 + len]);
    bad_adler[at + 8 + len..at + 12 + len].copy_from_slice(&crc.to_be_bytes());
    assert!(decode(&bad_adler, &request()).is_err());
    assert!(decode(&data[..data.len() - 1], &request()).is_err());
    let mut late = insert_before(&data, b"IEND", b"tEXt", b"key\0value");
    let position = late.len() - 13;
    late[position] ^= 1;
    assert!(decode(&late, &request()).is_err());
}

#[test]
fn late_metadata_is_rechecked_before_exposing_expanded_pixels() {
    let data = fixture(8, 256, false);
    let late = insert_before(&data, b"IEND", b"gAMA", &45455u32.to_be_bytes());
    assert!(!matches!(decode(&late, &request()), Ok(Some(_))));
    for (kind, payload) in [
        (b"cHRM", chromaticities()),
        (b"sRGB", vec![0]),
        (b"tRNS", vec![0]),
        (b"cICP", vec![1, 13, 0, 1]),
        (b"eXIf", b"II*\0\x08\0\0\0\0\0".to_vec()),
    ] {
        let late = insert_before(&data, b"IEND", kind, &payload);
        assert!(!matches!(decode(&late, &request()), Ok(Some(_))));
    }
    let replaced_palette = vec![17; 768];
    let late_palette = insert_before(&data, b"IEND", b"PLTE", &replaced_palette);
    assert!(!matches!(decode(&late_palette, &request()), Ok(Some(_))));
    let late_text = insert_before(&data, b"IEND", b"tEXt", b"key\0value");
    assert_eq!(
        decode(&late_text, &request()).unwrap().unwrap().pixels,
        EXPECTED_RGB
    );
}

#[test]
fn incomplete_transparent_packed_and_profiled_palettes_keep_native_route() {
    assert!(decode(&fixture(8, 255, false), &request())
        .unwrap()
        .is_none());
    assert!(decode(&fixture(8, 256, true), &request())
        .unwrap()
        .is_none());
    assert!(decode(&fixture(4, 16, false), &request())
        .unwrap()
        .is_none());
    let data = fixture(8, 256, false);
    for (kind, payload) in [(b"gAMA", &45455u32.to_be_bytes()[..]), (b"sRGB", &[0][..])] {
        let profiled = insert_before(&data, b"PLTE", kind, payload);
        assert!(decode(&profiled, &request()).unwrap().is_none());
    }
    let profiled = insert_before(&data, b"PLTE", b"cHRM", &chromaticities());
    assert!(decode(&profiled, &request()).unwrap().is_none());
    let mut profile = vec![0; 132];
    profile[..4].copy_from_slice(&132u32.to_be_bytes());
    profile[36..40].copy_from_slice(b"acsp");
    let mut iccp = b"profile\0\0".to_vec();
    iccp.extend_from_slice(&crate::deflate_encode::compress(&profile).unwrap());
    let profiled = insert_before(&data, b"PLTE", b"iCCP", &iccp);
    assert!(decode(&profiled, &request()).unwrap().is_none());
}

#[test]
fn interlaced_animated_and_precision_requests_keep_native_route() {
    // Adam7 for a1x1 image contains only the first pass, so its payload is the
    // same single filtered sample as a noninterlaced image.
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut data, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 1]);
    let palette: Vec<u8> = (0..256).flat_map(|i| [i as u8; 3]).collect();
    chunk(&mut data, b"PLTE", &palette);
    chunk(
        &mut data,
        b"IDAT",
        &crate::deflate_encode::compress(&[0, 1]).unwrap(),
    );
    chunk(&mut data, b"IEND", &[]);
    assert!(decode(&data, &request()).unwrap().is_none());
    let data = fixture(8, 256, false);
    let mut actl = Vec::new();
    actl.extend_from_slice(&1u32.to_be_bytes());
    actl.extend_from_slice(&0u32.to_be_bytes());
    let animated = insert_before(&data, b"IDAT", b"acTL", &actl);
    assert!(decode(&animated, &request()).unwrap().is_none());
    for flag in 0..4 {
        let mut r = request();
        match flag {
            0 => r.strip_metadata = false,
            1 => r.strip_color_profile = false,
            2 => r.extra.keep_copyright = true,
            _ => r.extra.preserve_hdr = true,
        }
        assert!(decode(&data, &r).unwrap().is_none());
    }
}

fn chromaticities() -> Vec<u8> {
    [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect()
}

#[test]
fn rgb_and_rgba_gamma_chrm_chunks_keep_native_color_management() {
    for color in [ColorType::Rgb, ColorType::Rgba] {
        let mut data = Vec::new();
        let mut encoder = fast_png::Encoder::new(&mut data, 1, 1);
        encoder.set_color(color);
        encoder.set_depth(BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[7, 11, 13, 127][..color.samples()])
            .unwrap();
        for (kind, payload) in [
            (b"gAMA", 45455u32.to_be_bytes().to_vec()),
            (b"cHRM", chromaticities()),
        ] {
            let profiled = insert_before(&data, b"IDAT", kind, &payload);
            assert!(decode(&profiled, &request()).unwrap().is_none());
        }
    }
}

#[test]
fn expanded_palette_raster_over_byte_budget_falls_back_before_allocation() {
    let mut data = fixture(8, 256, false);
    data[16..20].copy_from_slice(&8192u32.to_be_bytes());
    data[20..24].copy_from_slice(&4096u32.to_be_bytes());
    let crc = crc32fast::hash(&data[12..29]);
    data[29..33].copy_from_slice(&crc.to_be_bytes());
    //33,554,432 indices fit the original pixel limit, but their100,663,296 RGB
    //bytes exceed the64MiB raster ceiling. No expanded buffer may be allocated.
    assert!(decode(&data, &request()).unwrap().is_none());
}
