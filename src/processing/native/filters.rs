//! Bounded native filters with integer pixel blocks and mirrored partial edges.
use super::engine::checked;
use crate::{request::Request, Error};
use libvips::{
    enums::Extend,
    voption::{Setter, VOption},
    VipsImage,
};

pub(super) fn effects(mut image: VipsImage, r: &Request) -> Result<VipsImage, Error> {
    if r.effects.blur_sigma > 0.0 {
        image = image
            .gaussblur(r.effects.blur_sigma)
            .map_err(|_| "native blur failed")?;
    }
    if r.effects.sharpen_sigma > 0.0 {
        image = image
            .sharpen_with_opts(VOption::new().set("sigma", r.effects.sharpen_sigma))
            .map_err(|_| "native sharpen failed")?;
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
    Ok(image)
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
