use super::{engine, filters, geometry};
use libvips::{
    enums::{BandFormat, Interpretation},
    voption::{Setter, VOption},
    Vips, VipsImage, VipsSource,
};
use std::sync::{Arc, Weak};

fn source(hdr: bool) -> (VipsImage, Weak<Vec<u8>>) {
    engine::initialize().unwrap();
    let (format, space, pixels) = if hdr {
        (
            BandFormat::Ushort,
            Interpretation::Rgb16,
            [1024u16, 2048, 4096]
                .repeat(29 * 23)
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>(),
        )
    } else {
        (
            BandFormat::Uchar,
            Interpretation::Srgb,
            [11, 22, 33].repeat(29 * 23),
        )
    };
    let image = VipsImage::new_from_memory_copy(&pixels, 29, 23, 3, format)
        .unwrap()
        .copy_with_opts(VOption::new().set("interpretation", space as i32))
        .unwrap();
    let bytes = Arc::new(
        image
            .write_to_buffer_with_opts(
                ".png",
                VOption::new().set("bitdepth", if hdr { 16 } else { 8 }),
            )
            .unwrap(),
    );
    drop(image);
    let weak = Arc::downgrade(&bytes);
    let source = VipsSource::new_from_shared_memory(bytes).unwrap();
    let image = VipsImage::new_from_source(&source, "").unwrap();
    drop(source);
    (image, weak)
}

fn request(options: &str) -> crate::request::Request {
    crate::request::parse(
        &format!("/unsafe/{options}/plain/local:///fixture.png@png"),
        &crate::request::Config::unsafe_local(),
    )
    .unwrap()
}

#[test]
fn smart_crop_detaches_its_resized_analysis_raster_from_the_encoded_source() {
    let (input, weak) = source(false);
    let r = request("rs:fill:7:7/g:sm");
    let (output, _) = geometry::apply(input, &r, false, None).unwrap();
    Vips::thread_shutdown();
    assert!(
        weak.upgrade().is_none(),
        "SmartCrop retains its source graph for a second evaluation"
    );
    assert_eq!((output.get_width(), output.get_height()), (7, 7));
    assert_eq!(output.write_to_memory(), [11, 22, 33].repeat(49));
}

#[test]
fn filters_detach_resized_byte_samples_before_building_the_filter_graph() {
    for effect in ["bl:2", "pix:3"] {
        let (input, weak) = source(false);
        let r = request(&format!("rs:force:17:13/{effect}"));
        let (resized, _) = geometry::apply(input, &r, false, None).unwrap();
        let output = filters::effects(resized, &r).unwrap();
        Vips::thread_shutdown();
        assert!(
            weak.upgrade().is_none(),
            "{effect} retains its upstream resize graph"
        );
        assert_eq!(output.write_to_memory(), [11, 22, 33].repeat(17 * 13));
    }
}

#[test]
fn identity_crop_and_absent_filters_do_not_force_a_memory_copy() {
    let (input, weak) = source(false);
    let r = request("rs:force:29:23/g:sm");
    let (image, _) = geometry::apply(input, &r, false, None).unwrap();
    let image = filters::effects(image, &r).unwrap();
    Vips::thread_shutdown();
    assert!(weak.upgrade().is_some());
    assert_eq!(image.write_to_memory(), [11, 22, 33].repeat(29 * 23));
    drop(image);
    Vips::thread_shutdown();
    assert!(weak.upgrade().is_none());
}

#[test]
fn high_precision_filter_input_keeps_the_existing_lazy_precision_path() {
    let (input, weak) = source(true);
    assert_eq!(input.get_format().unwrap(), BandFormat::Ushort);
    let mut r = request("rs:force:17:13/bl:2");
    r.extra.preserve_hdr = true;
    let (image, _) = geometry::apply(input, &r, false, None).unwrap();
    let image = filters::effects(image, &r).unwrap();
    Vips::thread_shutdown();
    assert!(weak.upgrade().is_some());
    assert_ne!(image.get_format().unwrap(), BandFormat::Uchar);
    assert!(image
        .getpoint(8, 6)
        .unwrap()
        .iter()
        .all(|value| *value > 255.0));
}
