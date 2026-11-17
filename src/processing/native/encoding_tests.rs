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

#[test]
fn metadata_stripping_retains_palette_representation_and_removes_private_fields() {
    let mut source = image(&[20, 40, 60, 255], 1, 1);
    source.set_int("palette", 1).unwrap();
    source.set_int("bits-per-sample", 4).unwrap();
    source
        .set_string("exif-ifd0-Artist", "private artist")
        .unwrap();
    let result = super::super::metadata::strip(&source, &request("", "png")).unwrap();
    assert_eq!(result.get_int("palette").unwrap_or(0), 1);
    assert_eq!(result.get_int("bits-per-sample").unwrap(), 4);
    assert_eq!(result.get_typeof("exif-ifd0-Artist").unwrap_or(0), 0);
}

#[test]
fn inherited_palette_uses_native_png_quantizer_defaults_or_native_fallback() {
    let data: Vec<u8> = (0..16)
        .flat_map(|y| (0..16).flat_map(move |x| [x * 17, y * 17, (x + y) * 8, 255]))
        .collect();
    let mut source = image(&data, 16, 16);
    source.set_int("palette", 1).unwrap();
    source.set_int("bits-per-sample", 8).unwrap();
    let expected = source
        .write_to_buffer_with_opts(
            ".png",
            VOption::new()
                .set("palette", true)
                .set("bitdepth", 8)
                .set("filter", libvips::enums::ForeignPngFilter::None as i32),
        )
        .unwrap();
    let actual = save(source, &request("", "png"), OutputFormat::Png, 16).unwrap();
    assert_eq!(actual[25] == 3, expected[25] == 3);
    let decode = |bytes: &[u8]| {
        let mut image = VipsImage::new_from_buffer(bytes, "")
            .unwrap()
            .colourspace(Interpretation::Srgb)
            .unwrap();
        if !image.hasalpha() {
            image = image.addalpha().unwrap();
        }
        image.write_to_memory()
    };
    assert_eq!(decode(&actual), decode(&expected));
}

#[test]
fn inherited_palette_normalizes_source_depth_and_explicit_settings_override_it() {
    let mut source = image(&[20, 40, 60, 255], 1, 1);
    let mut r = request("", "png");
    source.set_int("bits-per-sample", 8).unwrap();
    assert_eq!(png_depth(&source, &r), None);
    source.set_int("palette", 1).unwrap();
    for (bits, want) in [
        (0, None),
        (1, Some(1)),
        (2, Some(2)),
        (3, Some(4)),
        (4, Some(4)),
        (5, Some(8)),
        (8, Some(8)),
        (16, None),
    ] {
        source.set_int("bits-per-sample", bits).unwrap();
        assert_eq!(png_depth(&source, &r), want);
    }
    r.extra.encoding.png_quantize = true;
    r.extra.encoding.png_colors = 2;
    assert_eq!(png_depth(&source, &r), Some(1));
}
