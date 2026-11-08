use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_in_chunks(input: &[u8], chunk_size: usize) -> [u8; 32] {
    let mut state = Sha256::new();
    state.update(&[]);
    for chunk in input.chunks(chunk_size) {
        state.update(chunk);
    }
    state.finish()
}

#[test]
fn sha256_known_vectors_and_block_boundaries() {
    // NIST SHA-256 examples, including padding across one and two blocks.
    let vectors: &[(&[u8], &str)] = &[
        (
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            concat!(
                "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn",
                "hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            )
            .as_bytes(),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ];
    for (input, expected) in vectors {
        for chunk in [1, 7, 55, 56, 63, 64, 65, 128] {
            assert_eq!(
                hex(&sha256_in_chunks(input, chunk)),
                *expected,
                "input length {}, chunk {chunk}",
                input.len()
            );
        }
    }
}

#[test]
fn hmac_sha256_rfc_4231_vectors() {
    // RFC 4231 test cases 1, 2, and 6. Case 6 hashes a key longer than a block.
    let first = hmac_sha256(&[0x0b; 20], &[b"Hi ", b"", b"There"]);
    assert_eq!(
        hex(&first),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    let second = hmac_sha256(b"Jefe", &[b"what do ya want ", b"for nothing?"]);
    assert_eq!(
        hex(&second),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    let long_key = hmac_sha256(
        &[0xaa; 131],
        &[b"Test Using Larger Than Block-Size Key - Hash Key First"],
    );
    assert_eq!(
        hex(&long_key),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn modified_signatures_paths_and_secrets_are_rejected() {
    let key = [0x31; 32];
    let salt = [0x73; 16];
    let path = "/rs:fit:8:8/plain/https://example.test/image.png";
    let signature = sign_imgproxy(path, &key, &salt);
    assert_eq!(signature.len(), 43);
    assert!(verify_imgproxy(&signature, path, &key, &salt));
    assert!(verify_imgproxy(&signature, &path[1..], &key, &salt));

    // A and Q have zero unused low bits, including at the last character.
    for position in [0, 21, 42] {
        let mut changed = signature.as_bytes().to_vec();
        changed[position] = if changed[position] == b'A' {
            b'Q'
        } else {
            b'A'
        };
        let changed = String::from_utf8(changed).unwrap();
        assert_eq!(decode_base64url(&changed, false).unwrap().len(), 32);
        assert!(!verify_imgproxy(&changed, path, &key, &salt));
    }
    assert!(!verify_imgproxy(
        &signature,
        "/rs:fit:8:9/plain/https://example.test/image.png",
        &key,
        &salt
    ));
    assert!(!verify_imgproxy(&signature, path, &[0x30; 32], &salt));
    assert!(!verify_imgproxy(&signature, path, &key, &[0x72; 16]));
    assert!(!verify_imgproxy(&signature[..42], path, &key, &salt));
    assert!(!verify_imgproxy(
        &format!("{signature}="),
        path,
        &key,
        &salt
    ));
    assert!(!verify_imgproxy("%", path, &key, &salt));
}

#[test]
fn base64url_and_hex_reject_malformed_encodings() {
    assert_eq!(decode_base64url("_w", false).unwrap(), vec![0xff]);
    assert_eq!(decode_base64url("_w==", true).unwrap(), vec![0xff]);
    for bad in ["A", "_x", "AB", "a+b", "_w=="] {
        assert!(decode_base64url(bad, false).is_err(), "{bad}");
    }
    assert!(decode_base64url("_w===", true).is_err());
    assert_eq!(decode_hex("00aAfF").unwrap(), vec![0, 0xaa, 0xff]);
    for bad in ["", "0", "0x00", "00x1", "AA\n"] {
        assert!(decode_hex(bad).is_err(), "{bad:?}");
    }
}
