use super::decode_block::{decode_with_limits, scan_geometry};
use super::*;

fn segment(bytes: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    bytes.extend_from_slice(&[0xff, marker]);
    bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
}

fn huffman_table(tag: u8) -> [u8; 18] {
    let mut table = [0; 18];
    table[0] = tag;
    table[1] = 1;
    table
}

// Two one-block scans with a redefinition and another table between them.
fn progressive_image() -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8];
    let mut quant = [1; 65];
    quant[0] = 0;
    segment(&mut bytes, 0xdb, &quant);
    segment(&mut bytes, 0xc2, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    segment(&mut bytes, 0xc4, &huffman_table(0));
    segment(&mut bytes, 0xda, &[1, 1, 0, 0, 0, 0]);
    bytes.push(0x7f);
    let mut two_tables = Vec::new();
    two_tables.extend_from_slice(&huffman_table(0));
    two_tables.extend_from_slice(&huffman_table(0x10));
    segment(&mut bytes, 0xc4, &two_tables);
    segment(&mut bytes, 0xda, &[1, 1, 0, 1, 63, 0]);
    bytes.extend_from_slice(&[0x7f, 0xff, 0xd9]);
    bytes
}

#[test]
fn cumulative_scan_and_huffman_limits_allow_the_exact_boundary() {
    let bytes = progressive_image();
    let image = decode_with_limits(&bytes, 2, 3).unwrap();
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(image.pixels.as_slice(), &[128, 128, 128, 255]);
    let image = decode(&bytes).unwrap();
    assert_eq!(image.pixels.as_slice(), &[128, 128, 128, 255]);
}

#[test]
fn cumulative_scan_and_huffman_limits_reject_the_next_unit() {
    let bytes = progressive_image();
    assert!(matches!(
        decode_with_limits(&bytes, 1, 3),
        Err("JPEG progressive: scan work exceeds limit")
    ));
    assert!(matches!(
        decode_with_limits(&bytes, 2, 2),
        Err("JPEG: Huffman table budget exceeded")
    ));
    assert!(matches!(
        decode_with_limits(&bytes, 0, 3),
        Err("JPEG progressive: scan work exceeds limit")
    ));
}

#[test]
fn interleaved_and_noninterleaved_scans_share_the_budget() {
    let mut bytes = vec![0xff, 0xd8];
    let mut quant = [1; 65];
    quant[0] = 0;
    segment(&mut bytes, 0xdb, &quant);
    segment(
        &mut bytes,
        0xc2,
        &[8, 0, 9, 0, 17, 3, 1, 0x22, 0, 2, 0x11, 0, 3, 0x11, 0],
    );
    segment(&mut bytes, 0xc4, &huffman_table(0));
    segment(&mut bytes, 0xc4, &huffman_table(0x10));
    segment(&mut bytes, 0xda, &[3, 1, 0, 2, 0, 3, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0x0f]);
    segment(&mut bytes, 0xda, &[1, 1, 0, 1, 63, 0]);
    bytes.push(0x03);
    segment(&mut bytes, 0xda, &[1, 2, 0, 1, 63, 0]);
    bytes.push(0x3f);
    segment(&mut bytes, 0xda, &[1, 3, 0, 1, 63, 0]);
    bytes.extend_from_slice(&[0x3f, 0xff, 0xd9]);

    let image = decode_with_limits(&bytes, 22, 2).unwrap();
    assert_eq!((image.width, image.height), (17, 9));
    assert!(image
        .pixels
        .as_slice()
        .chunks_exact(4)
        .all(|pixel| pixel == [128, 128, 128, 255]));
    assert!(matches!(
        decode_with_limits(&bytes, 21, 2),
        Err("JPEG progressive: scan work exceeds limit")
    ));
}

#[test]
fn dc_refinement_preserves_half_sample_rounding() {
    for (first_scan, expected) in [(0x5f, 129), (0x3f, 128)] {
        let mut bytes = vec![0xff, 0xd8];
        let mut quant = [1; 65];
        quant[0] = 0;
        segment(&mut bytes, 0xdb, &quant);
        segment(&mut bytes, 0xc2, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
        let mut dc = huffman_table(0);
        dc[17] = 2;
        segment(&mut bytes, 0xc4, &dc);
        segment(&mut bytes, 0xda, &[1, 1, 0, 0, 0, 1]);
        bytes.push(first_scan);
        segment(&mut bytes, 0xda, &[1, 1, 0, 0, 0, 0x10]);
        bytes.extend_from_slice(&[0x7f, 0xff, 0xd9]);
        let image = decode_with_limits(&bytes, 2, 1).unwrap();
        assert_eq!(
            image.pixels.as_slice(),
            &[expected, expected, expected, 255]
        );
    }
}

fn sampled_frame() -> Frame {
    Frame::parse(&[8, 0, 9, 0, 17, 3, 1, 0x22, 0, 2, 0x11, 0, 3, 0x11, 0]).unwrap()
}

#[test]
fn scan_work_counts_actual_blocks_and_interleaved_padding() {
    let frame = sampled_frame();
    let luma = parse_scan(&[1, 1, 0, 0, 0, 0], &frame).unwrap();
    let chroma = parse_scan(&[1, 2, 0, 0, 0, 0], &frame).unwrap();
    let two = parse_scan(&[2, 1, 0, 2, 0, 0, 0, 0], &frame).unwrap();
    let all = parse_scan(&[3, 1, 0, 2, 0, 3, 0, 0, 0, 0], &frame).unwrap();
    assert_eq!(scan_geometry(&frame, &luma).unwrap(), (3, 6, 6));
    assert_eq!(scan_geometry(&frame, &chroma).unwrap(), (2, 2, 2));
    assert_eq!(scan_geometry(&frame, &two).unwrap(), (2, 2, 10));
    assert_eq!(scan_geometry(&frame, &all).unwrap(), (2, 2, 12));
}

#[test]
fn scan_work_checks_both_multiplications() {
    let mut frame = sampled_frame();
    let scan = parse_scan(&[2, 1, 0, 2, 0, 0, 0, 0], &frame).unwrap();
    frame.mcu_cols = usize::MAX;
    frame.mcu_rows = 2;
    assert_eq!(
        scan_geometry(&frame, &scan),
        Err("JPEG progressive: scan work exceeds limit")
    );
    frame.mcu_rows = 1;
    assert_eq!(
        scan_geometry(&frame, &scan),
        Err("JPEG progressive: scan work exceeds limit")
    );
}
