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

#[test]
fn truncated_rotated_signatures_and_explicit_trust_do_not_change_path_binding() {
    let path = "/rs:fit:3:2/plain/local:///test.png@png";
    for size in [1, 16, 17, 31, 32] {
        let mut config = Config::signed(vec![1; 32], vec![2; 16]).unwrap();
        config.signature_size = size;
        let signed = config.sign(path).unwrap();
        assert!(parse(&signed, &config).is_ok());
        assert!(parse(&signed.replace("fit:3:2", "fit:4:2"), &config).is_err());
        if size != 32 {
            assert!(parse(&signed, &Config::signed(vec![1; 32], vec![2; 16]).unwrap()).is_err());
        }
        config.trusted.push("configured-trust".into());
        assert!(parse(&format!("/configured-trust{path}"), &config).is_ok());
        assert!(parse(&format!("/other{path}"), &config).is_err());
    }
}

#[test]
fn hints_are_defaults_and_explicit_url_options_win() {
    let c = Config::unsafe_local();
    let r = parse_with_hints("/unsafe/plain/local:///x@png", &c, (Some(24), Some(2.0))).unwrap();
    assert_eq!((r.options.width, r.extra.dpr), (12, 2.0));
    let r = parse_with_hints(
        "/unsafe/w:8/dpr:1/plain/local:///x@png",
        &c,
        (Some(24), Some(2.0)),
    )
    .unwrap();
    assert_eq!((r.options.width, r.extra.dpr), (8, 1.0));
}
#[test]
fn preset_only_paths_and_url_policies_run_after_signature_validation() {
    let mut c = Config::unsafe_local().with_presets("thumb=w:8").unwrap();
    c.policy.only_presets = true;
    assert_eq!(
        parse("/unsafe/thumb/plain/local:///x@png", &c)
            .unwrap()
            .options
            .width,
        8
    );
    assert!(parse("/unsafe/w:8/plain/local:///x@png", &c).is_err());
    c.policy.only_presets = false;
    c.policy.separator = ",".into();
    assert_eq!(
        parse("/unsafe/w,12/plain/local:///x@png", &c)
            .unwrap()
            .options
            .width,
        12
    );
    c.policy.separator = ":".into();
    c.policy.allow_security = false;
    assert!(parse("/unsafe/maf:2/plain/local:///x@png", &c).is_err());
}
