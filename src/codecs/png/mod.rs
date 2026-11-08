//! PNG decoding, checksums, scanline reconstruction and bounded row streaming.

#[cfg(test)]
mod security_regressions;

use crate::{Buffer, Error, Image, MAX_FILE, MAX_PIXELS};
const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_DIMENSION: usize = 100_000;
#[cfg(not(all(target_arch = "aarch64", target_feature = "crc")))]
const CRC_TABLE: [u32; 256] = make_crc_table();
pub(crate) struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u64,
    available: u8,
}
/// Bit access shared by contiguous input and the bounded IDAT file reader.
/// Both implementations expose the number of payload bytes logically consumed,
/// not any speculative lookahead in their underlying storage.
pub(crate) trait DeflateBits {
    fn read(&mut self, n: u8) -> Result<u32, Error>;
    fn peek_nine(&mut self) -> Result<Option<usize>, Error>;
    fn consume_huffman(&mut self, n: u8) -> Result<(), Error>;
    fn align_byte(&mut self);
    fn remaining(&self) -> usize;
    fn stored_header(&mut self) -> Result<(u16, u16), Error>;
    fn copy_stored<T: InflateOutput>(&mut self, output: &mut T, len: usize) -> Result<(), Error>;
}
/// Canonical Huffman tree, stored as counts and symbol ordering. DEFLATE
/// transmits each Huffman code most-significant bit first, unlike numeric fields.
pub(crate) struct Huffman {
    counts: [u16; 16],
    first: [u16; 16],
    offset: [u16; 16],
    symbols: [u16; 288],
    fast: [u16; 512],
}
const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
/// DEFLATE validation and symbol decoding are shared by the complete-buffer
/// and bounded-history destinations. Both destinations receive the exact
/// filtered bytes in stream order.
pub(crate) trait InflateOutput {
    fn capacity(&self) -> usize;
    fn literal(&mut self, byte: u8);
    fn stored(&mut self, bytes: &[u8]);
    fn backref(&mut self, distance: usize, length: usize);
    fn checksum(&mut self) -> u32;
    fn row_error(&self) -> Option<Error> {
        None
    }
}
pub(crate) struct WholeInflate<'a> {
    output: &'a mut [u8],
    used: usize,
}
/// Count the exact output size before allocating an ICC profile. The first
/// pass needs no history bytes: inflate_payload already checks match distances,
/// while the second pass reconstructs bytes and verifies the Adler checksum.
pub(crate) struct InflateSizeProbe {
    used: usize,
    capacity: usize,
}
const DEFLATE_WINDOW: usize = 32 * 1024;
const DEFLATE_MASK: usize = DEFLATE_WINDOW - 1;
/// Assembles filtered PNG rows and reconstructs them separately from the LZ77
/// history. Adler is calculated on the filtered bytes, before reconstruction.
pub(crate) struct StreamRows<'a, F: FnMut(usize, &[u8]), const VALIDATE_ONLY: bool> {
    filtered: Buffer,
    previous: Buffer,
    row_size: usize,
    filled: usize,
    row: usize,
    height: usize,
    adler_a: u32,
    adler_b: u32,
    invalid_filter: bool,
    on_row: &'a mut F,
}
/// A bounded DEFLATE destination. The ring stores filtered bytes exactly as
/// produced by DEFLATE, never reconstructed pixels. All produced bytes enter
/// the history immediately. The row consumer receives a complete 32 KiB
/// segment before the next segment can overwrite it, then the final remainder.
pub(crate) struct RingInflate<'a, F: FnMut(usize, &[u8]), const VALIDATE_ONLY: bool> {
    history: Buffer,
    rows: StreamRows<'a, F, VALIDATE_ONLY>,
    pos: usize,
    expected: usize,
}
/// The full, still-filtered DEFLATE output. Keeping its original bytes until
/// inflate and Adler validation finish is essential for LZ77 backreferences.
pub(crate) struct DecodedPng {
    width: usize,
    height: usize,
    color: u8,
    channels: usize,
    stride: usize,
    pixel_size: usize,
    raw: Buffer,
    palette: [u8; 768],
    pal_alpha: [u8; 256],
    palette_count: usize,
    transparent_gray: Option<u8>,
    transparent_rgb: Option<[u8; 3]>,
}
pub(crate) struct ParsedPng<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    color: u8,
    channels: usize,
    stride: usize,
    raw_size: usize,
    pixel_size: usize,
    idat_size: usize,
    single_idat: Option<&'a [u8]>,
    palette: [u8; 768],
    pal_alpha: [u8; 256],
    palette_count: usize,
    transparent_gray: Option<u8>,
    transparent_rgb: Option<[u8; 3]>,
}
pub(crate) enum Compressed<'a> {
    Borrowed(&'a [u8]),
    Owned(Buffer),
}
mod make_crc_table;
pub(crate) use make_crc_table::*;
mod inflate_icc_bounded;
pub(crate) use inflate_icc_bounded::*;
mod stream_rows;
pub(crate) use stream_rows::*;
mod parsed_png;
pub(crate) use parsed_png::*;
mod decode;
pub(crate) use decode::*;
