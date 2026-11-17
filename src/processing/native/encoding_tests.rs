use super::*;

fn request(options: &str, format: &str) -> Request {
    crate::request::parse(
        &format!("/unsafe/{options}plain/local:///fixture.png@{format}"),
        &crate::request::Config::unsafe_local(),
    )
    .unwrap()
}

fn image(data: &[u8], width: i32, height: i32) -> VipsImage {
    super::super::engine::initialize().unwrap();
    VipsImage::new_from_memory_copy(data, width, height, 4, BandFormat::Uchar)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        .unwrap()
}

#[test]
fn native_rgba_bmp_preserves_straight_bgra_without_white_compositing() {
    let source = image(&[11, 22, 33, 0, 44, 55, 66, 128], 2, 1);
    let bytes = save(source, &request("", "bmp"), OutputFormat::Bmp, 1).unwrap();
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 32);
    let offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    assert_eq!(&bytes[offset..], &[33, 22, 11, 0, 66, 55, 44, 128]);
}

#[test]
fn jpeg_defaults_to_white_for_transparency_and_respects_explicit_background() {
    let source = image(&[200, 15, 85, 0].repeat(16 * 16), 16, 16)
        .write_to_buffer(".png")
        .unwrap();
    for (options, expected) in [
        ("q:100/", [255., 255., 255.]),
        ("bg:112233/q:100/", [17., 34., 51.]),
    ] {
        let result =
            super::super::engine::process(source.clone(), &request(options, "jpg"), &[]).unwrap();
        let decoded = VipsImage::new_from_buffer(&result.body, "").unwrap();
        let actual = decoded.getpoint(8, 8).unwrap();
        assert_eq!(actual.len(), 3);
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(value, want)| (value - want).abs() <= 2.),
            "{options}: {actual:?}"
        );
    }
}
