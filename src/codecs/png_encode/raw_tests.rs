use super::*;

#[test]
fn gray_png_preserves_literal_samples_without_rgb_expansion() {
    let data = [0, 127, 255, 1, 64, 128];
    let bytes = encode_pixels(3, 2, data, 1).unwrap();
    assert_eq!(&bytes.as_slice()[24..26], &[8, 0]);
    let image = crate::png::decode(bytes.as_slice()).unwrap();
    assert_eq!((image.width, image.height), (3, 2));
    assert_eq!(
        image.pixels.as_slice(),
        &[
            0, 0, 0, 255, 127, 127, 127, 255, 255, 255, 255, 255, 1, 1, 1, 255, 64, 64, 64, 255,
            128, 128, 128, 255,
        ]
    );
}

#[test]
fn gray_alpha_png_preserves_partial_and_invisible_samples() {
    let data = [11, 0, 22, 1, 33, 128, 44, 254, 55, 255, 66, 100];
    let bytes = encode_pixels(3, 2, data, 2).unwrap();
    assert_eq!(&bytes.as_slice()[24..26], &[8, 4]);
    let image = crate::png::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        image.pixels.as_slice(),
        &[
            11, 11, 11, 0, 22, 22, 22, 1, 33, 33, 33, 128, 44, 44, 44, 254, 55, 55, 55, 255, 66,
            66, 66, 100,
        ]
    );
    let opaque = encode_pixels(3, 1, [0, 255, 128, 255, 255, 255], 2).unwrap();
    assert_eq!(&opaque.as_slice()[24..26], &[8, 0]);
    assert_eq!(
        crate::png::decode(opaque.as_slice())
            .unwrap()
            .pixels
            .as_slice(),
        &[0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
    );
}

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
        let owned = encode_pixels(width, height, rgb.clone(), 3).unwrap();
        assert_eq!(owned.as_slice(), encoded.as_slice());
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
    assert!(encode_pixels(0, 1, [], 3).is_err());
    assert!(encode_pixels(2, 2, [0; 11], 3).is_err());
}
