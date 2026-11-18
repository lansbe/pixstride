use super::*;

#[test]
fn packed_rgb_bmp_matches_rgba_output_and_decodes_odd_widths_and_padding() {
    for (width, height) in [(1, 1), (3, 7), (33, 19)] {
        let rgb: Vec<u8> = (0..width * height * 3)
            .map(|i| (i * 73 + 129) as u8)
            .collect();
        let mut rgba = Buffer::new(width * height * 4).unwrap();
        for (src, dst) in rgb
            .chunks_exact(3)
            .zip(rgba.as_mut_slice().chunks_exact_mut(4))
        {
            dst[..3].copy_from_slice(src);
            dst[3] = 255;
        }
        let expected = encode_bmp(&Image {
            width,
            height,
            pixels: rgba,
        })
        .unwrap();
        let actual = encode_bmp_rgb(width, height, &rgb).unwrap();
        assert_eq!(actual.as_slice(), expected.as_slice());
        let decoded = bmp(actual.as_slice()).unwrap();
        for (src, pixel) in rgb
            .chunks_exact(3)
            .zip(decoded.pixels.as_slice().chunks_exact(4))
        {
            assert_eq!(&pixel[..3], src);
            assert_eq!(pixel[3], 255);
        }
    }
    assert!(encode_bmp_rgb(0, 1, &[]).is_err());
    assert!(encode_bmp_rgb(2, 2, &[0; 11]).is_err());
}

#[test]
fn v5_alpha_masks_preserve_rgba_and_legacy_rgb32_reserved_bytes_stay_opaque() {
    let bottom_up = [66, 55, 44, 128, 33, 22, 11, 0];
    let top_down = [33, 22, 11, 0, 66, 55, 44, 128];
    for compression in [0u32, 3] {
        for (height, raster) in [(2i32, bottom_up), (-2, top_down)] {
            let mut bytes = vec![0; 146];
            bytes[..54].copy_from_slice(&bmp_header(1, 2, 146));
            for (start, field) in [
                (10, 138u32),
                (14, 124),
                (22, height as u32),
                (30, compression),
                (34, 8),
                (54, 0x00ff0000),
                (58, 0x0000ff00),
                (62, 0x000000ff),
                (66, 0xff000000),
                (70, 0x73524742),
            ] {
                bytes[start..start + 4].copy_from_slice(&field.to_le_bytes());
            }
            bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
            bytes[138..].copy_from_slice(&raster);
            assert_eq!(
                bmp(&bytes).unwrap().pixels.as_slice(),
                [11, 22, 33, 0, 44, 55, 66, 128]
            );
            assert!(bmp(&bytes[..145]).is_err());
            if compression == 3 {
                bytes[69] = 0;
                assert!(bmp(&bytes).is_err());
            }
        }
    }
    let mut bytes = bmp_header(1, 2, 62).to_vec();
    bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&bottom_up);
    assert_eq!(
        bmp(&bytes).unwrap().pixels.as_slice(),
        [11, 22, 33, 255, 44, 55, 66, 255]
    );
}
