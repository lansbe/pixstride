//! Bounded, separable RGBA8 resampling. No standard library or image library.

use crate::{Buffer, Error, Image, MAX_PIXELS};
use core::mem::{align_of, size_of};
use core::slice;
const CHANNELS: usize = 4;
const STRIPE: usize = 512;
const ROW_CACHE_BYTES: usize = 16 * 1024 * 1024;
const WEIGHT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const PI: f64 = core::f64::consts::PI;
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Filter {
    Linear,
    Lanczos3,
}
/// One destination sample's bounded source interval, with its normalization.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Span {
    first: usize,
    count: usize,
    weight_offset: usize,
    center: f64,
    scale: f64,
    inv_sum: f64,
    opaque_alpha: f32,
    fast_stride: u8,
    fast_eight: u8,
    // Typed writes can leave implicit padding uninitialized. This storage is
    // also exposed as bytes by Buffer, so every byte must be a real field.
    _padding: [u8; 2],
}
const _: () = assert!(
    size_of::<Span>() == 3 * size_of::<usize>() + 3 * size_of::<f64>() + size_of::<f32>() + 4
);
const _: () = assert!(align_of::<Span>() <= 16);
/// Precompute one horizontal stripe's weights when they fit the fixed budget.
/// For unusually extreme reductions we retain only spans and calculate their
/// weights as needed, rather than making a large coefficient allocation.
pub(crate) struct Plan {
    spans: Buffer,
    weights: Option<Buffer>,
    filter: Filter,
    opaque_weights: bool,
}
pub(crate) enum FilterSource<'a> {
    Rgba(&'a [u8]),
    /// Reconstructed RGB rows with one leading PNG filter byte per row.
    OpaqueRgbRows {
        bytes: &'a [u8],
        stride: usize,
        expand: fn(&[u8], &mut [u8]),
    },
}
pub(crate) enum FilterOutput<'a> {
    Rgba(&'a mut [u8]),
    Bmp {
        bytes: &'a mut [u8],
        stride: usize,
        pack: fn(&[u8], &mut [u8]),
    },
}
const STREAM_MAX_STRIPES: usize = 100_000_usize.div_ceil(STRIPE);
/// Private output storage for sequential rows and arbitrary stripes within a row.
/// A row is published to the storage only after all its stripes are written.
pub(crate) trait BmpTarget {
    fn stripe(
        &mut self,
        stride: usize,
        height: usize,
        y: usize,
        first: usize,
        bytes: usize,
    ) -> &mut [u8];
    fn end_row(&mut self, stride: usize, height: usize, y: usize) -> Result<(), Error>;

    #[inline]
    fn write_rgba(
        &mut self,
        stride: usize,
        height: usize,
        y: usize,
        first: usize,
        pixels: &[u8],
        pack: fn(&[u8], &mut [u8]),
    ) {
        let bytes = pixels.len() / CHANNELS * 3;
        pack(pixels, self.stripe(stride, height, y, first, bytes));
    }
}
/// A private RGBA destination for the filtered-row stream. The nearest/identity
/// path is deliberately not supported: its BGR-only writes fail closed.
pub(crate) struct StreamRgbaTarget {
    width: usize,
    height: usize,
    pixels: Buffer,
    unexpected_bgr: bool,
}
/// Progressive RGB-to-BMP sink. The caller withholds the owned output until
/// the complete compressed stream, checksum and every PNG row are validated.
pub(crate) struct StreamRgbBmp<T: BmpTarget = Buffer> {
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    bmp_stride: usize,
    bmp: T,
    error: Option<Error>,
    next_source: usize,
    next_output: usize,
    filtered: Option<StreamFiltered>,
    expand: fn(&[u8], &mut [u8]),
    pack: fn(&[u8], &mut [u8]),
}
const MAX_ALPHA_PATTERNS: usize = 64;
pub(crate) struct StreamOpaqueAlpha {
    map: Buffer,
    values: [f32; MAX_ALPHA_PATTERNS],
    len: usize,
}
pub(crate) struct StreamFiltered {
    vertical: Plan,
    plans: [Option<Plan>; STREAM_MAX_STRIPES],
    stripes: usize,
    rgba_row: Option<Buffer>,
    planes: Option<Buffer>,
    opaque_alpha: Option<StreamOpaqueAlpha>,
    phase_stride: usize,
    horizontal_row: Option<Buffer>,
    rows: Buffer,
    slots: usize,
    accumulate: bool,
    started: usize,
}
mod typed_mut;
pub(crate) use typed_mut::*;
mod all_opaque_neon;
pub(crate) use all_opaque_neon::*;
mod horizontal_phase_planes_rgb;
pub(crate) use horizontal_phase_planes_rgb::*;
mod rgb_to_bgr;
pub(crate) use rgb_to_bgr::*;
mod stream_filtered;
mod stream_rgb_bmp;
pub(crate) use stream_rgb_bmp::*;
