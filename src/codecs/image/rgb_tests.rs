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
