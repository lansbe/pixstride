use super::*;

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
