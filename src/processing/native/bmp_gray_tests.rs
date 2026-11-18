use super::*;
use crate::request::{self, Config, Request};
use libvips::{
    enums::{BandFormat, Interpretation},
    voption::{Setter, VOption},
};

fn fixture(top_down: bool, reverse_palette: bool) -> Vec<u8> {
    let mut bytes = vec![0u8; 1086];
    bytes[..2].copy_from_slice(b"BM");
    for (at, value) in [
        (2, 1086u32),
        (10, 1078),
        (14, 40),
        (18, 2),
        (22, if top_down { (-2i32) as u32 } else { 2 }),
        (34, 8),
        (46, 256),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[26] = 1;
    bytes[28] = 8;
    for index in 0..256 {
        let value = if reverse_palette { 255 - index } else { index } as u8;
        bytes[54 + index * 4..58 + index * 4].copy_from_slice(&[value, value, value, 173]);
    }
    let rows = if top_down {
        [0, 64, 91, 92, 128, 255, 93, 94]
    } else {
        [128, 255, 93, 94, 0, 64, 91, 92]
    };
    bytes[1078..].copy_from_slice(&rows);
    bytes
}

fn request() -> Request {
    request::parse(
        "/unsafe/plain/local:///fixture.bmp@bmp",
        &Config::unsafe_local(),
    )
    .unwrap()
}

#[test]
fn reads_both_row_orders_padding_and_real_palette_mapping_as_owned_gray() {
    super::super::engine::initialize().unwrap();
    for top_down in [false, true] {
        for reverse in [false, true] {
            let mut bytes = fixture(top_down, reverse);
            let output = try_load(&bytes).unwrap().expect("gray BMP8 loader");
            assert_eq!(output.get_bands(), 1);
            assert_eq!((output.get_width(), output.get_height()), (2, 2));
            assert_eq!(output.get_interpretation().unwrap(), Interpretation::BW);
            bytes.fill(0);
            drop(bytes);
            assert_eq!(
                &*super::super::raster::export(&output).unwrap(),
                if reverse {
                    &[255, 191, 127, 0]
                } else {
                    &[0, 64, 128, 255]
                }
            );
        }
    }
    let mut bytes = fixture(false, false);
    bytes[46..50].fill(0); // Zero means the full 256-entry palette for BMP8.
    for (index, value) in [(0usize, 17u8), (64, 17), (128, 64), (255, 250)] {
        bytes[54 + index * 4..57 + index * 4].fill(value);
    }
    let output = try_load(&bytes).unwrap().unwrap();
    assert_eq!(
        &*super::super::raster::export(&output).unwrap(),
        &[17, 17, 64, 250]
    );
}

#[test]
fn grayscale_encoder_has_exact_palette_rows_stride_and_native_roundtrip() {
    super::super::engine::initialize().unwrap();
    for (width, height) in [(3usize, 128usize), (3, 256), (33, 33), (129, 8)] {
        let pixels: Vec<u8> = (0..width * height).map(|n| (n * 37) as u8).collect();
        let input = VipsImage::new_from_owned_u8(pixels.clone(), width as i32, height as i32, 1)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::BW as i32))
            .unwrap();
        let body = super::super::bmp_encode::save(&input).unwrap();
        assert_eq!(u16::from_le_bytes(body[28..30].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(body[10..14].try_into().unwrap()), 1078);
        let stride = (width + 3) & !3;
        assert_eq!(body.len(), 1078 + stride * height);
        for index in 0..256 {
            assert_eq!(
                &body[54 + index * 4..58 + index * 4],
                &[index as u8, index as u8, index as u8, 0]
            );
        }
        for row in 0..height {
            assert_eq!(
                &body[1078 + stride * row..1078 + stride * row + width],
                &pixels[(height - 1 - row) * width..(height - row) * width]
            );
            assert!(body[1078 + stride * row + width..1078 + stride * (row + 1)]
                .iter()
                .all(|b| *b == 0));
        }
        assert!(super::super::needs(&body));
        let roundtrip = super::super::engine::process(body.clone(), &request(), &[]).unwrap();
        assert_eq!(roundtrip.origin, [width, height]);
        assert_eq!(roundtrip.dimensions, [width, height]);
        assert_eq!(roundtrip.body, body);
        assert_eq!(
            &*super::super::raster::export(&try_load(&body).unwrap().unwrap()).unwrap(),
            pixels.as_slice()
        );
    }
}

#[test]
fn palette_overhead_never_grows_small_gray_outputs() {
    super::super::engine::initialize().unwrap();
    for (width, height) in [(1, 1), (1, 512), (17, 7), (33, 15)] {
        let input =
            VipsImage::new_from_owned_u8(vec![128; width * height], width as i32, height as i32, 1)
                .unwrap()
                .copy_with_opts(VOption::new().set("interpretation", Interpretation::BW as i32))
                .unwrap();
        assert!(
            try_save(&input).unwrap().is_none(),
            "unnecessary BMP8 palette overhead"
        );
        let output = super::super::bmp_encode::save(&input).unwrap();
        assert_eq!(u16::from_le_bytes(output[28..30].try_into().unwrap()), 24);
    }
}

#[test]
fn small_gray_requested_byte_target_keeps_existing_success() {
    super::super::engine::initialize().unwrap();
    let mut pgm = b"P5\n17 7\n255\n".to_vec();
    pgm.extend_from_slice(&[128; 17 * 7]);
    let r = request::parse(
        "/unsafe/mb:500/plain/local:///fixture.pgm@bmp",
        &Config::unsafe_local(),
    )
    .unwrap();
    let result = super::super::engine::process(pgm, &r, &[]).unwrap();
    assert_eq!(result.body.len(), 418);
}

#[test]
fn only_uncompressed_complete_palette_is_admitted_and_limits_precede_decode() {
    super::super::engine::initialize().unwrap();
    let source = fixture(false, false);
    assert_eq!(source_dimensions(&source).unwrap(), Some([2, 2]));
    for (position, value) in [
        (14, 108u32),
        (18, 0),
        (18, u32::MAX),
        (22, 0),
        (22, i32::MIN as u32),
        (10, 1077),
        (10, u32::MAX),
        (30, 1),
        (46, 2),
    ] {
        let mut bad = source.clone();
        bad[position..position + 4].copy_from_slice(&value.to_le_bytes());
        assert!(try_load(&bad).is_err(), "header position {position}");
    }
    let mut planes = source.clone();
    planes[26] = 0;
    assert!(try_load(&planes).is_err());
    for length in [30, 53, 54, 1077, 1085] {
        assert!(try_load(&source[..length]).is_err(), "truncation {length}");
    }
    for frame_limit in [false, true] {
        let mut r = request();
        if frame_limit {
            r.extra.max_frame_pixels = 3;
        } else {
            r.extra.max_source_pixels = 3;
        }
        assert!(matches!(
            super::super::engine::process(source.clone(), &r, &[]),
            Err("BMP source pixel budget exceeded")
        ));
    }
}

#[test]
fn rgb_alpha_high_precision_and_other_formats_keep_existing_routes() {
    super::super::engine::initialize().unwrap();
    assert!(try_load(b"not a BMP").unwrap().is_none());
    for (bands, space) in [
        (3, Interpretation::Srgb),
        (4, Interpretation::Srgb),
        (2, Interpretation::BW),
    ] {
        let image = VipsImage::new_from_owned_u8(vec![128; bands], 1, 1, bands as i32)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", space as i32))
            .unwrap();
        assert!(try_save(&image).unwrap().is_none());
    }
    let gray16 = VipsImage::new_from_owned_u8(vec![128], 1, 1, 1)
        .unwrap()
        .cast(BandFormat::Ushort)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", Interpretation::Grey16 as i32))
        .unwrap();
    assert!(try_save(&gray16).unwrap().is_none());
}

#[test]
fn complete_color_palette_maps_to_owned_rgb_with_both_row_orders() {
    super::super::engine::initialize().unwrap();
    for top_down in [false, true] {
        let mut bytes = fixture(top_down, false);
        for (index, rgb) in [
            (0usize, [1, 2, 3]),
            (64, [4, 5, 6]),
            (128, [7, 8, 9]),
            (255, [10, 11, 12]),
        ] {
            bytes[54 + index * 4..57 + index * 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        let image = try_load(&bytes).unwrap().expect("color BMP8 loader");
        assert_eq!(image.get_bands(), 3);
        assert_eq!(image.get_interpretation().unwrap(), Interpretation::Srgb);
        bytes.fill(0);
        drop(bytes);
        assert_eq!(
            &*super::super::raster::export(&image).unwrap(),
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
    }
}

#[test]
fn color_palette_expansion_keeps_the_decoded_byte_ceiling() {
    let mut bytes = fixture(false, false);
    bytes.resize(1078 + 5000 * 4500, 0);
    bytes[18..22].copy_from_slice(&5000u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&4500u32.to_le_bytes());
    bytes[55] ^= 1;
    assert_eq!(source_dimensions(&bytes).unwrap(), Some([5000, 4500]));
    assert!(matches!(try_load(&bytes), Err("BMP8 raster exceeds limit")));
}
