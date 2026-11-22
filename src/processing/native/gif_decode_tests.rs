use super::{lzw, try_load};
use crate::{
    processing::native::{engine, raster},
    request::{self, Config},
};

fn request() -> request::Request {
    request::parse(
        "/unsafe/c:0.75:0.5:noea/rs:force:7:5/plain/local:///fixture.gif@jpg",
        &Config::unsafe_local(),
    )
    .unwrap()
}

fn fixture(width: u16, height: u16, interlaced: bool, transparent: Option<u8>) -> Vec<u8> {
    let indices = (0..usize::from(width) * usize::from(height))
        .map(|i| ((i / 7 + i) % 4) as u8)
        .collect::<Vec<_>>();
    let mut frame = gif::Frame::from_indexed_pixels(width, height, indices, transparent);
    frame.interlaced = interlaced;
    let mut data = Vec::new();
    let mut encoder = gif::Encoder::new(
        &mut data,
        width,
        height,
        &[23, 81, 7, 3, 17, 252, 255, 243, 221, 87, 55, 13],
    )
    .unwrap();
    encoder.write_frame(&frame).unwrap();
    drop(encoder);
    data
}

#[test]
fn indexed_full_frame_matches_native_rgb_even_at_interlace_boundaries() {
    engine::initialize().unwrap();
    for (w, h) in [(1, 1), (7, 2), (1, 9), (23, 17)] {
        for interlaced in [false, true] {
            let data = fixture(w, h, interlaced, None);
            let head = engine::load(&data, 1, false).unwrap();
            let image = try_load(&data, &head, &request()).unwrap().unwrap();
            assert_eq!(
                &*raster::export(&image).unwrap(),
                &*raster::export(&head).unwrap()
            );
        }
    }
}

#[test]
fn indexed_import_respects_limits_metadata_transparency_and_trailer() {
    engine::initialize().unwrap();
    let data = fixture(9, 7, false, None);
    let head = engine::load(&data, 1, false).unwrap();
    let mut r = request();
    r.strip_metadata = false;
    assert!(try_load(&data, &head, &r).unwrap().is_none());
    r.strip_metadata = true;
    r.extra.max_source_pixels = 62;
    assert!(try_load(&data, &head, &r).is_err());
    r.extra.max_source_pixels = 63;
    r.extra.max_frame_pixels = 62;
    assert!(try_load(&data, &head, &r).is_err());
    assert!(try_load(&data[..data.len() - 1], &head, &request()).is_err());
    let alpha = fixture(9, 7, false, Some(2));
    let head = engine::load(&alpha, 1, false).unwrap();
    assert!(try_load(&alpha, &head, &request()).unwrap().is_none());
}

#[test]
fn lzw_roundtrips_literals_repeated_phrases_resets_and_all_widths() {
    // Compressed fixtures come from the independent weezl encoder. Interleave
    // repeated phrases and noise so every minimum width crosses dictionary resets.
    for minimum in 2..=8 {
        let mask = ((1u16 << minimum) - 1) as u8;
        let mut seed = 99u32;
        let indices = (0..65535)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if i % 503 < 193 {
                    (i % 3) as u8
                } else {
                    (seed >> 24) as u8 & mask
                }
            })
            .collect::<Vec<_>>();
        let mut frame = gif::Frame::from_indexed_pixels(255, 257, indices.clone(), None);
        frame.make_lzw_pre_encoded();
        let (width, bytes) = frame.buffer.split_first().unwrap();
        assert_eq!(*width, minimum);
        let mut output = vec![0; indices.len()];
        lzw::decode(bytes, &mut output, *width).unwrap();
        assert_eq!(output, indices);
        assert!(lzw::decode(&bytes[..bytes.len() - 1], &mut output, *width).is_err());
        assert!(lzw::decode(bytes, &mut output[1..], *width).is_err());
    }
}

#[test]
fn full_dictionary_remains_readable_until_clear() {
    // GIF permits the encoder to freeze the full dictionary indefinitely.
    // Issue literal codes through all width changes, then reuse its final entry.
    let mut packed = Vec::new();
    let mut bits = 0u32;
    let mut count = 0u8;
    let mut push = |code: u16, width: u8| {
        bits |= u32::from(code) << count;
        count += width;
        while count >= 8 {
            packed.push(bits as u8);
            bits >>= 8;
            count -= 8;
        }
    };
    let mut expected = Vec::new();
    let mut width = 9;
    push(256, width);
    for i in 0..4_300 {
        let literal = (i % 253) as u8;
        expected.push(literal);
        push(u16::from(literal), width);
        if 258 + i == 1 << width && width < 12 {
            width += 1;
        }
    }
    push(4095, width);
    expected.extend_from_within(3837..3839);
    push(256, width);
    push(220, 9);
    expected.push(220);
    push(257, 9);
    if count > 0 {
        packed.push(bits as u8);
    }
    let mut decoded = vec![0; expected.len()];
    lzw::decode(&packed, &mut decoded, 8).unwrap();
    assert_eq!(decoded, expected);
}
