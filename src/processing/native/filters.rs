//! Bounded native filters with integer pixel blocks and mirrored partial edges.
use super::engine::checked;
use crate::{request::Request, Error};
use libvips::{
    enums::{BandFormat, Extend, Interpretation},
    voption::{Setter, VOption},
    VipsImage,
};

fn sharpen(image: VipsImage, sigma: f64) -> Result<VipsImage, Error> {
    let options = VOption::new().set("sigma", sigma);
    if image.get_bands() == 4
        && image.get_format().ok() == Some(BandFormat::Uchar)
        && image.get_interpretation().ok() == Some(Interpretation::Srgb)
        && i64::from(image.get_width()) * i64::from(image.get_height()) * 6
            <= crate::MAX_FILE as i64
    {
        // The sharpener acts only on LAB lightness. Keep alpha out of the
        // colour-space conversions, and compute LAB once for its three readers.
        let source = super::shared::materialize(image)?;
        let alpha = source
            .extract_band(3)
            .map_err(|_| "native sharpen alpha extraction failed")?;
        let lab = source
            .extract_band_with_opts(0, VOption::new().set("n", 3))
            .and_then(|i| i.colourspace(Interpretation::Labs))
            .and_then(|i| i.copy_memory())
            .map_err(|_| "native sharpen colour preparation failed")?;
        let color = lab
            .sharpen_with_opts(options)
            .and_then(|i| i.colourspace(Interpretation::Srgb))
            .map_err(|_| "native sharpen failed")?;
        return VipsImage::bandjoin(&[color, alpha])
            .map_err(|_| "native sharpen alpha restoration failed");
    }
    image
        .sharpen_with_opts(options)
        .map_err(|_| "native sharpen failed")
}

pub(super) fn effects(mut image: VipsImage, r: &Request) -> Result<VipsImage, Error> {
    let neighborhood_filter = r.effects.blur_sigma > 0.0 || r.effects.sharpen_sigma > 0.0;
    let filtered = neighborhood_filter
        || (r.effects.pixelate > 1 && image.get_width().max(image.get_height()) > 1);
    let mut restore = None;
    if neighborhood_filter && image.hasalpha() {
        let format = image
            .get_format()
            .map_err(|_| "invalid native filter type")?;
        let space = image
            .get_interpretation()
            .map_err(|_| "invalid native filter colour space")?;
        image = image
            .premultiply()
            .and_then(|i| i.cast(format))
            .map_err(|_| "native filter premultiplication failed")?;
        restore = Some((space, format));
    }
    if filtered && super::shared::fits_memory(&image) {
        // Finish the upstream geometry once; filter halos otherwise reevaluate
        // it while caches are disabled. HDR/large rasters keep their old route.
        image = super::shared::materialize(image)?;
    }
    if r.effects.blur_sigma > 0.0 {
        image = image
            .gaussblur(r.effects.blur_sigma)
            .map_err(|_| "native blur failed")?;
    }
    if r.effects.sharpen_sigma > 0.0 {
        image = sharpen(image, r.effects.sharpen_sigma)?;
    }
    let (w, h) = (image.get_width(), image.get_height());
    let block = (r.effects.pixelate as i32).min(w.max(h));
    if block > 1 {
        let (tw, th) = (
            ((w + block - 1) / block) * block,
            ((h + block - 1) / block) * block,
        );
        checked(tw, th)?;
        if tw != w || th != h {
            image = image
                .embed_with_opts(
                    0,
                    0,
                    tw,
                    th,
                    VOption::new().set("extend", Extend::Mirror as i32),
                )
                .map_err(|_| "native pixelation extension failed")?;
        }
        image = image
            .shrink(f64::from(block), f64::from(block))
            .and_then(|i| i.zoom(block, block))
            .map_err(|_| "native pixelation failed")?;
        if tw != w || th != h {
            image = image
                .extract_area(0, 0, w, h)
                .map_err(|_| "native pixelation crop failed")?;
        }
    }
    if let Some((space, format)) = restore {
        image = image
            .unpremultiply()
            .and_then(|i| i.colourspace(space))
            .and_then(|i| i.cast(format))
            .map_err(|_| "native filter alpha restoration failed")?;
    }
    Ok(image)
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
