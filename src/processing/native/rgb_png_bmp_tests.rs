use super::*;
use crate::request::{self, Config};

fn request(options: &str) -> Request {
    request::parse(
        &format!("/unsafe/{options}/plain/local:///fixture.png@bmp"),
        &Config::unsafe_local(),
    )
    .unwrap()
}

fn source() -> Vec<u8> {
    crate::png_encode::encode_pixels(
        2,
        3,
        [
            1, 10, 20, 2, 20, 40, 3, 30, 60, 4, 40, 80, 5, 50, 100, 6, 60, 120,
        ],
        3,
    )
    .unwrap()
    .as_slice()
    .to_vec()
}

#[test]
fn direct_rgb_rows_have_exact_bmp_header_bgr_order_and_padding() {
    let output = try_process(&source(), &request("fl:0:1"), &[])
        .unwrap()
        .expect("RGB row encoder");
    assert_eq!(output.origin, [2, 3]);
    assert_eq!(output.dimensions, [2, 3]);
    assert_eq!(output.body.len(), 78);
    assert_eq!(&output.body[..2], b"BM");
    assert_eq!(&output.body[28..30], &24u16.to_le_bytes());
    assert_eq!(
        &output.body[54..],
        &[20, 10, 1, 40, 20, 2, 0, 0, 60, 30, 3, 80, 40, 4, 0, 0, 100, 50, 5, 120, 60, 6, 0, 0]
    );
}

#[test]
fn row_permutations_match_literal_rgb_matrices() {
    for (options, expected) in [
        ("fl:1:0", [2, 1, 4, 3, 6, 5]),
        ("fl:0:1", [5, 6, 3, 4, 1, 2]),
        ("fl:1:1", [6, 5, 4, 3, 2, 1]),
        ("rot:180", [6, 5, 4, 3, 2, 1]),
        ("rot:180/fl:1:0", [5, 6, 3, 4, 1, 2]),
        ("rot:180/fl:0:1", [2, 1, 4, 3, 6, 5]),
        ("rot:180/fl:1:1", [1, 2, 3, 4, 5, 6]),
    ] {
        let output = try_process(&source(), &request(options), &[])
            .unwrap()
            .unwrap();
        let (_, _, pixels) = crate::image::BmpView::parse(&output.body)
            .unwrap()
            .decode_rgb()
            .unwrap();
        let indices: Vec<u8> = pixels.chunks_exact(3).map(|p| p[0]).collect();
        assert_eq!(indices, expected, "{options}");
    }
}

#[test]
fn unsupported_geometry_and_source_admission_keep_their_contract() {
    for options in [
        "",
        "rot:90",
        "rot:270",
        "fl:0:1/rs:fit:1:1",
        "fl:0:1/bl:1",
        "fl:0:1/c:1:1",
        "fl:0:1/mb:200",
        "fl:0:1/sm:0",
        "fl:0:1/ph:1",
    ] {
        assert!(
            try_process(&source(), &request(options), &[])
                .unwrap()
                .is_none(),
            "{options}"
        );
    }
    let mut r = request("fl:0:1");
    r.extra.max_source_pixels = 5;
    assert!(try_process(&source(), &r, &[]).is_err());
    r.extra.max_source_pixels = 6;
    r.extra.max_frame_pixels = 5;
    assert!(try_process(&source(), &r, &[]).is_err());
    r.extra.max_frame_pixels = 6;
    let data = source();
    r.extra.max_source_bytes = data.len() - 1;
    assert!(try_process(&data, &r, &[]).is_err());
    r.extra.max_source_bytes = data.len();
    r.extra.max_result_dimension = 2;
    assert!(try_process(&data, &r, &[]).unwrap().is_none());
}

#[test]
fn complete_chunk_validation_precedes_output() {
    let data = source();
    let r = request("fl:0:1");
    let idat = 33;
    assert_eq!(&data[idat + 4..idat + 8], b"IDAT");
    let size = u32::from_be_bytes(data[idat..idat + 4].try_into().unwrap()) as usize;
    let crc_at = idat + 8 + size;
    let mut bad_crc = data.clone();
    bad_crc[crc_at] ^= 1;
    assert!(try_process(&bad_crc, &r, &[]).is_err());
    let mut bad_adler = data.clone();
    bad_adler[crc_at - 1] ^= 1;
    let crc = crc32fast::hash(&bad_adler[idat + 4..crc_at]);
    bad_adler[crc_at..crc_at + 4].copy_from_slice(&crc.to_be_bytes());
    assert!(try_process(&bad_adler, &r, &[]).is_err());
    assert!(try_process(&data[..data.len() - 1], &r, &[]).is_err());

    let mut late_text = data.clone();
    let name_and_payload = b"tEXtkey\0value";
    let text = [
        9u32.to_be_bytes().as_slice(),
        name_and_payload,
        crc32fast::hash(name_and_payload).to_be_bytes().as_slice(),
    ]
    .concat();
    late_text.splice(data.len() - 12..data.len() - 12, text);
    assert!(try_process(&late_text, &r, &[]).unwrap().is_none());
}

#[test]
fn real_worker_engine_flips_odd_width_rows_without_moving_padding() {
    super::super::engine::initialize().unwrap();
    let pixels: Vec<_> = (0u8..99).flat_map(|n| [n, n * 2, 255 - n]).collect();
    let source = crate::png_encode::encode_pixels(33, 3, &pixels[..], 3).unwrap();
    let output =
        super::super::engine::process(source.as_slice().to_vec(), &request("fl:1:1"), &[]).unwrap();
    let (width, height, rgb) = crate::image::BmpView::parse(&output.body)
        .unwrap()
        .decode_rgb()
        .unwrap();
    let expected: Vec<_> = pixels.chunks_exact(3).rev().flatten().copied().collect();
    assert_eq!([width, height], [33, 3]);
    assert_eq!(rgb, expected);
    assert!(output.body[54..].chunks_exact(100).all(|row| row[99] == 0));
}
