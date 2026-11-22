use super::super::engine::initialize;
use super::*;
use libvips::enums::{BandFormat, Interpretation};

fn request(block: usize) -> Request {
    crate::request::parse(
        &format!("/unsafe/pix:{block}/plain/local:///fixture.png@png"),
        &crate::request::Config::unsafe_local(),
    )
    .unwrap()
}

fn image(data: &[u8], width: i32, height: i32, bands: i32) -> VipsImage {
    initialize().unwrap();
    VipsImage::new_from_memory_copy(data, width, height, bands, BandFormat::Uchar)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        .unwrap()
}

#[test]
fn pixelation_averages_integer_blocks_and_mirrors_partial_edges() {
    let data: Vec<u8> = [0, 20, 40, 60, 80, 100, 120, 140, 160]
        .into_iter()
        .flat_map(|v| [v; 3])
        .collect();
    let result = effects(image(&data, 3, 3, 3), &request(2)).unwrap();
    for (index, expected) in [40., 40., 70., 40., 40., 70., 130., 130., 160.]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            result
                .getpoint((index % 3) as i32, (index / 3) as i32)
                .unwrap(),
            [expected; 3]
        );
    }
}

#[test]
fn pixelation_clamps_oversized_blocks_before_mirroring() {
    let data: Vec<u8> = [0, 30, 60, 90, 120, 150]
        .into_iter()
        .flat_map(|v| [v; 3])
        .collect();
    let result = effects(image(&data, 3, 2, 3), &request(100)).unwrap();
    assert_eq!([result.get_width(), result.get_height()], [3, 2]);
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(result.getpoint(x, y).unwrap(), [90.; 3]);
        }
    }
}

#[test]
fn pixelation_averages_straight_color_and_alpha_independently() {
    let data = [
        0, 0, 0, 0, 40, 40, 40, 64, 80, 80, 80, 128, 120, 120, 120, 192,
    ];
    let result = effects(image(&data, 2, 2, 4), &request(2)).unwrap();
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(result.getpoint(x, y).unwrap(), [60., 60., 60., 96.]);
        }
    }
}

#[test]
fn blurring_an_alpha_edge_never_reveals_hidden_blue() {
    let mut r = request(2);
    r.effects.blur_sigma = 1.25;
    let pixels = [
        [255, 0, 0, 255],
        [255, 0, 0, 255],
        [0, 0, 255, 0],
        [0, 0, 255, 0],
    ]
    .concat()
    .repeat(4);
    let result = effects(image(&pixels, 4, 4, 4), &r).unwrap();
    let pixel = result.getpoint(2, 2).unwrap();
    assert!(pixel[3] > 0. && pixel[3] < 255.);
    assert_eq!(pixel[2], 0.);
    assert!(pixel[0] >= 254.);
}

#[test]
fn sharpening_srgb_separately_preserves_native_color_and_all_alpha_values() {
    super::super::engine::initialize().unwrap();
    let pixels: Vec<_> = (0..65536u32)
        .flat_map(|i| {
            [
                (i * 13) as u8,
                ((i >> 8) ^ i) as u8,
                (i * 37) as u8,
                (i >> 8) as u8,
            ]
        })
        .collect();
    let rgba = image(&pixels, 256, 256, 4);
    let expected = rgba
        .sharpen_with_opts(VOption::new().set("sigma", 1.5))
        .unwrap();
    let actual = sharpen(rgba, 1.5).unwrap();
    let original = super::super::raster::export(&expected).unwrap();
    let filtered = super::super::raster::export(&actual).unwrap();
    assert!(original.iter().eq(filtered.iter()));
}
