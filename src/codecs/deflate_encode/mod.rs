//! A small zlib/DEFLATE encoder written for the  PNG writer.
//!
//! The encoder performs a bounded LZ77 search. It compares fixed-Huffman and,
//! when its bounded token log fits, dynamic-Huffman blocks. If the fixed block
//! does not beat stored blocks, it may fall back to stored DEFLATE.
//! This module uses no external codec or crate. All sizes are bounded.

use crate::{Error, MAX_FILE};
const WINDOW: usize = 32_768;
const HASH_SIZE: usize = 65_536;
const EMPTY: u32 = u32::MAX;
const MAX_CHAIN: usize = 32;
// Optional dynamic coding must never make token storage scale without bound.
const MAX_TOKENS: usize = 2_000_000;
const MAX_ZLIB: usize = MAX_FILE - 57;
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
pub(crate) struct Bits {
    bytes: Vec<u8>,
    bits: u64,
    used: u8,
}
const LENGTH_CODES: [u8; 259] = length_codes();
mod bits;
pub(crate) use bits::*;
mod dynamic;
pub(crate) use dynamic::*;
