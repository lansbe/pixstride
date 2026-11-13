use super::*;

#[test]
fn rotated_keys_authenticate_the_same_unexpanded_path() {
    let config =
        Config::rotating(vec![(vec![1; 32], vec![2; 16]), (vec![3; 32], vec![4; 16])]).unwrap();
    let path = "/rs:fit:3:2/plain/local:///test.png@png";
    for (key, salt) in [(1, 2), (3, 4)] {
        let sig = signature::sign_imgproxy(path, &[key; 32], &[salt; 16]);
        assert!(parse(&format!("/{sig}{path}"), &config).is_ok());
    }
    assert!(parse(&format!("/unsafe{path}"), &config).is_err());
    assert!(Config::rotating(Vec::new()).is_err());
}

#[test]
fn presets_keep_order_defaults_and_signature_binding() {
    let config = Config::signed(vec![1; 32], vec![2; 16])
        .unwrap()
        .with_presets("default=w:2;thumb=rs:fit:4:3;sharp=pr:thumb/sh:1")
        .unwrap();
    let path = "/pr:sharp/w:7/plain/local:///test.png@png";
    let sig = signature::sign_imgproxy(path, &[1; 32], &[2; 16]);
    let r = parse(&format!("/{sig}{path}"), &config).unwrap();
    assert_eq!((r.options.width, r.options.height), (7, 3));
    assert_eq!(r.effects.sharpen_sigma, 1.0);
    assert!(parse(
        &format!("/{sig}{}", path.replace("sharp", "thumb")),
        &config
    )
    .is_err());
    for value in [
        "a=pr:a",
        "a=pr:b;b=pr:a",
        "a=pr:unknown",
        "a=w:1;a=h:1",
        "a=plain/local:///x",
    ] {
        assert!(
            Config::unsafe_local().with_presets(value).is_err(),
            "{value}"
        );
    }
}

#[test]
fn expanded_presets_have_a_cumulative_size_limit() {
    let value = "a=w:1;b=pr:a:a:a:a:a:a:a:a;c=pr:b:b:b:b:b:b:b:b;d=pr:c:c:c:c:c:c:c:c";
    assert!(Config::unsafe_local().with_presets(value).is_err());
}

#[test]
fn extended_options_are_finite_and_never_raise_hard_limits() {
    let c = Config::unsafe_local();
    let r=parse("/unsafe/rs:fill-down:3:2:1:1/dpr:2/z:1.5:2/g:sm/c:0.5:0.25/fq:jpg:75:webp:60/plain/local:///x@avif",&c).unwrap();
    assert!(r.extra.native && r.extra.fill_down && r.extra.smart && r.extra.extend);
    assert_eq!(r.extra.crop_size, Some([0.5, 0.25]));
    assert_eq!(r.extra.zoom, [1.5, 2.0]);
    assert_eq!(r.format, Some(OutputFormat::Avif));
    for option in [
        "dpr:NaN",
        "dpr:0",
        "z:inf",
        "g:ce:NaN",
        "msr:51",
        "msfs:0",
        "maf:257",
        "mb:67108865",
        "fn:bad%0Dname",
        "fq:webp:101",
    ] {
        assert!(
            parse(&format!("/unsafe/{option}/plain/local:///x@png"), &c).is_err(),
            "{option}"
        );
    }
}

#[test]
fn expiration_and_filenames_are_validated_before_source_access() {
    let c = Config::unsafe_local();
    assert!(matches!(
        parse("/unsafe/exp:1/plain/local:///x@png", &c),
        Err("expired URL")
    ));
    let r = parse(
        "/unsafe/exp:0/fn:photo%20one/att:1/plain/local:///x@png",
        &c,
    )
    .unwrap();
    assert!(r.extra.attachment);
    assert_eq!(r.extra.filename.as_deref(), Some("photo one"));
}
