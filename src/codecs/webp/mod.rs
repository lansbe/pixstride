//! WebP lossless (VP8L) implementation using only Rust standard facilities.
//!
//! The encoder emits a RIFF/VP8L still image with per-tile prediction,
//! subtract-green, canonical codes, spatial groups and bounded LZ77 matching.
//! No native image library or external crate is used. Lossy VP8, animation,
//! ICC conversion and images over the explicit resource limits are unsupported.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
const MAX_SIDE: usize = 16_384;
pub(crate) struct BitWriter {
    bytes: Buffer,
    length: usize,
    pending: u64,
    used: u32,
    error: Option<Error>,
}
/// An encoding-side canonical prefix tree. Symbols above 255 are used by the
/// green/length alphabet, so they take the normal (not simple) description.
pub(crate) enum EncodeTree {
    Single(usize),
    SingleLong(usize),
    Pair(usize, usize),
    Full8,
    Dynamic { lengths: Vec<u8>, codes: Vec<u16> },
}
#[derive(Clone, Copy)]
pub(crate) enum EncodeToken {
    Literal(u32),
    Copy { length: usize, distance: usize },
}
pub(crate) struct EncodeCounts {
    green: [u32; 280],
    red: [u32; 256],
    blue: [u32; 256],
    alpha: [u32; 256],
    distance: [u32; 40],
}
pub(crate) struct EncodeGroup {
    green: EncodeTree,
    red: EncodeTree,
    blue: EncodeTree,
    alpha: EncodeTree,
    distance: EncodeTree,
}
// At most 16 frequency groups, assigned across 256x256 entropy tiles. The
// group map covers the whole image and contains no image data or saved result.
pub(crate) struct EncodeZones {
    tile_width: usize,
    tile_height: usize,
    columns: usize,
    rows: usize,
}
pub(crate) enum Residuals {
    Allocated(Vec<u32>),
    InPlace(Buffer),
}
pub(crate) struct Prepared {
    width: usize,
    height: usize,
    has_alpha: bool,
    modes: Vec<u8>,
    transformed: Residuals,
}
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    offset: usize,
}
pub(crate) struct Huffman {
    counts: [u16; 16],
    first: [u16; 16],
    offsets: [usize; 16],
    symbols: Vec<u16>,
    single: Option<u16>,
}
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
// Pixel offsets from the normative VP8L distance table, indexed by code - 1.
const PLANE: [(i8, i8); 120] = [
    (0, 1),
    (1, 0),
    (1, 1),
    (-1, 1),
    (0, 2),
    (2, 0),
    (1, 2),
    (-1, 2),
    (2, 1),
    (-2, 1),
    (2, 2),
    (-2, 2),
    (0, 3),
    (3, 0),
    (1, 3),
    (-1, 3),
    (3, 1),
    (-3, 1),
    (2, 3),
    (-2, 3),
    (3, 2),
    (-3, 2),
    (0, 4),
    (4, 0),
    (1, 4),
    (-1, 4),
    (4, 1),
    (-4, 1),
    (3, 3),
    (-3, 3),
    (2, 4),
    (-2, 4),
    (4, 2),
    (-4, 2),
    (0, 5),
    (3, 4),
    (-3, 4),
    (4, 3),
    (-4, 3),
    (5, 0),
    (1, 5),
    (-1, 5),
    (5, 1),
    (-5, 1),
    (2, 5),
    (-2, 5),
    (5, 2),
    (-5, 2),
    (4, 4),
    (-4, 4),
    (3, 5),
    (-3, 5),
    (5, 3),
    (-5, 3),
    (0, 6),
    (6, 0),
    (1, 6),
    (-1, 6),
    (6, 1),
    (-6, 1),
    (2, 6),
    (-2, 6),
    (6, 2),
    (-6, 2),
    (4, 5),
    (-4, 5),
    (5, 4),
    (-5, 4),
    (3, 6),
    (-3, 6),
    (6, 3),
    (-6, 3),
    (0, 7),
    (7, 0),
    (1, 7),
    (-1, 7),
    (5, 5),
    (-5, 5),
    (7, 1),
    (-7, 1),
    (4, 6),
    (-4, 6),
    (6, 4),
    (-6, 4),
    (2, 7),
    (-2, 7),
    (7, 2),
    (-7, 2),
    (3, 7),
    (-3, 7),
    (7, 3),
    (-7, 3),
    (5, 6),
    (-5, 6),
    (6, 5),
    (-6, 5),
    (8, 0),
    (4, 7),
    (-4, 7),
    (7, 4),
    (-7, 4),
    (8, 1),
    (8, 2),
    (6, 6),
    (-6, 6),
    (8, 3),
    (5, 7),
    (-5, 7),
    (7, 5),
    (-7, 5),
    (8, 4),
    (6, 7),
    (-6, 7),
    (7, 6),
    (-7, 6),
    (8, 5),
    (7, 7),
    (-7, 7),
    (8, 6),
    (8, 7),
];
pub(crate) struct CodeGroup {
    green: Huffman,
    red: Huffman,
    blue: Huffman,
    alpha: Huffman,
    distance: Huffman,
}
pub(crate) enum Transform {
    Predictor {
        width: usize,
        bits: usize,
        tiles: Vec<u32>,
    },
    Color {
        width: usize,
        bits: usize,
        tiles: Vec<u32>,
    },
    SubtractGreen,
    Index {
        width: usize,
        bits: usize,
        palette: Vec<u32>,
    },
}
mod checked_pixels;
pub(crate) use checked_pixels::*;
mod encode_group;
pub(crate) use encode_group::*;
mod encode_prepared;
pub(crate) use encode_prepared::*;
mod decode_data;
pub(crate) use decode_data::*;
mod decode;
pub(crate) use decode::*;

pub(crate) type Payload<'a> = (&'a [u8], Option<(usize, usize)>);
