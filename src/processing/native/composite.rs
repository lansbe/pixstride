//! Watermark colour conversion, scaling, repetition and alpha composition.
use super::{
    engine::{checked, load},
    geometry,
};
use crate::{request::Request, Error};
use libvips::{
    enums::{BlendMode, Extend, Interpretation},
    voption::{Setter, VOption},
    VipsImage,
};

pub(super) fn watermark(
    mut image: VipsImage,
    data: &[u8],
    r: &Request,
) -> Result<VipsImage, Error> {
    let Some(spec) = r.watermark.as_ref().filter(|s| s.opacity > 0.0) else {
        return Ok(image);
    };
    if data.is_empty() {
        return Err("watermark image is not configured");
    }
    let mut mark = load(data, 1, false)?;
    checked(mark.get_width(), mark.get_height())?;
    if matches!(
        image.get_interpretation(),
        Ok(Interpretation::BW | Interpretation::Grey16)
    ) {
        let space = if r.extra.preserve_hdr {
            Interpretation::Rgb16
        } else {
            Interpretation::Srgb
        };
        image = image
            .colourspace(space)
            .map_err(|_| "native composition colour conversion failed")?;
    }
    let space = image
        .get_interpretation()
        .map_err(|_| "invalid composition colour space")?;
    let pixel_type = image
        .get_format()
        .map_err(|_| "invalid watermark pixel type")?;
    if !r.strip_color_profile && image.get_typeof("icc-profile-data").unwrap_or(0) != 0 {
        return Err("watermark composition requires conversion of the source ICC profile");
    }
    mark = mark.autorot().map_err(|_| "watermark orientation failed")?;
    if mark.get_typeof("icc-profile-data").unwrap_or(0) != 0 {
        mark = mark
            .icc_transform_with_opts(
                "srgb",
                VOption::new()
                    .set("embedded", true)
                    .set("depth", if r.extra.preserve_hdr { 16 } else { 8 }),
            )
            .map_err(|_| "watermark ICC conversion failed")?;
    }
    mark = mark
        .colourspace(space)
        .map_err(|_| "native watermark colour conversion failed")?;
    if spec.scale > 0.0 {
        let width = (f64::from(image.get_width()) * spec.scale)
            .round_ties_even()
            .max(1.0);
        let height = (f64::from(image.get_height()) * spec.scale)
            .round_ties_even()
            .max(1.0);
        let scale =
            (width / f64::from(mark.get_width())).min(height / f64::from(mark.get_height()));
        mark = geometry::resize_scale(&mark, scale, scale, false)?;
    }

    if !mark.hasalpha() {
        mark = mark
            .addalpha()
            .map_err(|_| "native watermark alpha failed")?;
    }
    let mut factors = vec![1.0; mark.get_bands() as usize];
    *factors.last_mut().unwrap() = spec.opacity * r.extra.base_opacity;
    mark = mark
        .linear(&factors, &vec![0.0; factors.len()])
        .map_err(|_| "native watermark opacity failed")?;
    let off = |n: f64, dim: i32| -> i32 {
        (if n.abs() < 1.0 {
            n * f64::from(dim)
        } else {
            n * r.extra.dpr
        })
        .round_ties_even() as i32
    };
    let ox = off(spec.offset_x, image.get_width());
    let oy = off(spec.offset_y, image.get_height());
    let (x, y) = if r.extra.watermark_repeat {
        let w = mark
            .get_width()
            .checked_add(ox)
            .ok_or("watermark spacing overflow")?;
        let h = mark
            .get_height()
            .checked_add(oy)
            .ok_or("watermark spacing overflow")?;
        checked(w, h)?;
        mark = mark
            .embed_with_opts(
                ox / 2,
                oy / 2,
                w,
                h,
                VOption::new().set("extend", Extend::Black as i32),
            )
            .map_err(|_| "native watermark spacing failed")?;
        let across = ((image.get_width() + w - 1) / w) | 1;
        let down = ((image.get_height() + h - 1) / h) | 1;
        let (rw, rh) = (
            w.checked_mul(across).ok_or("watermark tile overflow")?,
            h.checked_mul(down).ok_or("watermark tile overflow")?,
        );
        checked(rw, rh)?;
        mark = mark
            .replicate(across, down)
            .and_then(|i| {
                i.extract_area(
                    (rw - image.get_width()) / 2,
                    (rh - image.get_height()) / 2,
                    image.get_width(),
                    image.get_height(),
                )
            })
            .map_err(|_| "native watermark tiling failed")?;
        (0, 0)
    } else {
        let (ax, ay) = crate::ops::alignments(spec.gravity)?;
        (
            crate::ops::watermark_position(
                image.get_width() as usize,
                mark.get_width() as usize,
                ax,
                ox,
            ) as i32,
            crate::ops::watermark_position(
                image.get_height() as usize,
                mark.get_height() as usize,
                ay,
                oy,
            ) as i32,
        )
    };
    if !image.hasalpha() {
        image = image.addalpha().map_err(|_| "native alpha failed")?;
    }
    image
        .composite2_with_opts(
            &mark,
            BlendMode::Over,
            VOption::new()
                .set("x", x)
                .set("y", y)
                .set("compositing_space", space as i32),
        )
        .and_then(|i| i.cast(pixel_type))
        .map_err(|_| "native watermark composition failed")
}
