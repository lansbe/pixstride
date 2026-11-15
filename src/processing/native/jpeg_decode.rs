//! Native access policy and JPEG plans reduce work without changing geometry.
use super::scale::Plan;
use crate::{ops::Rotation, request::Request, Error};
use libvips::{
    enums::{Access, BandFormat, FailOn, Interpretation},
    voption::{Setter, VOption},
    VipsImage,
};

pub(super) fn source_access(
    format: Option<crate::request::OutputFormat>,
    head: &VipsImage,
    r: &Request,
) -> Access {
    if format == Some(crate::request::OutputFormat::Tiff)
        && sequential(r)
        && Plan::new(head.get_width(), head.get_height(), r, false)
            .is_ok_and(|p| reads_all_rows(&p, head.get_height()))
        && (!r.auto_rotate || head.get_int("orientation").unwrap_or(1) == 1)
    {
        Access::Sequential
    } else {
        Access::Random
    }
}

fn plan(width: i32, height: i32, r: &Request) -> Result<Option<(i32, Plan)>, Error> {
    if r.options.crop.is_some()
        || r.options.rotation != Rotation::R0
        || r.options.flip_horizontal
        || r.options.flip_vertical
        || r.effects.trim.is_some()
        || r.extra.encoding.linear
        || r.extra.enforce_thumbnail
    {
        return Ok(None);
    }
    let p = Plan::new(width, height, r, false)?;
    let largest = p.scale[0].max(p.scale[1]);
    let shrink = [8, 4, 2]
        .into_iter()
        .find(|n| largest <= 1.0 / f64::from(*n))
        .unwrap_or(1);
    Ok(Some((shrink, p)))
}

fn adjust(mut p: Plan, original: [i32; 2], decoded: [i32; 2]) -> Plan {
    for axis in 0..2 {
        p.scale[axis] *= f64::from(original[axis]) / f64::from(decoded[axis]);
    }
    p
}

fn sequential(r: &Request) -> bool {
    r.extra.max_bytes == 0
        && !r.extra.encoding.png_quantize
        && !r.extra.smart
        && r.options.crop.is_none()
        && r.options.rotation == Rotation::R0
        && !r.options.flip_vertical
        && r.options.padding == Default::default()
        && r.options.background.is_none()
        && r.options.watermark.is_none()
        && r.watermark.is_none()
        && !r.extra.extend
        && !r.extra.extend_aspect
        && !r.extra.fill_down
        && !r.extra.enforce_thumbnail
        && r.effects.trim.is_none()
        && r.effects.pixelate <= 1
}

fn reads_all_rows(plan: &Plan, height: i32) -> bool {
    // Final fill/auto/min/zoom geometry can discard the decoder's last rows.
    // Only horizontal cropping is safe for a single-pass source; random access
    // forces complete decoding before any implicit vertical crop is evaluated.
    plan.target[1] == 0 || f64::from(height) * plan.scale[1] <= f64::from(plan.target[1])
}

pub(super) fn crop_input(
    data: &std::sync::Arc<Vec<u8>>,
    head: &VipsImage,
    r: &Request,
) -> Result<Option<VipsImage>, Error> {
    if r.options.crop.is_none()
        || !super::shared::fits_memory(head)
        || !matches!(head.get_bands(), 1 | 3)
        || !matches!(
            head.get_interpretation(),
            Ok(Interpretation::Srgb | Interpretation::BW)
        )
    {
        return Ok(None);
    }
    let image = super::shared::load(
        data,
        VOption::new()
            .set("access", Access::Sequential as i32)
            .set("fail_on", FailOn::Error as i32),
    )?;
    // Read every source row before cropping, preserving complete-stream
    // validation without a random-access decoder cache over the same raster.
    super::shared::materialize(image).map(Some)
}

pub(super) fn try_load(
    data: &std::sync::Arc<Vec<u8>>,
    head: &VipsImage,
    r: &Request,
) -> Result<Option<(VipsImage, Plan)>, Error> {
    if head.get_format().ok() != Some(BandFormat::Uchar)
        || !matches!(
            head.get_interpretation().ok(),
            Some(Interpretation::Srgb | Interpretation::BW)
        )
        || !matches!(head.get_bands(), 1 | 3)
        || (r.auto_rotate && head.get_int("orientation").unwrap_or(1) != 1)
    {
        return Ok(None);
    }
    let Some((shrink, p)) = plan(head.get_width(), head.get_height(), r)? else {
        return Ok(None);
    };
    let access = if sequential(r) && reads_all_rows(&p, head.get_height()) {
        Access::Sequential
    } else {
        Access::Random
    };
    if shrink == 1 && access == Access::Random {
        return Ok(None);
    }
    let image = super::shared::load(
        data,
        VOption::new()
            .set("shrink", shrink)
            .set("access", access as i32)
            .set("fail_on", FailOn::Error as i32),
    )
    .map_err(|_| "native JPEG reduced decode failed")?;
    super::engine::checked(image.get_width(), image.get_height())?;
    let p = adjust(
        p,
        [head.get_width(), head.get_height()],
        [image.get_width(), image.get_height()],
    );
    Ok(Some((image, p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{self, Config};

    fn request(options: &str) -> request::Request {
        request::parse(
            &format!("/unsafe/{options}/plain/local:///fixture.jpg@jpg"),
            &Config::unsafe_local(),
        )
        .unwrap()
    }

    #[test]
    fn shrinks_only_when_both_axes_have_enough_source_samples() {
        let fit = request("rs:fit:320:320");
        assert_eq!(plan(2048, 920, &fit).unwrap().unwrap().0, 4);
        let fill = request("rs:fill:256:256");
        assert_eq!(plan(2048, 920, &fill).unwrap().unwrap().0, 2);
        assert_eq!(plan(320, 144, &fit).unwrap().unwrap().0, 1);
        assert!(plan(2048, 920, &request("c:800:600:ce")).unwrap().is_none());
        assert!(plan(2048, 920, &request("rot:90")).unwrap().is_none());
        assert!(plan(2048, 920, &request("rs:fit:320:320/trim:10"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn odd_jpeg_sizes_keep_the_unrounded_resize_plan() {
        let r = request("rs:fit:25:25");
        let (shrink, p) = plan(101, 71, &r).unwrap().unwrap();
        assert_eq!(shrink, 4);
        let p = adjust(p, [101, 71], [26, 18]);
        assert_eq!((26.0 * p.scale[0]).round_ties_even(), 25.0);
        assert_eq!((18.0 * p.scale[1]).round_ties_even(), 18.0);
        assert_eq!(p.target, [25, 25]);
    }

    #[test]
    fn repeated_encoding_and_row_rewinds_keep_random_access() {
        assert!(sequential(&request("rs:fit:320:320")));
        assert!(!sequential(&request("rs:fit:320:320/mb:500")));
        assert!(sequential(&request("rs:fit:320:320/sh:1")));
        assert!(sequential(&request("rs:fit:320:320/bl:2")));
        assert!(!sequential(&request("rs:fit:320:320/trim:10")));
        assert!(!sequential(&request("rot:90")));
    }

    #[test]
    fn tiff_streaming_requires_one_monotonic_pass() {
        use crate::request::OutputFormat::{Png, Tiff};
        super::super::engine::initialize().unwrap();
        let mut head = VipsImage::new_from_owned_u8(vec![0; 6], 2, 1, 3).unwrap();
        assert_eq!(
            source_access(Some(Tiff), &head, &request("rs:fit:640:640")),
            Access::Sequential
        );
        for options in [
            "rot:90",
            "c:80:60:ce",
            "rs:fit:640:640/mb:500",
            "trim:10",
            "rs:fit:640:640/g:sm",
        ] {
            assert_eq!(
                source_access(Some(Tiff), &head, &request(options)),
                Access::Random
            );
        }
        assert_eq!(
            source_access(Some(Png), &head, &request("rs:fit:640:640")),
            Access::Random
        );
        head.set_int("orientation", 6).unwrap();
        assert_eq!(
            source_access(Some(Tiff), &head, &request("rs:fit:640:640")),
            Access::Random
        );
        assert_eq!(
            source_access(Some(Tiff), &head, &request("ar:0/rs:fit:640:640")),
            Access::Sequential
        );
    }

    #[test]
    fn cropped_jpeg_materialization_keeps_pixels_and_releases_encoded_source() {
        use std::sync::Arc;
        super::super::engine::initialize().unwrap();
        let pixels = (0..67 * 43 * 3).map(|n| (n * 73 + 9) as u8).collect();
        let image = VipsImage::new_from_owned_u8(pixels, 67, 43, 3).unwrap();
        let data = Arc::new(image.write_to_buffer(".jpg").unwrap());
        drop(image);
        let weak = Arc::downgrade(&data);
        let head = super::super::shared::load(&data, VOption::new()).unwrap();
        let expected = super::super::raster::export(&head).unwrap();
        let r = request("c:19:13:ce/rs:fit:9:9");
        let eager = crop_input(&data, &head, &r)
            .unwrap()
            .expect("bounded crop route");
        drop(head);
        drop(data);
        libvips::Vips::thread_shutdown();
        assert!(weak.upgrade().is_none(), "materialized JPEG retains source");
        assert_eq!(&*super::super::raster::export(&eager).unwrap(), &*expected);
        let (cropped, _) = super::super::geometry::apply(eager, &r, false, None).unwrap();
        assert_eq!((cropped.get_width(), cropped.get_height()), (9, 6));
        let pixels = super::super::raster::export(&cropped).unwrap();
        assert_eq!(pixels.len(), 9 * 6 * 3);
    }

    #[test]
    fn implicit_vertical_crops_reject_truncated_jpeg_tails() {
        super::super::engine::initialize().unwrap();
        let mut state = 1u32;
        let pixels = (0..256 * 1024 * 3)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        let image = VipsImage::new_from_owned_u8(pixels, 256, 1024, 3).unwrap();
        let data = image
            .write_to_buffer_with_opts(".jpg", VOption::new().set("Q", 90))
            .unwrap();
        drop(image);
        for options in [
            "rs:fill:128:16/g:ce",
            "rs:fill:128:16/g:no",
            "rs:auto:64:128/g:ce",
        ] {
            let mut r = request(options);
            r.format = Some(crate::request::OutputFormat::Png);
            assert!(super::super::engine::process(data.clone(), &r, &[]).is_ok());
            let truncated = data[..data.len() * 9 / 10].to_vec();
            assert!(
                super::super::engine::process(truncated, &r, &[]).is_err(),
                "accepted truncated JPEG with {options}"
            );
        }
    }

    #[test]
    fn tiff_vertical_crop_requires_complete_source_access() {
        super::super::engine::initialize().unwrap();
        let head = VipsImage::new_from_owned_u8(vec![0; 4 * 16 * 3], 4, 16, 3).unwrap();
        for options in ["rs:fill:4:1", "rs:auto:2:4", "rs:fill-down:4:1"] {
            assert_eq!(
                source_access(
                    Some(crate::request::OutputFormat::Tiff),
                    &head,
                    &request(options)
                ),
                Access::Random,
                "sequential TIFF can discard rows with {options}"
            );
        }
    }
}
