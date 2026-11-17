use super::*;
use crate::request::{self, Config};
use libvips::{
    enums::{BandFormat, Interpretation},
    voption::{Setter, VOption},
};

fn request(options: &str, extension: &str) -> Request {
    request::parse(
        &format!("/unsafe/{options}/plain/local:///fixture.{extension}@png"),
        &Config::unsafe_local(),
    )
    .unwrap()
}

fn webp() -> Arc<Vec<u8>> {
    super::super::engine::initialize().unwrap();
    let pixels = (0..256 * 128 * 3)
        .map(|n| ((n * 73 + n / 256 * 19) % 256) as u8)
        .collect();
    let image = VipsImage::new_from_owned_u8(pixels, 256, 128, 3)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
        .unwrap();
    Arc::new(
        image
            .write_to_buffer_with_opts(".webp", VOption::new().set("lossless", true))
            .unwrap(),
    )
}

fn head(data: &[u8]) -> VipsImage {
    super::super::engine::initialize().unwrap();
    super::super::engine::load(data, 1, false).unwrap()
}

fn equal_pixels(actual: &VipsImage, expected: &VipsImage) {
    assert_eq!(actual.get_width(), expected.get_width());
    assert_eq!(actual.get_height(), expected.get_height());
    assert_eq!(actual.get_bands(), expected.get_bands());
    assert_eq!(
        &*super::super::raster::export(actual).unwrap(),
        &*super::super::raster::export(expected).unwrap()
    );
}

#[test]
fn webp_reloads_at_decoder_scale_instead_of_lanczos_resampling() {
    let data = webp();
    let r = request("rs:force:64:32/dpr:2", "webp");
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .expect("WebP reduction should use the decoder");
    assert_eq!(loaded.plan.scale, [1.0; 2]);
    assert_eq!(loaded.plan.target, [128, 64]);
    assert_eq!(loaded.plan.dpr, 2.0);
    // Independent reference: WebP's own decoder at the requested half size.
    let expected =
        VipsImage::new_from_buffer_with_opts(&data, "", VOption::new().set("scale", 0.5)).unwrap();
    equal_pixels(&loaded.image, &expected);
}

#[test]
fn webp_fractional_reduction_preserves_relative_crop_offsets_and_position() {
    let data = webp();
    let r = request("c:128:64:soea:0.1:0.1/rs:fill:42:42/g:noea", "webp");
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .expect("fractional WebP reduction should use the decoder");
    assert_eq!(
        [loaded.image.get_width(), loaded.image.get_height()],
        [168, 84]
    );
    assert_eq!(loaded.request.extra.crop_size, Some([84.0, 42.0]));
    assert_eq!(loaded.request.extra.crop_offsets, [0.1, 0.1]);
    assert_eq!(r.extra.crop_size, Some([128.0, 64.0]));
    let (actual, _) =
        super::super::geometry::apply(loaded.image, &loaded.request, false, Some(loaded.plan))
            .unwrap();
    // 168x84 decoder raster; southeast crop84x42 has offsets17,8;
    // northeast result42x42 starts at(109,34) in that raster.
    let expected =
        VipsImage::new_from_buffer_with_opts(&data, "", VOption::new().set("scale", 0.65625))
            .unwrap()
            .extract_area(109, 34, 42, 42)
            .unwrap();
    equal_pixels(&actual, &expected);
}

#[test]
fn crop_fraction_zero_and_clamping_are_resolved_in_original_coordinates() {
    let data = webp();
    for (options, expected) in [
        ("c:0.5:0:ce/rs:force:32:32", [32.0, 32.0]),
        ("c:1024:1024:ce/rs:force:32:16", [32.0, 16.0]),
    ] {
        let r = request(options, "webp");
        let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
            .unwrap()
            .expect("bounded source crop reduction");
        assert_eq!(loaded.request.extra.crop_size, Some(expected));
    }
}

#[test]
fn absolute_crop_offsets_scale_without_becoming_relative() {
    let data = webp();
    let r = request("c:128:64:ce:1:-5/rs:fill:42:42", "webp");
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .expect("absolute offsets can be adjusted");
    assert_eq!(loaded.request.extra.crop_offsets, [1.0, -3.0]);
}

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="192"><path d="M0 0 Q110 190 256 0 L256 192 L0 192 Z" fill="red" opacity="0.5"/></svg>"#;

#[test]
fn svg_renders_at_102_by_77_then_resamples_only_the_final_row_axis() {
    let data = Arc::new(SVG.to_vec());
    let r = request("rs:force:51:38/dpr:2", "svg");
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Svg)
        .unwrap()
        .expect("SVG should render near its final dimensions");
    assert_eq!(
        [loaded.image.get_width(), loaded.image.get_height()],
        [102, 77]
    );
    assert_eq!(loaded.plan.scale[0], 1.0);
    assert!((loaded.plan.scale[1] - 76.0 / 77.0).abs() < 1e-14);
    assert_eq!(loaded.plan.target, [102, 76]);
    let (actual, dpr) =
        super::super::geometry::apply(loaded.image, &loaded.request, true, Some(loaded.plan))
            .unwrap();
    assert_eq!(dpr, 2.0);
    let expected = VipsImage::new_from_buffer_with_opts(
        SVG,
        "",
        VOption::new().set("dpi", 96.0).set("scale", 0.298828125),
    )
    .unwrap()
    .premultiply()
    .unwrap()
    .cast(BandFormat::Uchar)
    .unwrap()
    .resize_with_opts(1.0, VOption::new().set("vscale", 76.0 / 77.0))
    .unwrap()
    .unpremultiply()
    .unwrap()
    .cast(BandFormat::Uchar)
    .unwrap();
    equal_pixels(&actual, &expected);
}

#[test]
fn vector_dpr_enlargement_does_not_require_raster_enlarge_permission() {
    let data = Arc::new(SVG.to_vec());
    let r = request("rs:force:256:192/dpr:2", "svg");
    assert!(!r.options.enlarge);
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Svg)
        .unwrap()
        .expect("vectors must render at requested density");
    assert_eq!(
        [loaded.image.get_width(), loaded.image.get_height()],
        [512, 384]
    );
    assert_eq!(loaded.plan.scale, [1.0; 2]);
    assert_eq!(loaded.plan.dpr, 2.0);
    let expected = VipsImage::new_from_buffer_with_opts(
        SVG,
        "",
        VOption::new().set("dpi", 96.0).set("scale", 1.5),
    )
    .unwrap();
    equal_pixels(&loaded.image, &expected);
}

#[test]
fn anisotropic_vector_render_is_rejected_before_oversized_allocation() {
    let data = Arc::new(br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="256"><rect width="1" height="256"/></svg>"#.to_vec());
    let r = request("rs:force:10000:1", "svg");
    assert!(try_load(&data, &head(&data), &r, OutputFormat::Svg).is_err());
}

#[test]
fn unsupported_sources_and_geometry_keep_the_original_route() {
    let data = webp();
    for options in [
        "rs:force:512:256", // raster enlargement is disabled
        "rs:force:256:128", // identity
        "rs:fit:64:64/rot:90",
        "rs:fit:64:64/fl:1",
        "rs:fit:64:64/trim:10",
    ] {
        let r = request(options, "webp");
        assert!(try_load(&data, &head(&data), &r, OutputFormat::Webp)
            .unwrap()
            .is_none());
    }
    let mut r = request("rs:fit:64:64", "webp");
    assert!(try_load(&data, &head(&data), &r, OutputFormat::Png)
        .unwrap()
        .is_none());
    r.extra.encoding.linear = true;
    assert!(try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .is_none());
    r.extra.encoding.linear = false;
    r.extra.enforce_thumbnail = true;
    assert!(try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .is_none());
    r.extra.enforce_thumbnail = false;
    let mut oriented = head(&data);
    oriented.set_int("orientation", 6).unwrap();
    assert!(try_load(&data, &oriented, &r, OutputFormat::Webp)
        .unwrap()
        .is_none());
    let mut animated = head(&data);
    animated.set_int("n-pages", 2).unwrap();
    assert!(try_load(&data, &animated, &r, OutputFormat::Webp)
        .unwrap()
        .is_none());
}

#[test]
fn reduced_lazy_image_owns_encoded_source_after_caller_releases_it() {
    let data = webp();
    let weak = Arc::downgrade(&data);
    let r = request("rs:force:128:64", "webp");
    let header = head(&data);
    let loaded = try_load(&data, &header, &r, OutputFormat::Webp)
        .unwrap()
        .expect("reduced source owns input");
    drop(header);
    drop(data);
    assert!(weak.upgrade().is_some());
    assert!(!super::super::raster::export(&loaded.image)
        .unwrap()
        .is_empty());
    drop(loaded);
    libvips::Vips::thread_shutdown();
    assert!(weak.upgrade().is_none());
}

#[test]
fn embedded_watermark_raster_keeps_the_standard_decode_route() {
    let data = webp();
    let mut r = request("rs:force:128:64", "webp");
    r.options.watermark = Some(crate::ops::Watermark {
        image: crate::Image {
            width: 1,
            height: 1,
            pixels: crate::Buffer::new(4).unwrap(),
        },
        gravity: crate::ops::Gravity::Center,
        offset_x: 0,
        offset_y: 0,
        opacity: 255,
    });
    assert!(try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .is_none());
}

#[test]
fn adjusted_geometry_request_preserves_watermark_formats_and_limits() {
    let data = webp();
    let mut r = request("rs:force:128:64", "webp");
    r.format = Some(OutputFormat::Avif);
    r.quality = Some(65);
    r.watermark = Some(request::WatermarkSpec {
        opacity: 0.75,
        gravity: crate::ops::Gravity::SouthEast,
        offset_x: 3.0,
        offset_y: 4.0,
        scale: 0.5,
    });
    r.strip_metadata = false;
    r.strip_color_profile = false;
    r.extra.max_source_pixels = 500_000;
    r.extra.max_source_bytes = 4_000_000;
    r.extra.max_frame_pixels = 400_000;
    r.extra.max_result_dimension = 2048;
    let original = serde_json::to_value(&r).unwrap();
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .expect("ordinary typed options do not disable reduced decode");
    assert_eq!(serde_json::to_value(&loaded.request).unwrap(), original);
}

#[test]
fn absolute_fractional_crop_dimensions_truncate_before_decoder_planning() {
    let data = webp();
    let r = request("c:128.75:64.75:ce/rs:fill:42:42", "webp");
    let loaded = try_load(&data, &head(&data), &r, OutputFormat::Webp)
        .unwrap()
        .expect("absolute crop dimensions resolve before shrink");
    // Reference CalcCropSize converts absolute values to integer pixels;
    // 128x64 selects a168x84 uniform decoder render and a84x42 crop.
    assert_eq!(
        [loaded.image.get_width(), loaded.image.get_height()],
        [168, 84]
    );
    assert_eq!(loaded.request.extra.crop_size, Some([84.0, 42.0]));
}
