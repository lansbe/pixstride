//! Progressive 8-bit Huffman JPEG, derived from ITU-T T.81 Annex G.
//!
//! Coefficients remain private until all scans, restart markers, quantization
//! tables and the final EOI have been checked. No external image codec is used.
use super::{
    inverse_dct, parse_dht, read_marker, read_u16, sample_plane, store_rgb, Component, Entropy,
    Huffman, ZIGZAG,
};
use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
const MAX_COMPONENTS: usize = 4;
const MAX_SCANS: usize = 512;
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    block_offset: usize,
    padded_blocks_w: usize,
    padded_blocks_h: usize,
    blocks_w: usize,
    blocks_h: usize,
    width: usize,
    height: usize,
    plane_offset: usize,
}
pub(crate) struct Frame {
    width: usize,
    height: usize,
    count: usize,
    max_h: usize,
    max_v: usize,
    mcu_cols: usize,
    mcu_rows: usize,
    components: [Component; MAX_COMPONENTS],
    layout: [Layout; MAX_COMPONENTS],
    // One approximation level per coefficient in zigzag order; -1 means
    // the coefficient has not yet received its first transmission.
    approximation: [[i8; 64]; MAX_COMPONENTS],
    component_quant: [[u16; 64]; MAX_COMPONENTS],
    has_quant: [bool; MAX_COMPONENTS],
    coeffs: Buffer,
    plane_bytes: usize,
}
pub(crate) struct Scan {
    count: usize,
    order: [usize; MAX_COMPONENTS],
    dc: [usize; MAX_COMPONENTS],
    ac: [usize; MAX_COMPONENTS],
    start: usize,
    end: usize,
    high: u8,
    low: u8,
}
mod parse_quantization;
pub(crate) use parse_quantization::*;
mod decode_block;
pub(crate) use decode_block::*;
