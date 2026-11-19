use super::*;

fn transformed(options: &str) -> VipsImage {
    initialize().unwrap();
    let pixels: Vec<u8> = (0..8)
        .flat_map(|y| {
            (0..8).flat_map(move |x| {
                [
                    ((x * 37 + y * 17) % 256) as u8,
                    ((x * 83 + y * 31) % 256) as u8,
                    ((x * 19 + y * 67) % 256) as u8,
                ]
            })
        })
        .collect();
    let image = VipsImage::new_from_memory_copy(&pixels, 8, 8, 3, BandFormat::Uchar)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        .unwrap();
    let source = image
        .write_to_buffer_with_opts(".webp", VOption::new().set("lossless", true))
        .unwrap();
    let r = crate::request::parse(
        &format!("/unsafe/{options}/plain/local:///fixture.webp@png"),
        &crate::request::Config::unsafe_local(),
    )
    .unwrap();
    let output = process(source, &r, &[]).unwrap();
    VipsImage::new_from_buffer(&output.body, "")
        .unwrap()
        .copy_memory()
        .unwrap()
}

#[test]
fn pipeline_uses_webp_decoder_samples_instead_of_full_raster_lanczos() {
    let output = transformed("rs:force:4:4");
    assert_eq!([output.get_width(), output.get_height()], [4, 4]);
    // Captured from the reference WebP decoder's exact half-scale operation.
    let expected = [
        27, 57, 43, 101, 159, 81, 175, 133, 119, 121, 107, 157, 61, 119, 177, 135, 93, 151, 209,
        195, 125, 27, 105, 99, 95, 181, 55, 169, 91, 93, 179, 129, 131, 61, 167, 169, 129, 115,
        189, 203, 153, 163, 85, 63, 137, 95, 165, 47,
    ];
    assert_eq!(output.write_to_memory(), expected);
}

#[test]
fn pipeline_applies_crop_in_reduced_source_coordinates() {
    let output = transformed("c:4:4:soea:0.1:0.1/rs:fill:2:2");
    assert_eq!([output.get_width(), output.get_height()], [2, 2]);
    assert_eq!(
        output.write_to_memory(),
        [179, 129, 131, 61, 167, 169, 85, 63, 137, 95, 165, 47]
    );
}
