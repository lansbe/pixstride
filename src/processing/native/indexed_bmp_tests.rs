use super::*;
use crate::request::{self, Config, OutputFormat};
use fast_png::{BitDepth, ColorType};

fn request(options: &str) -> Request {
    request::parse(
        &format!("/unsafe/{options}/plain/local:///fixture.png@bmp"),
        &Config::unsafe_local(),
    )
    .unwrap()
}

fn source(width: u32, height: u32) -> Vec<u8> {
    indexed(
        width,
        height,
        &(0..width * height).map(|n| n as u8).collect::<Vec<_>>(),
    )
}

fn indexed(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    {
        let mut encoder = fast_png::Encoder::new(&mut data, width, height);
        encoder.set_color(ColorType::Indexed);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_palette(
            (0..256)
                .flat_map(|n| [n as u8, (3 * n) as u8, (7 * n) as u8])
                .collect::<Vec<_>>(),
        );
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
    data
}

#[test]
fn opaque_palette_vertical_flip_keeps_indices_palette_and_roundtrip_pixels() {
    super::super::engine::initialize().unwrap();
    let data = source(33, 35);
    let output = super::super::engine::process(data, &request("fl:0:1"), &[]).unwrap();
    assert_eq!(output.origin, [33, 35]);
    assert_eq!(output.dimensions, [33, 35]);
    assert_eq!(output.format, OutputFormat::Bmp);
    assert_eq!(
        u16::from_le_bytes(output.body[28..30].try_into().unwrap()),
        8
    );
    assert_eq!(output.body.len(), 1078 + 36 * 35);
    for n in 0..256 {
        assert_eq!(
            &output.body[54 + 4 * n..58 + 4 * n],
            &[(7 * n) as u8, (3 * n) as u8, n as u8, 0]
        );
    }
    // Bottom-up BMP plus a vertical flip puts the first input row first on wire.
    assert_eq!(&output.body[1078..1111], &(0u8..33).collect::<Vec<_>>());
    assert_eq!(&output.body[1111..1114], &[0, 0, 0]);
    let loaded = super::super::bmp_gray::try_load(&output.body)
        .unwrap()
        .unwrap();
    let rgb = super::super::raster::export(&loaded).unwrap();
    // First displayed pixel is input row34, index1122 modulo256 =98.
    assert_eq!(&rgb[..6], &[98, 38, 174, 99, 41, 181]);
}

#[test]
fn orthogonal_rotations_then_flips_match_literal_color_blocks() {
    super::super::engine::initialize().unwrap();
    let pixels: Vec<u8> = (0..30)
        .flat_map(|y| (0..20).map(move |x| 1 + (y / 10) * 2 + x / 10))
        .collect();
    let data = indexed(20, 30, &pixels);
    for (options, width, expected) in [
        ("fl:1:0", 20, [2, 1, 4, 3, 6, 5]),
        ("fl:0:1", 20, [5, 6, 3, 4, 1, 2]),
        ("fl:1:1", 20, [6, 5, 4, 3, 2, 1]),
        ("rot:90", 30, [5, 3, 1, 6, 4, 2]),
        ("rot:90/fl:1:0", 30, [1, 3, 5, 2, 4, 6]),
        ("rot:90/fl:0:1", 30, [6, 4, 2, 5, 3, 1]),
        ("rot:90/fl:1:1", 30, [2, 4, 6, 1, 3, 5]),
        ("rot:180", 20, [6, 5, 4, 3, 2, 1]),
        ("rot:180/fl:1:0", 20, [5, 6, 3, 4, 1, 2]),
        ("rot:180/fl:0:1", 20, [2, 1, 4, 3, 6, 5]),
        ("rot:180/fl:1:1", 20, [1, 2, 3, 4, 5, 6]),
        ("rot:270", 30, [2, 4, 6, 1, 3, 5]),
        ("rot:270/fl:1:0", 30, [6, 4, 2, 5, 3, 1]),
        ("rot:270/fl:0:1", 30, [1, 3, 5, 2, 4, 6]),
        ("rot:270/fl:1:1", 30, [5, 3, 1, 6, 4, 2]),
    ] {
        let output = try_process(&data, &request(options), &[]).unwrap().unwrap();
        assert_eq!(output.dimensions, [width, 600 / width], "{options}");
        let loaded = super::super::bmp_gray::try_load(&output.body)
            .unwrap()
            .unwrap();
        let rgb = super::super::raster::export(&loaded).unwrap();
        for (n, sample) in rgb.chunks_exact(3).enumerate() {
            let value = expected[((n / width) / 10) * (width / 10) + (n % width) / 10];
            assert_eq!(sample, [value, value * 3, value * 7], "{options} pixel{n}");
        }
    }
}

#[test]
fn palette_permutations_obey_source_frame_bytes_and_result_bounds() {
    let data = source(33, 35);
    for frame in [false, true] {
        let mut r = request("fl:0:1");
        if frame {
            r.extra.max_frame_pixels = 1154;
        } else {
            r.extra.max_source_pixels = 1154;
        }
        assert!(try_process(&data, &r, &[]).is_err());
    }
    let mut r = request("fl:0:1");
    r.extra.max_source_bytes = data.len() - 1;
    assert!(try_process(&data, &r, &[]).is_err());
    let mut r = request("rot:90");
    r.extra.max_result_dimension = 34;
    assert!(try_process(&data, &r, &[]).unwrap().is_none());
    assert!(try_process(&data, &request("fl:0:1"), b"watermark")
        .unwrap()
        .is_none());
}

#[test]
fn complete_stream_is_required_and_malformed_checksums_cannot_reach_output() {
    let data = source(33, 35);
    let mut bad = data.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(try_process(&bad, &request("fl:0:1"), &[]).is_err());
    assert!(try_process(&data[..data.len() - 1], &request("fl:0:1"), &[]).is_err());
    let mut trailing = data;
    trailing.extend_from_slice(b"trailing");
    assert!(try_process(&trailing, &request("fl:0:1"), &[])
        .unwrap()
        .is_none());
}

#[test]
fn other_transforms_and_small_palettes_keep_the_general_route() {
    let data = source(33, 35);
    for options in [
        "",
        "fl:0:1/rs:fit:16:16",
        "fl:0:1/c:16:16",
        "fl:0:1/dpr:2",
        "fl:0:1/bl:1",
        "fl:0:1/pd:1",
        "fl:0:1/bg:ffffff",
        "fl:0:1/mb:100000",
        "fl:0:1/sm:0",
        "fl:0:1/scp:0",
        "fl:0:1/ph:1",
    ] {
        assert!(
            try_process(&data, &request(options), &[])
                .unwrap()
                .is_none(),
            "{options}"
        );
    }
    assert!(try_process(&source(2, 3), &request("fl:0:1"), &[])
        .unwrap()
        .is_none());
}
