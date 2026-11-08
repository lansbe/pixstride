use crate::{
    extra_encode, gif_encode, image, jpeg_encode, pipeline, png_encode, request, signature, webp,
    Buffer, Image,
};

#[test]
fn request_authentication_binds_the_signature_to_the_processing_path() {
    let key = vec![0x31; 32];
    let salt = vec![0x73; 16];
    assert!(request::Config::signed(vec![0; 31], salt.clone()).is_err());
    assert!(request::Config::signed(key.clone(), vec![0; 15]).is_err());
    let config = request::Config::signed(key.clone(), salt.clone()).unwrap();
    let path = "/rs:fit:8:8/plain/https://example.test/image.png";
    let sig = signature::sign_imgproxy(path, &key, &salt);
    let valid = format!("/{sig}{path}");
    let parsed = request::parse(&valid, &config).unwrap();
    assert_eq!(parsed.source, "https://example.test/image.png");

    // The outer HTTP query is explicitly excluded from this signature format.
    assert!(request::parse(&format!("{valid}?ignored=1"), &config).is_ok());
    assert!(request::parse(
        &format!("/{sig}/rs:fit:8:9/plain/https://example.test/image.png"),
        &config
    )
    .is_err());
    assert!(request::parse(
        &format!("/{sig}/rs:fit:8:8/plain/https://example.test/other.png"),
        &config
    )
    .is_err());
    assert!(request::parse(&format!("/unsafe{path}"), &config).is_err());
    let mut changed = sig.into_bytes();
    changed[0] = if changed[0] == b'A' { b'Q' } else { b'A' };
    let changed = String::from_utf8(changed).unwrap();
    assert!(request::parse(&format!("/{changed}{path}"), &config).is_err());

    let query_path = "/plain/https://example.test/image.png%3Fx%3D1";
    let query_sig = signature::sign_imgproxy(query_path, &key, &salt);
    let target = format!("/{query_sig}{query_path}?ignored=1");
    assert_eq!(
        request::parse(&target, &config).unwrap().source,
        "https://example.test/image.png?x=1"
    );
}

#[test]
fn request_parser_rejects_malformed_encoding_and_unsupported_options() {
    let config = request::Config::unsafe_local();
    assert!(request::parse("/unsafe/plain/https://example.test/a.png", &config).is_ok());
    let invalid = [
        "/unsafe/plain/https://example.test/%",
        "/unsafe/plain/https://example.test/%2",
        "/unsafe/plain/https://example.test/%ZZ",
        "/unsafe/plain/https://example.test/%00",
        "/unsafe/plain/https://example.test/%FF",
        "/unsafe/plain/https://example.test/a%20b",
        "/unsafe/plain/https://example.test%40example.test/a.png",
        "/unsafe/w:100001/plain/https://example.test/a.png",
        "/unsafe/q:101/plain/https://example.test/a.png",
        "/unsafe/wm:NaN/plain/https://example.test/a.png",
        "/unsafe/not-an-option:1/plain/https://example.test/a.png",
        "/unsafe/A",
    ];
    for target in invalid {
        assert!(
            request::parse(target, &config).is_err(),
            "accepted {target:?}"
        );
    }
}

fn tiny_image() -> Image {
    let mut pixels = Buffer::new(4).unwrap();
    pixels.as_mut_slice().copy_from_slice(&[33, 66, 99, 255]);
    Image {
        width: 1,
        height: 1,
        pixels,
    }
}

#[test]
fn small_images_reject_deterministic_truncations() {
    let image = tiny_image();
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        (
            "PNG",
            png_encode::encode(&image).unwrap().as_slice().to_vec(),
        ),
        (
            "JPEG",
            jpeg_encode::encode(&image, 80).unwrap().as_slice().to_vec(),
        ),
        ("WebP", webp::encode(&image).unwrap().as_slice().to_vec()),
        (
            "BMP",
            image::encode_bmp(&image).unwrap().as_slice().to_vec(),
        ),
        (
            "GIF",
            gif_encode::encode(&image).unwrap().as_slice().to_vec(),
        ),
        (
            "TIFF",
            extra_encode::encode_tiff(&image)
                .unwrap()
                .as_slice()
                .to_vec(),
        ),
        (
            "ICO",
            extra_encode::encode_ico(&image)
                .unwrap()
                .as_slice()
                .to_vec(),
        ),
        ("PPM", b"P6\n1 1\n255\n\x21\x42\x63".to_vec()),
        ("SVG", b"<svg width=\"1\" height=\"1\"></svg>".to_vec()),
    ];
    for (name, bytes) in fixtures {
        let decoded = pipeline::decode(&bytes)
            .unwrap_or_else(|error| panic!("{name} fixture failed: {error}"));
        assert_eq!((decoded.width, decoded.height), (1, 1), "{name}");
        for end in [0, 1, 2, 4, 8, 16, bytes.len() / 2, bytes.len() - 1] {
            if end < bytes.len() {
                assert!(
                    pipeline::decode(&bytes[..end]).is_err(),
                    "{name} accepted {end}/{} bytes",
                    bytes.len()
                );
            }
        }
    }
}

#[test]
fn invalid_dimensions_checksum_and_svg_external_content_fail_closed() {
    assert!(image::dimensions(0, 1).is_err());
    assert!(image::dimensions(100_001, 1).is_err());
    assert!(image::dimensions(usize::MAX, 2).is_err());

    let encoded = png_encode::encode(&tiny_image()).unwrap();
    let mut corrupt = encoded.as_slice().to_vec();
    // The first chunk is IHDR: byte 29 is the first byte of its CRC.
    assert_eq!(&corrupt[..16], b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    corrupt[29] ^= 1;
    assert!(pipeline::decode(&corrupt).is_err());

    let invalid_svg: &[&[u8]] = &[
        br#"<svg width="1" height="1"><image href="https://example.test/a.png"/></svg>"#,
        br#"<!DOCTYPE svg [<!ENTITY x SYSTEM "https://example.test/a">]><svg width="1" height="1">&x;</svg>"#,
        br#"<svg width="1" height="1"><script/></svg>"#,
    ];
    for source in invalid_svg {
        assert!(crate::svg::decode(source).is_err(), "accepted {source:?}");
    }
}
