//! Preserve the native Lanczos sampling grid while decoding large RGBA PNGs a band at a time.
use super::Plan;
use crate::{request::Request, Error};
use fast_png::Reader;
use libvips::{
    enums::{BandFormat, Interpretation, Kernel},
    voption::{Setter, VOption},
    VipsImage,
};
use std::io::Cursor;

const STRIP_BYTES: usize = 4 * 1024 * 1024;

pub(in super::super) fn plan(
    source: [i32; 2],
    bands: i32,
    r: &Request,
) -> Result<Option<Plan>, Error> {
    let Some(plan) = super::downscale(source, bands, r)? else {
        return Ok(None);
    };
    // Small images already use FIR. Keep its sampling semantics and only
    // replace the full-frame native reduction when both axes shrink.
    if super::dimensions(source, &plan)?.is_some()
        || plan.scale.iter().any(|&s| !(0.125..1.0).contains(&s))
    {
        return Ok(None);
    }
    let target = target(source, &plan);
    let intermediate = i64::from(source[1]) * i64::from(target[0]);
    if intermediate > 16_000_000
        || i64::from(source[0]) * i64::from(source[1])
            + 2 * intermediate
            + i64::from(target[0]) * i64::from(target[1])
            > 100_000_000
    {
        return Ok(None);
    }
    Ok(Some(plan))
}

fn target(source: [i32; 2], plan: &Plan) -> [i32; 2] {
    std::array::from_fn(|axis| {
        (f64::from(source[axis]) * plan.scale[axis])
            .round_ties_even()
            .max(1.0) as i32
    })
}

// For uchar sRGB the native premultiply then cast truncates each product.
fn premultiply(row: &mut [u8]) {
    for p in row.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        p[0] = (u16::from(p[0]) * a / 255) as u8;
        p[1] = (u16::from(p[1]) * a / 255) as u8;
        p[2] = (u16::from(p[2]) * a / 255) as u8;
    }
}

pub(in super::super) fn resize(
    reader: &mut Reader<Cursor<&[u8]>>,
    source: [i32; 2],
    plan: &Plan,
) -> Result<([i32; 2], Vec<u8>), Error> {
    let [sw, sh] = source;
    let [tw, th] = target(source, plan);
    let stride = sw as usize * 4;
    // Lanczos3 covers three reduced samples on either side. Allow for the
    // optional integer box shrink and both libvips endpoint roundings.
    let halo = (5.0 / plan.scale[1]).ceil() as i32 + 4;
    let band = ((STRIP_BYTES / stride) as f64 * plan.scale[1]).clamp(16.0, 256.0) as i32;
    let mut output = Vec::new();
    output
        .try_reserve_exact(tw as usize * th as usize * 4)
        .map_err(|_| "alpha resize allocation failed")?;
    let mut overlap = Vec::new();
    let mut read = 0;
    for y in (0..th).step_by(band as usize) {
        let end = (y + band).min(th);
        let first = ((f64::from(y) / plan.scale[1]).floor() as i32 - halo).max(0);
        let last = ((f64::from(end) / plan.scale[1]).ceil() as i32 + halo).min(sh);
        let mut pixels = std::mem::take(&mut overlap);
        pixels
            .try_reserve_exact((last - read) as usize * stride)
            .map_err(|_| "PNG strip allocation failed")?;
        while read < last {
            let row = reader
                .next_row()
                .map_err(|_| "PNG row decode failed")?
                .ok_or("missing PNG row")?;
            if row.data().len() != stride {
                return Err("inconsistent PNG row width");
            }
            let start = pixels.len();
            pixels.extend_from_slice(row.data());
            premultiply(&mut pixels[start..]);
            read += 1;
        }
        if end < th {
            let next_first = ((f64::from(end) / plan.scale[1]).floor() as i32 - halo).max(0);
            overlap.extend_from_slice(&pixels[(next_first - first) as usize * stride..]);
        }
        // Embed lazily at the true source origin. Sampling coordinates, the
        // convolution phase and clamping at both source edges stay global.
        let image = VipsImage::new_from_owned_u8(pixels, sw, last - first, 4)
            .and_then(|i| {
                i.copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            })
            .and_then(|i| i.embed(0, first, sw, sh))
            .and_then(|i| {
                i.resize_with_opts(
                    plan.scale[0],
                    VOption::new()
                        .set("vscale", plan.scale[1])
                        .set("kernel", Kernel::Lanczos3 as i32),
                )
            })
            .and_then(|i| i.extract_area(0, y, tw, end - y))
            .and_then(|i| i.unpremultiply())
            .and_then(|i| i.cast(BandFormat::Uchar))
            .map_err(|_| "native PNG strip resize failed")?;
        output.extend_from_slice(&super::super::raster::export(&image)?);
        drop(image);
        libvips::Vips::thread_shutdown();
    }
    if read != sh
        || reader
            .next_row()
            .map_err(|_| "PNG row finalization failed")?
            .is_some()
    {
        return Err("inconsistent PNG row count");
    }
    Ok(([tw, th], output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fast_png::{BitDepth, ColorType, Decoder, Encoder};

    #[test]
    fn overlapping_png_bands_match_native_lanczos_on_noise_at_edges_and_seams() {
        super::super::super::engine::initialize().unwrap();
        let source = [513, 1777];
        let mut seed = 903u32;
        let mut pixels = vec![0; (source[0] * source[1] * 4) as usize];
        for byte in &mut pixels {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (seed >> 24) as u8;
        }
        let mut png = Vec::new();
        let mut encoder = Encoder::new(&mut png, source[0] as u32, source[1] as u32);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        let original = VipsImage::new_from_owned_u8(pixels, source[0], source[1], 4)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            .unwrap();
        for scale in [[0.71, 0.67], [0.28, 0.17]] {
            let plan = Plan {
                scale,
                target: [0, 0],
                dpr: 1.0,
            };
            let expected =
                super::super::super::geometry::resize_scale(&original, scale[0], scale[1], false)
                    .unwrap();
            let mut reader = Decoder::new(Cursor::new(png.as_slice()))
                .read_info()
                .unwrap();
            let (dimensions, actual) = resize(&mut reader, source, &plan).unwrap();
            assert_eq!(dimensions, [expected.get_width(), expected.get_height()]);
            let reference = super::super::super::raster::export(&expected).unwrap();
            assert_eq!(actual.len(), reference.len());
            assert_eq!(
                actual
                    .iter()
                    .zip(reference.iter())
                    .position(|(a, b)| a != b),
                None
            );
            reader.finish().unwrap();
        }
    }
}
