use super::*;

fn huffman_table(tag: u8, symbol: u8) -> [u8; 18] {
    let mut table = [0; 18];
    table[0] = tag;
    table[1] = 1;
    table[17] = symbol;
    table
}

fn segment(bytes: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    bytes.extend_from_slice(&[0xff, marker]);
    bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
}

#[test]
fn huffman_budget_counts_each_definition_across_segments() {
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut remaining = 2;
    parse_dht(&huffman_table(0, 0), &mut dc, &mut ac, &mut remaining).unwrap();
    parse_dht(&huffman_table(0, 1), &mut dc, &mut ac, &mut remaining).unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(dc[0].values[0], 1);
    assert_eq!(
        parse_dht(&huffman_table(0, 2), &mut dc, &mut ac, &mut remaining),
        Err("JPEG: Huffman table budget exceeded")
    );
    assert_eq!(dc[0].values[0], 1);
}

#[test]
fn huffman_budget_counts_tables_inside_one_segment() {
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut payload = Vec::new();
    payload.extend_from_slice(&huffman_table(0, 0));
    payload.extend_from_slice(&huffman_table(0x10, 0));
    let mut remaining = 1;
    assert_eq!(
        parse_dht(&payload, &mut dc, &mut ac, &mut remaining),
        Err("JPEG: Huffman table budget exceeded")
    );
    assert_eq!(remaining, 0);
    assert!(dc[0].present);
    assert!(!ac[0].present);
}

#[test]
fn huffman_exhaustion_precedes_table_construction() {
    let mut dc = [Huffman::EMPTY; 4];
    let mut ac = [Huffman::EMPTY; 4];
    let mut remaining = 0;
    let mut invalid_counts = huffman_table(0, 0);
    invalid_counts[1] = 3;
    assert_eq!(
        parse_dht(&invalid_counts, &mut dc, &mut ac, &mut remaining),
        Err("JPEG: Huffman table budget exceeded")
    );
    assert!(!dc[0].present);
}

fn baseline_image(cmyk: bool) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8];
    let mut quant = [1; 65];
    quant[0] = 0;
    segment(&mut bytes, 0xdb, &quant);
    if cmyk {
        segment(&mut bytes, 0xee, b"Adobe\0\0\0\0\0\0\0");
        segment(
            &mut bytes,
            0xc0,
            &[
                8, 0, 1, 0, 1, 4, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 4, 0x11, 0,
            ],
        );
    } else {
        segment(&mut bytes, 0xc0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    }
    segment(&mut bytes, 0xc4, &huffman_table(0, 0));
    segment(&mut bytes, 0xc4, &huffman_table(0x10, 0));
    if cmyk {
        segment(&mut bytes, 0xda, &[4, 1, 0, 2, 0, 3, 0, 4, 0, 0, 63, 0]);
        bytes.push(0);
    } else {
        segment(&mut bytes, 0xda, &[1, 1, 0, 0, 63, 0]);
        bytes.push(0x3f);
    }
    bytes.extend_from_slice(&[0xff, 0xd9]);
    bytes
}

#[test]
fn baseline_and_cmyk_pixels_are_preserved_with_separate_dht_segments() {
    let gray = decode(&baseline_image(false)).unwrap();
    assert_eq!((gray.width, gray.height), (1, 1));
    assert_eq!(gray.pixels.as_slice(), &[128, 128, 128, 255]);
    let cmyk = cmyk::decode(&baseline_image(true)).unwrap();
    assert_eq!((cmyk.width, cmyk.height), (1, 1));
    assert_eq!(cmyk.pixels.as_slice(), &[64, 64, 64, 255]);
}
