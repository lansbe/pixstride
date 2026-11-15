//! Render bounded static SVG directly at its target size inside the isolated worker.
//! Font/raster resources and complex options retain the full native implementation.
use super::{engine::NativeResult, scale::Plan};
use crate::{
    ops,
    request::{OutputFormat, Request},
    Buffer, Error, Image,
};
use resvg::{tiny_skia, usvg};

fn eligible(r: &Request) -> bool {
    let o = &r.options;
    let e = &r.extra;
    r.format == Some(OutputFormat::Png)
        && r.strip_metadata
        && r.strip_color_profile
        && !e.keep_copyright
        && !e.preserve_hdr
        && !e.raw
        && !e.skip.contains(&OutputFormat::Svg)
        && e.force_formats.is_empty()
        && !e.encoding.sanitize_svg
        && !e.encoding.png_interlaced
        && !e.encoding.png_quantize
        && !e.encoding.linear
        && o.crop.is_none()
        && o.rotation == ops::Rotation::R0
        && !o.flip_horizontal
        && !o.flip_vertical
        && o.padding == Default::default()
        && o.background.is_none()
        && o.watermark.is_none()
        && r.watermark.is_none()
        && matches!(o.mode, ops::ResizeMode::Fit | ops::ResizeMode::Force)
        && !e.extend
        && !e.extend_aspect
        && !e.fill_down
        && !e.smart
        && e.min == [0; 2]
        && e.offsets == [0.0; 2]
        && r.effects.blur_sigma == 0.0
        && r.effects.sharpen_sigma == 0.0
        && r.effects.pixelate <= 1
        && r.effects.trim.is_none()
}

fn side(value: Option<&str>) -> Option<u32> {
    let value = value?;
    value
        .strip_suffix("px")
        .unwrap_or(value)
        .parse()
        .ok()
        .filter(|n| (1..=100_000).contains(n))
}

pub(super) fn try_process(data: &[u8], r: &Request) -> Result<Option<NativeResult>, Error> {
    if !eligible(r) {
        return Ok(None);
    }
    if data.len() > 10 * 1024 * 1024 {
        return Err("SVG source exceeds limit");
    }
    let text = std::str::from_utf8(data).map_err(|_| "SVG must be UTF-8")?;
    let doc = roxmltree::Document::parse(text).map_err(|_| "invalid SVG XML")?;
    for node in doc.descendants().filter(|n| n.is_element()) {
        if matches!(
            node.tag_name().name(),
            "text" | "tspan" | "textPath" | "image"
        ) || node
            .attributes()
            .any(|a| matches!(a.name(), "href" | "src") && !a.value().starts_with('#'))
        {
            return Ok(None);
        }
    }
    let root = doc.root_element();
    let (Some(width), Some(height)) = (
        side(root.attribute("width")),
        side(root.attribute("height")),
    ) else {
        return Ok(None);
    };
    // Admission is shared with the native renderer, including CSS/active/DTD,
    // structural and reference restrictions. No raster-data resolver is reached
    // because images/non-fragment resource references fell back above.
    super::svg::validate(data)?;
    let source_pixels = width as usize * height as usize;
    crate::image::dimensions(width as usize, height as usize)?;
    if source_pixels > r.extra.max_source_pixels || source_pixels > r.extra.max_frame_pixels {
        return Err("SVG source pixel budget exceeded");
    }
    let options = usvg::Options {
        dpi: 72.0,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(data, &options).map_err(|_| "SVG render parse failed")?;
    let size = tree.size().to_int_size();
    if size.width() != width || size.height() != height {
        return Ok(None);
    }
    let plan = Plan::new(width as i32, height as i32, r, true)?;
    let mut dimensions = [
        (width as f64 * plan.scale[0]).round_ties_even().max(1.0) as usize,
        (height as f64 * plan.scale[1]).round_ties_even().max(1.0) as usize,
    ];
    for (dimension, target) in dimensions.iter_mut().zip(plan.target) {
        if target > 0 {
            *dimension = (*dimension).min(target as usize);
        }
    }
    crate::image::dimensions(dimensions[0], dimensions[1])?;
    let len = dimensions[0]
        .checked_mul(dimensions[1])
        .and_then(|n| n.checked_mul(4))
        .ok_or("SVG raster exceeds limit")?;
    if len > crate::MAX_FILE {
        return Ok(None);
    }
    let mut pixmap = tiny_skia::Pixmap::new(dimensions[0] as u32, dimensions[1] as u32)
        .ok_or("SVG raster allocation failed")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(plan.scale[0] as f32, plan.scale[1] as f32),
        &mut pixmap.as_mut(),
    );
    let mut pixels = Buffer::new(len)?;
    pixels.as_mut_slice().copy_from_slice(pixmap.data());
    drop(pixmap);
    for pixel in pixels.as_mut_slice().chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha != 255 {
            for value in &mut pixel[..3] {
                *value = ((u16::from(*value) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let image = Image {
        width: dimensions[0],
        height: dimensions[1],
        pixels,
    };
    Ok(Some(NativeResult {
        body: super::encode::optimized_png(&image)?,
        dimensions,
        origin: [width as usize, height as usize],
        format: OutputFormat::Png,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{self, Config};
    #[test]
    fn renders_at_target_size_with_straight_alpha_and_original_dimensions() {
        let r = request::parse(
            "/unsafe/rs:fit:2:1/plain/local:///fixture.svg@png",
            &Config::unsafe_local(),
        )
        .unwrap();
        let source = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="2" height="2" fill="red" opacity="0.5"/></svg>"#;
        let result = try_process(source, &r).unwrap().unwrap();
        assert_eq!(result.dimensions, [2, 1]);
        assert_eq!(result.origin, [4, 2]);
        let decoded = crate::png::decode(&result.body).unwrap();
        assert_eq!(decoded.pixels.as_slice(), &[255, 0, 0, 128, 0, 0, 0, 0]);
    }
    #[test]
    fn unsupported_text_images_and_metadata_fall_back_without_silently_losing_content() {
        let r = request::parse(
            "/unsafe/plain/local:///fixture.svg@png",
            &Config::unsafe_local(),
        )
        .unwrap();
        for source in [
            br#"<svg width="4" height="2"><text>x</text></svg>"#.as_slice(),
            br#"<svg width="4" height="2"><image href="file:///private"/></svg>"#.as_slice(),
        ] {
            assert!(try_process(source, &r).unwrap().is_none());
        }
        let huge = br#"<svg width="100000" height="100000"><rect width="1" height="1"/></svg>"#;
        assert!(try_process(huge, &r).is_err());
    }
}
