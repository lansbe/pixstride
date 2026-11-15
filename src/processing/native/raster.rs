//! Own the exact GLib raster allocation without a second RGBA expansion/copy.
use crate::Error;
use libvips::{enums::BandFormat, VipsImage};
use std::{ops::Deref, ptr::NonNull};

pub(super) struct Memory {
    raw: NonNull<std::ffi::c_void>,
    len: usize,
}
impl Deref for Memory {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        // SAFETY: export validated the exact returned size; this object owns
        // the initialized allocation, with no mutable aliases until Drop.
        unsafe { std::slice::from_raw_parts(self.raw.as_ptr().cast(), self.len) }
    }
}
impl AsRef<[u8]> for Memory {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl Drop for Memory {
    fn drop(&mut self) {
        // SAFETY: uniquely owned memory returned by vips_image_write_to_memory.
        unsafe { libvips::bindings::g_free(self.raw.as_ptr()) };
    }
}

pub(super) fn export(image: &VipsImage) -> Result<Memory, Error> {
    super::engine::checked(image.get_width(), image.get_height())?;
    if image.get_format().ok() != Some(BandFormat::Uchar) || !matches!(image.get_bands(), 1..=4) {
        return Err("unsupported native raster type");
    }
    let expected = (image.get_width() as usize)
        .checked_mul(image.get_height() as usize)
        .and_then(|n| n.checked_mul(image.get_bands() as usize))
        .ok_or("native raster overflow")?;
    let mut len = 0;
    // SAFETY: image is live; libvips sets len and returns an independent owned
    // allocation, or NULL. Reject NULL and inconsistent size before any read.
    let raw = NonNull::new(unsafe {
        libvips::bindings::vips_image_write_to_memory(image.as_mut_ptr(), &mut len)
    })
    .ok_or("native raster export failed")?;
    let memory = Memory { raw, len };
    if len != expected {
        return Err("inconsistent native raster size");
    }
    Ok(memory)
}

pub(super) struct Detached {
    pub width: usize,
    pub height: usize,
    pub bands: usize,
    pub memory: Memory,
}

/// Materialize every output pixel, then release the graph before Rust encoding.
pub(super) fn detach(image: VipsImage) -> Result<Detached, Error> {
    let raster = Detached {
        width: image.get_width() as usize,
        height: image.get_height() as usize,
        bands: image.get_bands() as usize,
        memory: export(&image)?,
    };
    drop(image);
    // This worker thread owns the graph. Private regions may retain source
    // images after its public references drop; the exported memory is separate.
    libvips::Vips::thread_shutdown();
    Ok(raster)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detached_pixels_release_the_source_graph_before_compression() {
        use libvips::VipsSource;
        use std::sync::Arc;
        super::super::engine::initialize().unwrap();
        let image = VipsImage::new_from_owned_u8(vec![1, 2, 3, 4, 5, 6], 2, 1, 3).unwrap();
        let bytes = Arc::new(image.write_to_buffer(".png").unwrap());
        drop(image);
        let weak = Arc::downgrade(&bytes);
        let source = VipsSource::new_from_shared_memory(bytes).unwrap();
        let image = VipsImage::new_from_source(&source, "").unwrap();
        drop(source);
        let detached = detach(image).unwrap();
        assert!(weak.upgrade().is_none(), "encoded source still retained");
        assert_eq!((detached.width, detached.height, detached.bands), (2, 1, 3));
        assert_eq!(&*detached.memory, &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn owned_export_preserves_bands_and_rejects_non_byte_samples() {
        super::super::engine::initialize().unwrap();
        let image =
            VipsImage::new_from_memory_copy(&[1, 2, 3, 4, 5, 6], 2, 1, 3, BandFormat::Uchar)
                .unwrap();
        let memory = export(&image).unwrap();
        drop(image);
        assert_eq!(&*memory, &[1, 2, 3, 4, 5, 6]);
        let image =
            VipsImage::new_from_memory_copy(&[1, 2, 3, 4, 5, 6], 2, 1, 3, BandFormat::Uchar)
                .unwrap();
        assert!(export(&image.cast(BandFormat::Float).unwrap()).is_err());
    }
}
