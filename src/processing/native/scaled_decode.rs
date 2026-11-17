//! Reduced raster decoding and direct vector rendering preserve reference samples.
use crate::{
    ops::{Rotation, TransformOptions},
    request::{OutputFormat, Request},
    Error,
};
use libvips::{
    enums::{Access, FailOn},
    voption::{Setter, VOption},
    VipsImage,
};
use std::sync::Arc;

pub(super) struct Loaded {
    pub image: VipsImage,
    pub plan: super::scale::Plan,
    pub request: Request,
}

fn selected_crop(source: [i32; 2], r: &Request) -> Result<[i32; 2], Error> {
    let Some(crop) = r.options.crop else {
        return Ok(source);
    };
    let values = r
        .extra
        .crop_size
        .unwrap_or([crop.width as f64, crop.height as f64]);
    let mut selected = source;
    for axis in 0..2 {
        let value = values[axis];
        if !value.is_finite() || value < 0.0 {
            return Err("invalid native source crop");
        }
        selected[axis] = if value == 0.0 {
            source[axis]
        } else if value < 1.0 {
            (f64::from(source[axis]) * value).round().max(1.0) as i32
        } else {
            value.min(f64::from(source[axis])).trunc().max(1.0) as i32
        };
    }
    Ok(selected)
}

// Native worker requests never carry a raster watermark (it is serde-skipped).
// The caller retains its original request for preparation, effects and encoding.
fn geometry_request(r: &Request) -> Request {
    let o = &r.options;
    Request {
        source: r.source.clone(),
        extra: r.extra.clone(),
        options: TransformOptions {
            width: o.width,
            height: o.height,
            mode: o.mode,
            gravity: o.gravity,
            enlarge: o.enlarge,
            rotation: o.rotation,
            flip_horizontal: o.flip_horizontal,
            flip_vertical: o.flip_vertical,
            padding: o.padding,
            background: o.background,
            watermark: None,
            method: o.method,
            crop: o.crop,
        },
        format: r.format,
        quality: r.quality,
        auto_rotate: r.auto_rotate,
        strip_metadata: r.strip_metadata,
        strip_color_profile: r.strip_color_profile,
        effects: r.effects,
        watermark: r.watermark,
    }
}

pub(super) fn try_load(
    data: &Arc<Vec<u8>>,
    head: &VipsImage,
    r: &Request,
    format: OutputFormat,
) -> Result<Option<Loaded>, Error> {
    let vector = format == OutputFormat::Svg;
    if !matches!(format, OutputFormat::Webp | OutputFormat::Svg)
        || (r.auto_rotate && head.get_int("orientation").unwrap_or(1) != 1)
        || r.options.rotation != Rotation::R0
        || r.options.flip_horizontal
        || r.options.flip_vertical
        || r.options.watermark.is_some()
        || r.effects.trim.is_some()
        || r.extra.encoding.linear
        || r.extra.enforce_thumbnail
        || head.get_n_pages() > 1
    {
        return Ok(None);
    }
    let source = [head.get_width(), head.get_height()];
    super::engine::checked(source[0], source[1])?;
    let selected = selected_crop(source, r)?;
    let mut plan = super::scale::Plan::new(selected[0], selected[1], r, vector)?;
    // Rounded original dimensions matter: an arbitrary floating decoder scale
    // can otherwise miss the intended edge by one pixel.
    let rendered = std::array::from_fn::<_, 2, _>(|axis| {
        (f64::from(source[axis]) * plan.scale[axis])
            .round()
            .max(1.0)
    });
    let preshrink = (f64::from(source[0]) / rendered[0]).min(f64::from(source[1]) / rendered[1]);
    if preshrink == 1.0 || (!vector && preshrink < 1.0) {
        return Ok(None);
    }
    let scale = 1.0 / preshrink;
    let mut expected = [0; 2];
    for axis in 0..2 {
        // Check the entire uniform vector render, including a potentially huge
        // axis that a subsequent force resize/crop would discard. Ceil bounds
        // loader rounding conservatively before allocating any output pixels.
        let dimension = f64::from(source[axis]) * scale;
        if !dimension.is_finite() || !(0.0..=100_000.0).contains(&dimension) {
            return Err("native reduced decode dimensions exceed limit");
        }
        expected[axis] = dimension.ceil().max(1.0) as i32;
    }
    super::engine::checked(expected[0], expected[1])?;
    let options = VOption::new()
        .set("access", Access::Random as i32)
        .set("fail_on", FailOn::Error as i32);
    let options = if vector {
        // The engine's header load already validated/sanitized this SVG. Match
        // reference DPI compensation while retaining the shared encoded owner.
        options.set("dpi", 96.0).set("scale", scale * 72.0 / 96.0)
    } else {
        options.set("scale", scale)
    };
    let image = super::shared::load(data, options)?;
    let decoded = [image.get_width(), image.get_height()];
    super::engine::checked(decoded[0], decoded[1])?;
    let axis_preshrink =
        std::array::from_fn::<_, 2, _>(|axis| f64::from(source[axis]) / f64::from(decoded[axis]));
    for axis in 0..2 {
        plan.scale[axis] *= axis_preshrink[axis];
        if (f64::from(decoded[axis]) * plan.scale[axis]).round() == f64::from(decoded[axis]) {
            plan.scale[axis] = 1.0;
        }
    }
    let mut request = geometry_request(r);
    if let Some(crop) = &mut request.options.crop {
        let adjusted = std::array::from_fn::<_, 2, _>(|axis| {
            (f64::from(selected[axis]) / axis_preshrink[axis])
                .round()
                .max(1.0)
        });
        crop.width = adjusted[0] as usize;
        crop.height = adjusted[1] as usize;
        request.extra.crop_size = Some(adjusted);
        for (offset, preshrink) in request.extra.crop_offsets.iter_mut().zip(axis_preshrink) {
            if offset.abs() >= 1.0 {
                *offset = (*offset / preshrink).round_ties_even();
            }
        }
    }
    Ok(Some(Loaded {
        image,
        plan,
        request,
    }))
}

#[cfg(test)]
#[path = "scaled_decode_tests.rs"]
mod tests;
