use super::*;

#[test]
fn shared_fixed_alphabet_matches_the_canonical_builder() {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    let expected = (
        Huffman::build(&lengths).unwrap(),
        Huffman::build(&[5; 32]).unwrap(),
    );
    let actual = fixed_trees().unwrap();
    for (a, b) in [(&actual.0, &expected.0), (&actual.1, &expected.1)] {
        assert_eq!(a.counts, b.counts);
        assert_eq!(a.first, b.first);
        assert_eq!(a.offset, b.offset);
        assert_eq!(a.symbols, b.symbols);
        assert_eq!(a.fast, b.fast);
    }
    assert!(std::ptr::eq(actual, fixed_trees().unwrap()));
}

fn empty_stored_stream(blocks: usize) -> Vec<u8> {
    let mut stream = vec![0x78, 0x01];
    for i in 0..blocks {
        stream.extend_from_slice(&[u8::from(i + 1 == blocks), 0, 0, 255, 255]);
    }
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream
}

#[test]
fn empty_blocks_are_limited_independently_of_decoded_bytes() {
    assert!(inflate(&empty_stored_stream(1), &mut []).is_ok());
    assert!(inflate(&empty_stored_stream(16_384), &mut []).is_ok());
    assert_eq!(
        inflate(&empty_stored_stream(16_385), &mut []),
        Err("DEFLATE: block work limit")
    );
}
