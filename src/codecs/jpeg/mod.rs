//! JPEG baseline and progressive decoding, including supported CMYK inputs.

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
#[path = "jpeg_cmyk.rs"]
pub(crate) mod cmyk;
#[path = "jpeg_progressive/mod.rs"]
pub(crate) mod progressive;
// T.81, figure A.6: mapping from zigzag scan order to matrix position.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
// Each row x contains C(u) * cos((2*x+1)*u*pi/16), where C(0)=1/sqrt(2).
// A separable IDCT avoids needing a cosine function or a math runtime.
const BASIS: [[f64; 8]; 8] = [
    [
        0.7071067811865476,
        0.9807852804032304,
        0.9238795325112867,
        0.8314696123025452,
        0.7071067811865476,
        0.5555702330196023,
        0.3826834323650898,
        0.1950903220161283,
    ],
    [
        0.7071067811865476,
        0.8314696123025452,
        0.3826834323650898,
        -0.1950903220161282,
        -0.7071067811865475,
        -0.9807852804032304,
        -0.9238795325112868,
        -0.5555702330196022,
    ],
    [
        0.7071067811865476,
        0.5555702330196023,
        -0.3826834323650897,
        -0.9807852804032304,
        -0.7071067811865477,
        0.1950903220161283,
        0.9238795325112865,
        0.8314696123025455,
    ],
    [
        0.7071067811865476,
        0.1950903220161283,
        -0.9238795325112867,
        -0.5555702330196022,
        0.7071067811865474,
        0.8314696123025455,
        -0.3826834323650899,
        -0.9807852804032304,
    ],
    [
        0.7071067811865476,
        -0.1950903220161282,
        -0.9238795325112868,
        0.5555702330196018,
        0.7071067811865477,
        -0.8314696123025451,
        -0.3826834323650906,
        0.9807852804032303,
    ],
    [
        0.7071067811865476,
        -0.555570233019602,
        -0.3826834323650903,
        0.9807852804032304,
        -0.7071067811865467,
        -0.195090322016128,
        0.9238795325112867,
        -0.8314696123025451,
    ],
    [
        0.7071067811865476,
        -0.8314696123025453,
        0.38268343236509,
        0.1950903220161288,
        -0.7071067811865471,
        0.9807852804032304,
        -0.9238795325112864,
        0.5555702330196015,
    ],
    [
        0.7071067811865476,
        -0.9807852804032304,
        0.9238795325112865,
        -0.8314696123025451,
        0.7071067811865466,
        -0.5555702330196022,
        0.3826834323650904,
        -0.1950903220161286,
    ],
];
#[derive(Clone, Copy)]
pub(crate) struct Huffman {
    counts: [u8; 16],
    values: [u8; 256],
    // Packed symbol, code length, total bits and signed amplitude. DC uses
    // the first 256 entries for eight-bit prefixes; AC uses all 1,024 entries
    // for ten-bit prefixes. Longer codes use the checked fallback.
    short: [u32; 1024],
    len: usize,
    present: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: usize,
    dc: usize,
    ac: usize,
    predictor: i32,
}
pub(crate) struct Entropy<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u32,
    remaining: u8,
}
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
const BASIS_PAIRS: [[[f64; 2]; 8]; 4] = basis_pairs();
// For reduced decoding, integrate the existing full-resolution IDCT basis
// over disjoint 2- or 4-sample box windows. This avoids point-sampling aliasing
// and computes the average *before* per-sample clipping and rounding. It is a
// deliberately different quality/speed contract, not bit-equivalent to the
// full-resolution decoder, libjpeg's scaled IDCT, or Lanczos3.
pub(crate) struct ReducedBasis<const N: usize> {
    values: [[f64; 8]; N],
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    pairs: [[[f64; 2]; 8]; 2],
}
const REDUCED_2: ReducedBasis<4> = reduced_basis::<4, 2>();
const REDUCED_4: ReducedBasis<2> = reduced_basis::<2, 4>();
pub(crate) struct ParsedJpeg<'a> {
    data: &'a [u8],
    pos: usize,
    width: usize,
    height: usize,
    count: usize,
    max_h: usize,
    max_v: usize,
    quant: [[u16; 64]; 4],
    quant_present: [bool; 4],
    dc: [Huffman; 4],
    ac: [Huffman; 4],
    components: [Component; 3],
    order: [usize; 3],
    restart_interval: usize,
}
mod huffman;
pub(crate) use huffman::*;
mod reduced_basis;
pub(crate) use reduced_basis::*;
mod rgb_sixteen_values;
pub(crate) use rgb_sixteen_values::*;
mod decode_scan;
pub(crate) use decode_scan::*;
mod parsed_jpeg;
pub(crate) use parsed_jpeg::*;
#[cfg(test)]
mod tests;
