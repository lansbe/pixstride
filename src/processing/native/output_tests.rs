use super::parse_output;

fn frame() -> Vec<u8> {
    let mut bytes = b"PXN2".to_vec();
    for value in [3u64, 2, 9, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(b"\x89PNG\r\n\x1a\nfixture");
    bytes
}

#[test]
fn parsed_worker_payload_owns_the_received_frame_without_copying() {
    let bytes = frame();
    let pointer = bytes[37..].as_ptr();
    let output = parse_output(bytes).unwrap();
    assert_eq!(output.body.as_slice(), b"\x89PNG\r\n\x1a\nfixture");
    assert_eq!((output.width, output.height), (3, 2));
    assert_eq!(output.origin, [9, 7]);
    assert_eq!(output.mime, "image/png");
    assert_eq!(output.body.as_slice().as_ptr(), pointer);
}

#[test]
fn owning_a_frame_keeps_header_dimension_format_and_truncation_guards() {
    let good = frame();
    for length in 0..41 {
        assert!(parse_output(good[..length.min(good.len())].to_vec()).is_err());
    }
    for position in [0, 4, 12, 20, 28, 36] {
        let mut bytes = good.clone();
        if position == 0 {
            bytes[0] ^= 1;
        } else if position == 36 {
            bytes[36] = 255;
        } else {
            bytes[position..position + 8].fill(0);
        }
        assert!(parse_output(bytes).is_err(), "position {position}");
    }
}
