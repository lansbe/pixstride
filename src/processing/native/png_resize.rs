//! Bounded premultiplied Lanczos3 for ordinary RGBA8 PNG-to-JPEG resizing.
use super::scale::Plan;
use crate::{
    ops,
    request::{OutputFormat, Request},
    Error,
};
use fast_image_resize::{images::Image, PixelType, ResizeOptions, Resizer};

#[path = "png_resize_strips.rs"]
pub(super) mod strips;

fn dimensions(source: [i32; 2], plan: &Plan) -> Result<Option<[i32; 2]>, Error> {
    if source.iter().any(|n| !(1..=100_000).contains(n)) {
        return Err("invalid alpha resize dimensions");
    }
    let target = std::array::from_fn(|axis| {
        (f64::from(source[axis]) * plan.scale[axis])
            .round_ties_even()
            .max(1.0) as i32
    });
    if target.iter().any(|n| !(1..=100_000).contains(n)) {
        return Err("invalid alpha resize target");
    }
    let [w, h] = source.map(|n| n as u64);
    let [tw, th] = target.map(|n| n as u64);
    let intermediate = (h * tw).max(w * th);
    // Source, premultiplied source, destination and convolution scratch all fit
    // this ceiling; no persistent resizer or thread pool retains them afterwards.
    if intermediate > 16_000_000
        || w * h + 2 * intermediate + tw * th > 100_000_000
        || (2 * w * h + intermediate + tw * th) * 4 + 128 * (w + h + tw + th) + 4096
            > crate::MAX_FILE as u64
    {
        return Ok(None);
    }
    Ok(Some(target))
}

fn downscale(source: [i32; 2], bands: i32, r: &Request) -> Result<Option<Plan>, Error> {
    let o = &r.options;
    if bands != 4
        || r.format != Some(OutputFormat::Jpeg)
        || r.extra.preserve_hdr
        || r.extra.encoding.linear
        || r.extra.enforce_thumbnail
        || !matches!(o.mode, ops::ResizeMode::Fit | ops::ResizeMode::Force)
        || o.crop.is_some()
        || o.rotation != ops::Rotation::R0
        || o.flip_horizontal
        || o.flip_vertical
        || r.effects.trim.is_some()
    {
        return Ok(None);
    }
    let plan = Plan::new(source[0], source[1], r, false)?;
    if plan.scale.iter().any(|s| *s > 1.0) || plan.scale == [1.0; 2] {
        return Ok(None);
    }
    Ok(Some(plan))
}

pub(super) fn plan(source: [i32; 2], bands: i32, r: &Request) -> Result<Option<Plan>, Error> {
    let Some(plan) = downscale(source, bands, r)? else {
        return Ok(None);
    };
    Ok(dimensions(source, &plan)?.map(|_| plan))
}

pub(super) fn resize(
    pixels: Vec<u8>,
    source: [i32; 2],
    plan: &mut Plan,
) -> Result<([i32; 2], Vec<u8>), Error> {
    let target = dimensions(source, plan)?.ok_or("alpha resize scratch limit exceeded")?;
    if pixels.len() != source[0] as usize * source[1] as usize * 4 {
        return Err("invalid alpha resize raster");
    }
    let input = Image::from_vec_u8(source[0] as u32, source[1] as u32, pixels, PixelType::U8x4)
        .map_err(|_| "invalid alpha resize input")?;
    let len = target[0] as usize * target[1] as usize * 4;
    let mut output = Vec::new();
    output
        .try_reserve_exact(len)
        .map_err(|_| "alpha resize allocation failed")?;
    output.resize(len, 0);
    let mut output =
        Image::from_vec_u8(target[0] as u32, target[1] as u32, output, PixelType::U8x4)
            .map_err(|_| "invalid alpha resize output")?;
    Resizer::new()
        .resize(&input, &mut output, &ResizeOptions::new())
        .map_err(|_| "alpha Lanczos resize failed")?;
    plan.scale = [1.0; 2];
    Ok((target, output.into_vec()))
}

#[cfg(test)]
mod tests {
    use crate::request::{self, Config};

    fn request(options: &str) -> request::Request {
        request::parse(
            &format!("/unsafe/{options}/plain/local:///fixture.png@jpg"),
            &Config::unsafe_local(),
        )
        .unwrap()
    }

    #[test]
    fn premultiplied_resize_keeps_transparent_colors_out_of_visible_pixels() {
        let r = request("rs:fit:1:1");
        let mut plan = super::plan([2, 1], 4, &r).unwrap().unwrap();
        let (dimensions, pixels) =
            super::resize(vec![255, 0, 0, 128, 0, 0, 255, 0], [2, 1], &mut plan).unwrap();
        assert_eq!(dimensions, [1, 1]);
        assert!(pixels[0] >= 250 && pixels[1] == 0 && pixels[2] == 0);
        assert!((63..=65).contains(&pixels[3]));
        assert_eq!(plan.scale, [1.0; 2]);
    }

    #[test]
    fn identity_complex_geometry_precision_and_scratch_limits_keep_native_route() {
        for options in [
            "",
            "c:1:1:ce/rs:fit:1:1",
            "rot:90/rs:fit:1:1",
            "rs:fit:1:1/trim:10",
        ] {
            assert!(super::plan([2, 1], 4, &request(options)).unwrap().is_none());
        }
        let mut r = request("rs:fit:1:1");
        r.extra.preserve_hdr = true;
        assert!(super::plan([2, 1], 4, &r).unwrap().is_none());
        r.extra.preserve_hdr = false;
        r.extra.encoding.linear = true;
        assert!(super::plan([2, 1], 4, &r).unwrap().is_none());
        r.extra.encoding.linear = false;
        assert!(super::plan([2, 1], 3, &r).unwrap().is_none());
        assert!(super::plan([16000, 16000], 4, &r).unwrap().is_none());
    }

    #[test]
    fn opaque_constant_pixels_keep_exact_color_and_plan_dimensions() {
        let r = request("rs:fit:8:8/dpr:2");
        let mut plan = super::plan([32, 24], 4, &r).unwrap().unwrap();
        let dpr = plan.dpr;
        let (dimensions, pixels) =
            super::resize([88, 144, 200, 255].repeat(32 * 24), [32, 24], &mut plan).unwrap();
        assert_eq!(dimensions, [16, 12]);
        assert!(pixels.chunks_exact(4).all(|p| p == [88, 144, 200, 255]));
        assert_eq!(plan.dpr, dpr);
    }
}
