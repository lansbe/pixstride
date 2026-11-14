use super::{public, Url};
use std::net::Ipv4Addr;

#[test]
fn ipv4_policy_separates_global_examples_from_local_and_reserved_ranges() {
    // No resolution or network access: these values reach the IP predicate only.
    let denied = [
        "0.1.2.3",
        "10.0.0.1",
        "100.64.0.1",
        "100.127.255.254",
        "127.0.0.1",
        "169.254.1.1",
        "172.16.0.1",
        "172.31.255.254",
        "192.168.1.1",
        "192.0.0.1",
        "192.88.99.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.19.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
    ];
    let permitted = [
        "1.1.1.1",
        "8.8.8.8",
        "100.63.255.255",
        "100.128.0.1",
        "172.15.255.255",
        "172.32.0.1",
    ];
    for address in denied {
        let ip: Ipv4Addr = address.parse().unwrap();
        assert!(!public(ip), "unexpectedly allowed {ip}");
    }
    for address in permitted {
        let ip: Ipv4Addr = address.parse().unwrap();
        assert!(public(ip), "unexpectedly denied {ip}");
    }
}

#[test]
fn remote_url_parser_accepts_only_supported_authorities() {
    let url = Url::parse("https://EXAMPLE.TEST:443/a.png?x=1").unwrap();
    assert_eq!(url.scheme, "https");
    assert_eq!(url.host, "example.test");
    assert_eq!(url.port, 443);
    assert_eq!(url.path, "/a.png?x=1");
    let invalid = [
        "file://example.test/a.png",
        "https://example.test:/a.png",
        "https://user@example.test/a.png",
        "https://example.test:0/a.png",
        "https://example.test:notaport/a.png",
        "https://example.test/a.png#fragment",
        "https://example.test/a b",
        "https://example.test/\n",
        "https://example.test/\\a",
        "https://example.test/\u{00e9}",
    ];
    for value in invalid {
        assert!(Url::parse(value).is_err(), "accepted {value:?}");
    }
    let ipv6 = Url::parse("https://[2001:4860:4860::8888]:8443/a").unwrap();
    assert_eq!(ipv6.host, "2001:4860:4860::8888");
    assert_eq!(
        ipv6.redirect("/next").unwrap().full,
        "https://[2001:4860:4860::8888]:8443/next"
    );
    assert!(Url::parse("https://[::1]extra/").is_err());
    assert!(Url::parse("https://[fe80::1%25en0]/").is_err());
}

#[test]
fn redirect_parsing_is_bounded_to_supported_url_syntax() {
    let base = Url::parse("https://example.test/a/image.png?old=1").unwrap();
    let next = base.redirect("?new=1").unwrap();
    assert_eq!(next.host, "example.test");
    assert_eq!(next.path, "/a/image.png?new=1");
    let next = base.redirect("//example.test/next.png").unwrap();
    assert_eq!(next.scheme, "https");
    assert_eq!(next.host, "example.test");
    assert_eq!(next.path, "/next.png");
    assert!(base.redirect("https://user@example.test/a.png").is_err());
    assert!(base.redirect("#fragment").is_err());
}
