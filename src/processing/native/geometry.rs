//! Native geometry uses the same bounded plans as the builtin engine.
use super::engine::checked;
use crate::{ops as plan, request::Request, Error};
use libvips::{
    enums::{Angle, Direction, Extend, Kernel},
    voption::{Setter, VOption},
    VipsImage,
};

fn size(n: f64) -> Result<i32, Error> {
    if !n.is_finite() || n > 100_000.0 || n < 0.0 {
        return Err("native dimensions exceed limit");
    }
    Ok((n.round_ties_even() as i32).max(1))
}

pub(super) fn resize(
    image: &VipsImage,
    width: i32,
    height: i32,
    nearest: bool,
) -> Result<VipsImage, Error> {
    resize_scale(
        image,
        f64::from(width) / f64::from(image.get_width()),
        f64::from(height) / f64::from(image.get_height()),
        nearest,
    )
}

pub(super) fn resize_scale(
    image: &VipsImage,
    sx: f64,
    sy: f64,
    nearest: bool,
) -> Result<VipsImage, Error> {
    let width = size(f64::from(image.get_width()) * sx)?;
    let height = size(f64::from(image.get_height()) * sy)?;
    checked(width, height)?;
    if sx == 1.0 && sy == 1.0 {
        return Ok(image.clone());
    }
    let source = i64::from(image.get_width()) * i64::from(image.get_height());
    let intermediate = i64::from(image.get_height()) * i64::from(width);
    if intermediate > 16_000_000
        || source + 2 * intermediate + i64::from(width) * i64::from(height) > 100_000_000
    {
        return Err("resize computation budget exceeded");
    }
    let options = VOption::new().set("vscale", sy).set(
        "kernel",
        if nearest {
            Kernel::Nearest as i32
        } else {
            Kernel::Lanczos3 as i32
        },
    );
    if image.hasalpha() {
        let format = image
            .get_format()
            .map_err(|_| "invalid native pixel type")?;
        image
            .premultiply()
            // Preserve the source precision for integer convolution, as in
            // imgproxy's native resize pipeline; floating/HDR types stay typed.
            .and_then(|i| if nearest { Ok(i) } else { i.cast(format) })
            .and_then(|i| i.resize_with_opts(sx, options))
            .and_then(|i| i.unpremultiply())
            .and_then(|i| i.cast(format))
            .map_err(|_| "native alpha resize failed")
    } else {
        image
            .resize_with_opts(sx, options)
            .map_err(|_| "native resize failed")
    }
}

fn crop(
    image: &VipsImage,
    width: i32,
    height: i32,
    gravity: plan::Gravity,
    offsets: [f64; 2],
    smart: bool,
) -> Result<VipsImage, Error> {
    let (width, height) = (
        width.min(image.get_width()).max(1),
        height.min(image.get_height()).max(1),
    );
    if width == image.get_width() && height == image.get_height() {
        return Ok(image.clone());
    }
    if smart {
        // Analysis and the eventual crop both consume these pixels. Keep one
        // bounded raster instead of evaluating the upstream resize twice.
        let image = if super::shared::fits_memory(image) {
            super::shared::materialize(image.clone())?
        } else {
            image.clone()
        };
        return image
            .smartcrop(width, height)
            .map_err(|_| "native smart crop failed");
    }
    let (ax, ay) = plan::alignments(gravity)?;
    let position = |base: i32, child: i32, align: plan::Alignment, offset: f64| {
        let offset = if offset.abs() < 1.0 {
            offset * f64::from(base)
        } else {
            offset
        };
        let offset = offset.round_ties_even() as i128;
        let offset = if matches!(align, plan::Alignment::End) {
            -offset
        } else {
            offset
        };
        (plan::raw_axis_position(base as usize, child as usize, align) + offset)
            .clamp(0, i128::from(base - child)) as i32
    };
    image
        .extract_area(
            position(image.get_width(), width, ax, offsets[0]),
            position(image.get_height(), height, ay, offsets[1]),
            width,
            height,
        )
        .map_err(|_| "native crop failed")
}

pub(super) fn apply(
    mut image: VipsImage,
    r: &Request,
    vector: bool,
    decoded_plan: Option<super::scale::Plan>,
) -> Result<(VipsImage, f64), Error> {
    let o = &r.options;
    let e = &r.extra;
    image = match o.rotation {
        plan::Rotation::R0 => image,
        plan::Rotation::R90 => image
            .rot(Angle::D90)
            .map_err(|_| "native rotation failed")?,
        plan::Rotation::R180 => image
            .rot(Angle::D180)
            .map_err(|_| "native rotation failed")?,
        plan::Rotation::R270 => image
            .rot(Angle::D270)
            .map_err(|_| "native rotation failed")?,
    };
    if o.flip_horizontal {
        image = image
            .flip(Direction::Horizontal)
            .map_err(|_| "native flip failed")?;
    }
    if o.flip_vertical {
        image = image
            .flip(Direction::Vertical)
            .map_err(|_| "native flip failed")?;
    }
    // Crop in final orientation so gravity and fractional offsets share one frame.
    if let Some(c) = o.crop {
        let values = e.crop_size.unwrap_or([c.width as f64, c.height as f64]);
        let dims = [image.get_width(), image.get_height()];
        let mut selected = [0; 2];
        for i in 0..2 {
            selected[i] = if values[i] == 0.0 {
                dims[i]
            } else if values[i] < 1.0 {
                size(f64::from(dims[i]) * values[i])?
            } else {
                size(values[i])?
            };
        }
        image = crop(
            &image,
            selected[0],
            selected[1],
            c.gravity,
            e.crop_offsets,
            e.crop_smart,
        )?;
    }
    let p = match decoded_plan {
        Some(plan) => plan,
        None => super::scale::Plan::new(image.get_width(), image.get_height(), r, vector)?,
    };
    let dpr = p.dpr;
    if e.encoding.linear && p.scale != [1.0; 2] {
        let space = image
            .get_interpretation()
            .map_err(|_| "invalid resize colour space")?;
        let pixel_type = image
            .get_format()
            .map_err(|_| "invalid resize pixel type")?;
        image = image
            .colourspace(libvips::enums::Interpretation::Scrgb)
            .map_err(|_| "linear colour conversion failed")?;
        image = resize_scale(&image, p.scale[0], p.scale[1], false)?;
        image = image
            .colourspace(space)
            .and_then(|i| i.cast(pixel_type))
            .map_err(|_| "linear resize colour restoration failed")?;
    } else {
        image = resize_scale(&image, p.scale[0], p.scale[1], false)?;
    }
    {
        let mut tw = if p.target[0] == 0 {
            image.get_width()
        } else {
            p.target[0]
        };
        let mut th = if p.target[1] == 0 {
            image.get_height()
        } else {
            p.target[1]
        };
        if e.fill_down {
            let scale = (f64::from(image.get_width()) / f64::from(tw))
                .min(f64::from(image.get_height()) / f64::from(th))
                .min(1.0);
            tw = size(f64::from(tw) * scale)?;
            th = size(f64::from(th) * scale)?;
        }
        image = crop(
            &image,
            tw,
            th,
            o.gravity,
            e.offsets.map(|n| if n.abs() < 1.0 { n } else { n * dpr }),
            e.smart,
        )?;
    }
    for aspect in [false, true] {
        if !(if aspect { e.extend_aspect } else { e.extend }) {
            continue;
        }
        let mut w = image.get_width();
        let mut h = image.get_height();
        let tw = if o.width == 0 { w } else { p.target[0].max(1) };
        let th = if o.height == 0 { h } else { p.target[1].max(1) };
        if !aspect {
            w = w.max(tw);
            h = h.max(th);
        }
        if aspect && o.width > 0 && o.height > 0 {
            let ratio = f64::from(tw) / f64::from(th);
            if f64::from(w) / f64::from(h) < ratio {
                w = size(f64::from(h) * ratio)?;
            } else {
                h = size(f64::from(w) / ratio)?;
            }
        }
        checked(w, h)?;
        let (ax, ay) = plan::alignments(if aspect {
            e.aspect_gravity
        } else {
            e.extend_gravity
        })?;
        let offsets = if aspect {
            e.aspect_offsets
        } else {
            e.extend_offsets
        };
        let off = |n: f64, dim: i32| {
            if n.abs() < 1.0 {
                n * f64::from(dim)
            } else {
                n * dpr
            }
        };
        let x = plan::watermark_position(
            w as usize,
            image.get_width() as usize,
            ax,
            off(offsets[0], w).round_ties_even() as i32,
        )
        .clamp(0, i128::from(w - image.get_width())) as i32;
        let y = plan::watermark_position(
            h as usize,
            image.get_height() as usize,
            ay,
            off(offsets[1], h).round_ties_even() as i32,
        )
        .clamp(0, i128::from(h - image.get_height())) as i32;
        if !image.hasalpha() {
            image = image.addalpha().map_err(|_| "native alpha failed")?;
        }
        image = image
            .embed_with_opts(
                x,
                y,
                w,
                h,
                VOption::new().set("extend", Extend::Black as i32),
            )
            .map_err(|_| "native extend failed")?;
    }
    if image.get_width().max(image.get_height()) as usize > e.max_result_dimension {
        let factor =
            e.max_result_dimension as f64 / f64::from(image.get_width().max(image.get_height()));
        image = resize(
            &image,
            size(f64::from(image.get_width()) * factor)?,
            size(f64::from(image.get_height()) * factor)?,
            false,
        )?;
    }
    Ok((image, dpr))
}

pub(super) fn decorate(mut image: VipsImage, r: &Request, d: f64) -> Result<VipsImage, Error> {
    let p = r.options.padding;
    let sides = [p.top, p.right, p.bottom, p.left].map(|n| (n as f64 * d).round_ties_even() as i32);
    if sides.iter().any(|&n| n > 0) {
        let w = image
            .get_width()
            .checked_add(sides[1])
            .and_then(|n| n.checked_add(sides[3]))
            .ok_or("padding overflow")?;
        let h = image
            .get_height()
            .checked_add(sides[0])
            .and_then(|n| n.checked_add(sides[2]))
            .ok_or("padding overflow")?;
        checked(w, h)?;
        if !image.hasalpha() {
            image = image.addalpha().map_err(|_| "native alpha failed")?;
        }
        image = image
            .embed_with_opts(
                sides[3],
                sides[0],
                w,
                h,
                VOption::new().set("extend", Extend::Black as i32),
            )
            .map_err(|_| "native padding failed")?;
    }
    if let Some(color) = r.options.background {
        image = background(image, color, r.extra.preserve_hdr)?;
    }
    Ok(image)
}

pub(super) fn background(
    mut image: VipsImage,
    color: [u8; 4],
    preserve_hdr: bool,
) -> Result<VipsImage, Error> {
    if !image.hasalpha() {
        return Ok(image);
    }
    if image.hasalpha()
        && (color[0] != color[1] || color[1] != color[2])
        && matches!(
            image.get_interpretation(),
            Ok(libvips::enums::Interpretation::BW | libvips::enums::Interpretation::Grey16)
        )
    {
        image = image
            .colourspace(if preserve_hdr {
                libvips::enums::Interpretation::Rgb16
            } else {
                libvips::enums::Interpretation::Srgb
            })
            .map_err(|_| "native background colour conversion failed")?;
    }
    let scale = if matches!(
        image.get_interpretation(),
        Ok(libvips::enums::Interpretation::Rgb16 | libvips::enums::Interpretation::Grey16)
    ) {
        257.0
    } else {
        1.0
    };
    let background = color[..3]
        .iter()
        .map(|&c| f64::from(c) * scale)
        .collect::<Vec<_>>();
    image = image
        .flatten_with_opts(VOption::new().set("background", background.as_slice()))
        .map_err(|_| "native background failed")?;
    Ok(image)
}

#[cfg(test)]
mod alpha_tests {
    use super::*;

    fn background_request(color: &str) -> Request {
        crate::request::parse(
            &format!("/unsafe/bg:{color}/plain/local:///fixture.png@png"),
            &crate::request::Config::unsafe_local(),
        )
        .unwrap()
    }

    #[test]
    fn background_preserves_opaque_rgb_and_gray_samples_and_bands() {
        super::super::engine::initialize().unwrap();
        for (pixels, bands, space) in [
            (vec![9u8, 27, 81], 3, libvips::enums::Interpretation::Srgb),
            (vec![91u8], 1, libvips::enums::Interpretation::BW),
        ] {
            let image = VipsImage::new_from_memory_copy(
                &pixels,
                1,
                1,
                bands,
                libvips::enums::BandFormat::Uchar,
            )
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", space as i32))
            .unwrap();
            for color in ["ffffff", "080b12"] {
                let result = decorate(image.clone(), &background_request(color), 1.0).unwrap();
                assert_eq!(result.get_bands(), bands);
                assert_eq!(
                    result.getpoint(0, 0).unwrap(),
                    pixels.iter().map(|p| f64::from(*p)).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn background_still_flattens_real_transparency() {
        super::super::engine::initialize().unwrap();
        let image = VipsImage::new_from_memory_copy(
            &[100u8, 50, 10, 0],
            1,
            1,
            4,
            libvips::enums::BandFormat::Uchar,
        )
        .unwrap()
        .copy_with_opts(VOption::new().set(
            "interpretation",
            libvips::enums::Interpretation::Srgb as i32,
        ))
        .unwrap();
        let result = decorate(image, &background_request("080b12"), 1.0).unwrap();
        assert_eq!(result.get_bands(), 3);
        assert_eq!(result.getpoint(0, 0).unwrap(), [8.0, 11.0, 18.0]);
    }

    #[test]
    fn identity_geometry_does_not_charge_nonexistent_convolution_work() {
        super::super::engine::initialize().unwrap();
        let image = VipsImage::black(4096, 4096).unwrap();
        let result = resize_scale(&image, 1.0, 1.0, false).unwrap();
        assert_eq!([result.get_width(), result.get_height()], [4096, 4096]);
        let cross_axis = VipsImage::black(1, 4001).unwrap();
        assert!(matches!(
            resize_scale(&cross_axis, 4001.0, 1.0 / 4001.0, false),
            Err("resize computation budget exceeded")
        ));
    }

    #[test]
    fn alpha_resize_uses_the_input_precision_for_premultiplied_samples() {
        super::super::engine::initialize().unwrap();
        let data = [128u8, 64, 32, 128].repeat(64);
        let image =
            VipsImage::new_from_memory_copy(&data, 8, 8, 4, libvips::enums::BandFormat::Uchar)
                .unwrap()
                .copy_with_opts(VOption::new().set(
                    "interpretation",
                    libvips::enums::Interpretation::Srgb as i32,
                ))
                .unwrap();
        let resized = resize_scale(&image, 0.5, 0.5, false).unwrap();
        assert_eq!(resized.getpoint(0, 0).unwrap(), [127., 63., 31., 128.]);
    }
}
