//! Bounded EXIF orientation and ICC preservation for supported image formats.
//!
//! Orientation is the primary TIFF IFD orientation (tag 0x0112). JPEG APP1
//! contains `Exif\0\0` before TIFF; PNG eXIf contains TIFF directly. A TIFF
//! input is accepted for metadata inspection, independently of image decoding.
//! Linked IFD directories are bounded and checked for cycles, but EXIF sub-IFD
//! metadata is not traversed or retained. Missing orientation means 1. TIFF's
//! primary ICC tag and the selected embedded PNG of an ICO are also inspected.
//!
//! ICC is not converted. JPEG ICC_PROFILE APP2 segments and WebP ICCP are
//! assembled/read. PNG iCCP is decompressed with a bounded output, complete
//! DEFLATE and Adler checks. All accepted profiles need a structurally valid
//! RGB header and bounded tag table; profile semantics are not interpreted.
//! Color samples must remain in their original space when reattaching one.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const ICC_MAX: usize = 16 * 1024 * 1024;
const MAX_IFDS: usize = 128;
/// Owned raw RGB ICC profile. No color conversion is performed here.
pub enum IccProfile {
    RawRgb(Buffer),
}
pub struct Metadata {
    pub orientation: u8,
    pub icc: Option<IccProfile>,
}
mod read16;
pub(crate) use read16::*;
mod webp_metadata;
pub(crate) use webp_metadata::*;
mod icc_tag;
pub(crate) use icc_tag::*;
