use crate::{Buffer, Error, Image, MAX_PIXELS};
/// A validated view of the BMP raster. Its bytes remain owned by the caller.
/// An explicit V5 alpha mask distinguishes alpha from BI_RGB32 padding.
pub(crate) struct BmpView<'a> {
    width: usize,
    height: usize,
    top_down: bool,
    channels: usize,
    alpha: bool,
    stride: usize,
    source: &'a [u8],
}
pub(crate) struct BmpLayout {
    width: usize,
    height: usize,
    top_down: bool,
    channels: usize,
    alpha: bool,
    stride: usize,
    offset: usize,
    end: usize,
}
/// A normalized encoded image. Borrowing is restricted to fully validated
/// identity inputs normalized in place; the caller keeps its freshly read
/// source alive until output is written. The source file itself is unchanged.
/// No result is retained between requests.
pub enum Output<'a> {
    Owned(Buffer),
    Borrowed(&'a [u8]),
}
mod expand_row;
pub(crate) use expand_row::*;
mod transform;
pub(crate) use transform::*;

mod prefilter;
#[cfg(test)]
mod rgb_tests;
use prefilter::prehalve_parsed;
