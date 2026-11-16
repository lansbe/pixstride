//! Shared immutable source bytes stay alive through native image graph lifetimes.
use crate::Error;
use libvips::{voption::VOption, VipsImage, VipsSource};
use std::sync::Arc;

pub(super) fn load(bytes: &Arc<Vec<u8>>, options: VOption<'_>) -> Result<VipsImage, Error> {
    let source = VipsSource::new_from_shared_memory(Arc::clone(bytes))
        .map_err(|_| "native shared source unavailable")?;
    VipsImage::new_from_source_with_opts(&source, "", options)
        .map_err(|_| "native source decode failed")
}

pub(super) fn fits_memory(image: &VipsImage) -> bool {
    image.get_format().ok() == Some(libvips::enums::BandFormat::Uchar)
        && matches!(image.get_bands(), 1..=4)
        && image.get_width() > 0
        && image.get_height() > 0
        && i64::from(image.get_width())
            .checked_mul(i64::from(image.get_height()))
            .and_then(|n| n.checked_mul(i64::from(image.get_bands())))
            .is_some_and(|n| n <= crate::MAX_FILE as i64)
}

/// Own a bounded raster independently of decoder/filter regions before reuse.
pub(super) fn materialize(image: VipsImage) -> Result<VipsImage, Error> {
    if !fits_memory(&image) {
        return Err("native materialization exceeds raster budget");
    }
    let owned = image
        .copy_memory()
        .map_err(|_| "native raster materialization failed")?;
    drop(image);
    libvips::Vips::thread_shutdown();
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use libvips::{
        voption::{Setter, VOption},
        Vips, VipsImage, VipsSource,
    };
    use std::sync::Arc;

    #[test]
    fn owned_pixels_outlive_the_original_image_and_derived_graph() {
        super::super::engine::initialize().unwrap();
        let image = VipsImage::new_from_owned_u8(vec![255, 0, 0, 128, 2, 3, 4, 0], 2, 1, 4)
            .unwrap()
            .copy_with_opts(VOption::new().set(
                "interpretation",
                libvips::enums::Interpretation::Srgb as i32,
            ))
            .unwrap();
        let derived = image.copy().unwrap();
        drop(image);
        let exported = super::super::raster::export(&derived).unwrap();
        assert_eq!(&*exported, &[255, 0, 0, 128, 2, 3, 4, 0]);
        drop(derived);
        Vips::thread_shutdown();
        assert_eq!(&*exported, &[255, 0, 0, 128, 2, 3, 4, 0]);
        assert!(VipsImage::new_from_owned_u8(vec![0; 5], 2, 1, 3).is_err());
        assert!(VipsImage::new_from_owned_u8(vec![], 0, 1, 3).is_err());
    }

    #[test]
    fn shared_source_survives_source_wrapper_drop_until_lazy_decode_finishes() {
        super::super::engine::initialize().unwrap();
        let encoded = crate::png_encode::encode(&crate::Image {
            width: 2,
            height: 1,
            pixels: {
                let mut pixels = crate::Buffer::new(8).unwrap();
                pixels
                    .as_mut_slice()
                    .copy_from_slice(&[255, 0, 0, 128, 2, 3, 4, 0]);
                pixels
            },
        })
        .unwrap();
        let bytes = Arc::new(encoded.as_slice().to_vec());
        let weak = Arc::downgrade(&bytes);
        let source = VipsSource::new_from_shared_memory(Arc::clone(&bytes)).unwrap();
        let alias = source.clone();
        drop(bytes);
        drop(source);
        let alive = weak.upgrade().is_some();
        if !alive {
            // Do not invoke C with a stale pointer when testing a broken clone.
            std::mem::forget(alias);
            panic!("Cloned source did not retain its C reference and input owner");
        }
        let image = VipsImage::new_from_source(&alias, "").unwrap();
        drop(alias);
        assert!(weak.upgrade().is_some());
        let exported = super::super::raster::export(&image).unwrap();
        assert_eq!(&*exported, &[255, 0, 0, 128, 2, 3, 4, 0]);
        drop(image);
        Vips::thread_shutdown();
        assert!(
            weak.upgrade().is_none(),
            "Native source retained an encoded input allocation after release"
        );
    }
}
