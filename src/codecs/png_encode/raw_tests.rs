use super::*;

#[test]
fn packed_rgb_encoding_keeps_pixels_and_rgba_encoder_bytes() {
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
        let expected = encode(&Image {
            width,
            height,
            pixels: rgba,
        })
        .unwrap();
        let encoded = encode_pixels(width, height, &rgb, 3).unwrap();
        assert_eq!(encoded.as_slice(), expected.as_slice());
        let decoded = crate::png::decode(encoded.as_slice()).unwrap();
        assert_eq!((decoded.width, decoded.height), (width, height));
        for (pixel, expected) in decoded
            .pixels
            .as_slice()
            .chunks_exact(4)
            .zip(rgb.chunks_exact(3))
        {
            assert_eq!(&pixel[..3], expected);
            assert_eq!(pixel[3], 255);
        }
    }
    assert!(encode_pixels(0, 1, &[], 3).is_err());
    assert!(encode_pixels(2, 2, &[0; 11], 3).is_err());
}
