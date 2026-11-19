use super::*;

const RGB: [u8; 18] = [
    10, 20, 30, 70, 80, 90, 7, 8, 9, 1, 2, 3, 4, 5, 6, 11, 22, 33,
];

fn bmp(bits: u16, top_down: bool) -> Vec<u8> {
    let mut bytes = vec![0; 54];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&78u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&3i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&(if top_down { -2i32 } else { 2i32 }).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&bits.to_le_bytes());
    bytes[34..38].copy_from_slice(&24u32.to_le_bytes());
    let rows = if bits == 24 {
        [
            [30, 20, 10, 90, 80, 70, 9, 8, 7, 201, 202, 203],
            [3, 2, 1, 6, 5, 4, 33, 22, 11, 204, 205, 206],
        ]
    } else {
        // BI_RGB32's fourth byte is unused, including nonzero padding.
        [
            [30, 20, 10, 0, 90, 80, 70, 17, 9, 8, 7, 255],
            [3, 2, 1, 99, 6, 5, 4, 128, 33, 22, 11, 1],
        ]
    };
    for row in if top_down { rows } else { [rows[1], rows[0]] } {
        bytes.extend_from_slice(&row);
    }
    bytes
}

#[test]
fn bmp_import_has_three_bands_and_preserves_rows_padding_and_rgb32_unused_bytes() {
    initialize().unwrap();
    for bits in [24, 32] {
        for top_down in [false, true] {
            let source = bmp(bits, top_down);
            let image = load(&source, 1, false).unwrap();
            drop(source);
            assert_eq!(
                (image.get_width(), image.get_height(), image.get_bands()),
                (3, 2, 3)
            );
            assert!(!image.hasalpha());
            assert_eq!(image.write_to_memory(), RGB);
            let derived = image.extract_area(1, 0, 2, 2).unwrap();
            drop(image);
            assert_eq!(
                derived.write_to_memory(),
                [70, 80, 90, 7, 8, 9, 4, 5, 6, 11, 22, 33]
            );
        }
    }
}

#[test]
fn bmp_import_still_rejects_truncation_and_unsupported_compression_or_depth() {
    initialize().unwrap();
    let source = bmp(24, false);
    assert!(load(&source[..source.len() - 1], 1, false).is_err());
    let mut compressed = source.clone();
    compressed[30..34].copy_from_slice(&1u32.to_le_bytes());
    assert!(load(&compressed, 1, false).is_err());
    let mut depth = source;
    depth[28..30].copy_from_slice(&16u16.to_le_bytes());
    assert!(load(&depth, 1, false).is_err());
}

#[test]
fn ico_bridge_retains_real_alpha_and_hidden_rgb() {
    initialize().unwrap();
    let mut source = vec![
        0, 0, 1, 0, 1, 0, 2, 1, 0, 0, 1, 0, 32, 0, 48, 0, 0, 0, 22, 0, 0, 0,
    ];
    let mut dib = vec![0; 40];
    dib[..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&2i32.to_le_bytes());
    dib[8..12].copy_from_slice(&2i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    dib[20..24].copy_from_slice(&8u32.to_le_bytes());
    source.extend_from_slice(&dib);
    source.extend_from_slice(&[33, 22, 11, 0, 66, 55, 44, 128]);
    let image = load(&source, 1, false).unwrap();
    assert_eq!(image.get_bands(), 4);
    assert!(image.hasalpha());
    assert_eq!(image.write_to_memory(), [11, 22, 33, 0, 44, 55, 66, 128]);
}
