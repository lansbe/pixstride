//! Finish bounded RGB downscaling before constructing a sharpen graph.
use super::scale::Plan;
use crate::{
    ops,
    request::{OutputFormat, Request},
    Error,
};
use libvips::{
    enums::{BandFormat, Interpretation},
    VipsImage,
};

fn target(image: &VipsImage, p: &Plan, r: &Request) -> Option<[i32; 2]> {
    if r.effects.sharpen_sigma <= 0.0
        || r.format != Some(OutputFormat::Jpeg)
        || !r.extra.force_formats.is_empty()
        || !r.strip_metadata
        || !r.strip_color_profile
        || r.extra.keep_copyright
        || r.extra.preserve_hdr
        || r.extra.encoding.linear
        || r.extra.enforce_thumbnail
        || r.extra.max_bytes != 0
        || !matches!(
            r.options.mode,
            ops::ResizeMode::Fit | ops::ResizeMode::Force
        )
        || r.options.crop.is_some()
        || r.options.rotation != ops::Rotation::R0
        || r.options.flip_horizontal
        || r.options.flip_vertical
        || r.effects.trim.is_some()
        || image.get_bands() != 3
        || image.get_format().ok() != Some(BandFormat::Uchar)
        || image.get_interpretation().ok() != Some(Interpretation::Srgb)
        || image.get_typeof("icc-profile-data").unwrap_or(0) != 0
        || p.scale == [1.0; 2]
        || p.scale
            .iter()
            .any(|s| !s.is_finite() || *s <= 0.0 || *s > 1.0)
    {
        return None;
    }
    dimensions([image.get_width(), image.get_height()], p)
}

fn dimensions(source: [i32; 2], p: &Plan) -> Option<[i32; 2]> {
    if source.iter().any(|n| !(1..=100_000).contains(n)) {
        return None;
    }
    let output = std::array::from_fn(|i| {
        (f64::from(source[i]) * p.scale[i])
            .round_ties_even()
            .max(1.0) as i32
    });
    if output.iter().any(|n| !(1..=100_000).contains(n)) {
        return None;
    }
    let [w, h] = source.map(|n| n as u64);
    let [tw, th] = output.map(|n| n as u64);
    let intermediate = (h * tw).max(w * th);
    if intermediate > 16_000_000
        || w * h + 2 * intermediate + tw * th > 100_000_000
        || 3 * (w * h + 2 * intermediate + tw * th) + 128 * (w + h + tw + th) + 4096
            > crate::MAX_FILE as u64
    {
        return None;
    }
    Some(output)
}

pub(super) fn apply(
    image: VipsImage,
    mut plan: Plan,
    r: &Request,
) -> Result<(VipsImage, Plan), Error> {
    if target(&image, &plan, r).is_none() {
        return Ok((image, plan));
    }
    // Finish the same native Lanczos operation, then release its decoder,
    // convolution graph and thread regions before constructing the filter.
    let resized = super::geometry::resize_scale(&image, plan.scale[0], plan.scale[1], false)?;
    drop(image);
    let image = super::shared::materialize(resized)?;
    plan.scale = [1.0; 2];
    Ok((image, plan))
}

#[cfg(test)]
mod tests {
    use crate::request::{self, Config};
    use libvips::{
        enums::Interpretation,
        voption::{Setter, VOption},
        VipsImage,
    };

    fn request(options: &str) -> request::Request {
        request::parse(
            &format!("/unsafe/{options}/plain/local:///fixture.jpg@jpg"),
            &Config::unsafe_local(),
        )
        .unwrap()
    }
    fn image(bands: i32) -> VipsImage {
        super::super::engine::initialize().unwrap();
        VipsImage::new_from_owned_u8(vec![128; 32 * 24 * bands as usize], 32, 24, bands)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            .unwrap()
    }
    #[test]
    fn bounded_rgb_sharpen_resize_finishes_geometry_and_keeps_dpr_and_color() {
        let r = request("rs:fit:8:8/dpr:2/sh:1");
        let p = super::super::scale::Plan::new(32, 24, &r, false).unwrap();
        let dpr = p.dpr;
        let (image, p) = super::apply(image(3), p, &r).unwrap();
        assert_eq!([image.get_width(), image.get_height()], [16, 12]);
        assert_eq!(image.getpoint(8, 6).unwrap(), [128.; 3]);
        assert_eq!(p.scale, [1.; 2]);
        assert_eq!(p.dpr, dpr);
    }
    #[test]
    fn metadata_alpha_precision_and_other_requests_keep_the_native_resizer() {
        let base = request("rs:fit:8:8/sh:1");
        let mut variants = vec![request("rs:fit:8:8")];
        for setting in 0..7 {
            let mut r = request("rs:fit:8:8/sh:1");
            match setting {
                0 => r.strip_metadata = false,
                1 => r.strip_color_profile = false,
                2 => r.extra.encoding.linear = true,
                3 => r.extra.preserve_hdr = true,
                4 => r.extra.keep_copyright = true,
                5 => r.extra.max_bytes = 100,
                _ => r
                    .extra
                    .force_formats
                    .push(crate::request::OutputFormat::Png),
            }
            variants.push(r);
        }
        for r in variants {
            let p = super::super::scale::Plan::new(32, 24, &r, false).unwrap();
            let (image, _) = super::apply(image(3), p, &r).unwrap();
            assert_eq!([image.get_width(), image.get_height()], [32, 24]);
        }
        let p = super::super::scale::Plan::new(32, 24, &base, false).unwrap();
        assert_eq!(super::apply(image(4), p, &base).unwrap().0.get_width(), 32);
    }
    #[test]
    fn separated_resize_preserves_every_native_lanczos_sample() {
        let r = request("rs:fit:17:17/sh:1");
        let p = super::super::scale::Plan::new(97, 61, &r, false).unwrap();
        let pixels = (0..97 * 61 * 3)
            .map(|i| ((i * 71 + i / 13) % 256) as u8)
            .collect();
        super::super::engine::initialize().unwrap();
        let source = VipsImage::new_from_owned_u8(pixels, 97, 61, 3)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            .unwrap();
        let expected =
            super::super::geometry::resize_scale(&source, p.scale[0], p.scale[1], false).unwrap();
        let expected = super::super::raster::export(&expected).unwrap();
        let (actual, _) = super::apply(source, p, &r).unwrap();
        let actual = super::super::raster::export(&actual).unwrap();
        assert_eq!(actual.as_ref(), expected.as_ref());
    }
    #[test]
    fn oversized_convolution_scratch_keeps_the_bounded_native_route() {
        let r = request("rs:fit:4096:4096/sh:1");
        let p = super::super::scale::Plan::new(16000, 16000, &r, false).unwrap();
        assert!(super::dimensions([16000, 16000], &p).is_none());
        assert!(super::dimensions([0, 24], &p).is_none());
    }
    #[test]
    fn downscaled_pixels_do_not_retain_the_decoder_or_encoded_source() {
        use std::sync::Arc;
        let source = Arc::new(image(3).write_to_buffer(".jpg").unwrap());
        let weak = Arc::downgrade(&source);
        let lazy = super::super::shared::load(
            &source,
            VOption::new().set("access", libvips::enums::Access::Sequential as i32),
        )
        .unwrap();
        drop(source);
        let r = request("rs:fit:8:8/sh:1");
        let p = super::super::scale::Plan::new(32, 24, &r, false).unwrap();
        let (resized, _) = super::apply(lazy, p, &r).unwrap();
        assert!(
            weak.upgrade().is_none(),
            "decoder retained its source through sharpening"
        );
        assert_eq!(resized.getpoint(4, 3).unwrap(), [128.; 3]);
    }
}
