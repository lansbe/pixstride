use super::handle;
use crate::{fetch, request, signature, watermark};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::OnceLock,
    time::Duration,
};

fn exchange(config: request::Config, build_request: impl FnOnce(u16) -> String) -> String {
    String::from_utf8(exchange_bytes(
        config,
        Default::default(),
        None,
        build_request,
    ))
    .unwrap()
}

fn exchange_bytes(
    config: request::Config,
    settings: crate::network::settings::Settings,
    root: Option<crate::local::LocalRoot>,
    build_request: impl FnOnce(u16) -> String,
) -> Vec<u8> {
    static POOL: OnceLock<fetch::Pool> = OnceLock::new();
    let pool = POOL.get_or_init(|| fetch::Pool::new().unwrap());
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let request = build_request(port);
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let watermark = watermark::Config::from_path(None, 1.0).unwrap();
        handle(
            &mut socket,
            port,
            &root,
            &config,
            &watermark,
            pool,
            &crate::network::engine::Engine::new(1),
            &settings,
        );
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    client.write_all(request.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    worker.join().unwrap();
    response
}

fn signed_config() -> request::Config {
    request::Config::signed(vec![0x42; 32], vec![0x37; 16]).unwrap()
}

#[test]
fn unsigned_health_and_image_accept_expected_loopback_authority() {
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let response = exchange(request::Config::unsafe_local(), |port| {
            format!("GET /health HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n")
        });
        assert!(response.starts_with("HTTP/1.1 200 "), "{host}: {response}");
        assert!(response.ends_with("\r\n\r\nok\n"));
    }
    let response = exchange(request::Config::unsafe_local(), |port| {
        format!(
            "GET /unsafe/plain/local:///sample.png@png HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"
        )
    });
    assert!(response.starts_with("HTTP/1.1 422 "));
    assert!(response.ends_with("local sources are disabled"));
}

#[test]
fn unsigned_rejects_public_and_wrong_port_hosts_before_routing() {
    for host in ["example.test:8080", "localhost", "localhost:1"] {
        let response = exchange(request::Config::unsafe_local(), |_| {
            format!("GET /health HTTP/1.1\r\nHost: {host}\r\n\r\n")
        });
        assert!(response.starts_with("HTTP/1.1 400 "), "{host}: {response}");
    }
}

#[test]
fn signed_request_behind_public_host_still_requires_valid_signature() {
    let path = "/plain/local:///sample.png@png";
    let signature = signature::sign_imgproxy(path, &[0x42; 32], &[0x37; 16]);
    let response = exchange(signed_config(), |_| {
        format!("GET /{signature}{path} HTTP/1.1\r\nHost: images.example.test:443\r\n\r\n")
    });
    assert!(response.starts_with("HTTP/1.1 422 "), "{response}");
    assert!(response.ends_with("local sources are disabled"));

    for target in [
        "/unsafe/plain/local:///sample.png@png",
        "/invalid/plain/local:///sample.png@png",
    ] {
        let response = exchange(signed_config(), |_| {
            format!("GET {target} HTTP/1.1\r\nHost: images.example.test:443\r\n\r\n")
        });
        assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
        assert!(response.ends_with("invalid signature"));
    }
}

#[test]
fn fragmented_header_still_checks_unsigned_host() {
    for (host, expected) in [("localhost", "200"), ("example.test", "400")] {
        let response = exchange(request::Config::unsafe_local(), |port| {
            format!(
                "GET /health HTTP/1.1\r\nX-Padding: {}\r\nHost: {host}:{port}\r\n\r\n",
                "a".repeat(3000)
            )
        });
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected} ")),
            "{response}"
        );
    }
}

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pixstride-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("sample.ppm"), b"P6\n2 1\n255\n\xff\0\0\0\xff\0").unwrap();
        Self(path)
    }
    fn root(&self) -> Option<crate::local::LocalRoot> {
        Some(crate::local::LocalRoot::open(&self.0).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn parts(response: &[u8]) -> (&str, &[u8]) {
    let end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    (
        std::str::from_utf8(&response[..end]).unwrap(),
        &response[end..],
    )
}
fn field<'a>(headers: &'a str, name: &str) -> &'a str {
    headers
        .split("\r\n")
        .find_map(|line| line.strip_prefix(name))
        .unwrap()
        .trim()
}

#[test]
fn authorization_precedes_routes_and_metrics_remain_bounded() {
    for (token, status) in [
        ("", "401"),
        ("Bearer wrong", "401"),
        ("Bearer example", "200"),
    ] {
        for route in ["/health", "/metrics"] {
            let settings = crate::network::settings::Settings {
                secret: Some("example".into()),
                ..Default::default()
            };
            let response = exchange_bytes(
                request::Config::unsafe_local(),
                settings,
                None,
                |port| {
                    format!("GET {route} HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: {token}\r\n\r\n")
                },
            );
            let (headers, body) = parts(&response);
            assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")));
            if route == "/metrics" && status == "200" {
                assert!(std::str::from_utf8(body)
                    .unwrap()
                    .contains("pixstride_requests_total"));
            }
        }
    }
}

#[test]
fn image_head_validators_cache_expiry_and_disposition_work_end_to_end() {
    let fixture = Fixture::new();
    let settings = || crate::network::settings::Settings {
        ttl: 3600,
        etag: true,
        last_modified: true,
        origin: Some("https://example.test".into()),
        ..Default::default()
    };
    let target = "/unsafe/fn:caf%C3%A9/att:1/plain/local:///sample.ppm@png";
    let get = exchange_bytes(
        request::Config::unsafe_local(),
        settings(),
        fixture.root(),
        |p| format!("GET {target} HTTP/1.1\r\nHost: localhost:{p}\r\n\r\n"),
    );
    let (headers, body) = parts(&get);
    assert!(headers.starts_with("HTTP/1.1 200 "), "{headers}");
    assert!(body.starts_with(b"\x89PNG"));
    assert!(headers.contains("Cache-Control: public, max-age=3600\r\n"));
    assert!(headers.contains("Access-Control-Allow-Origin: https://example.test\r\n"));
    assert!(headers.contains("attachment; filename*=UTF-8''caf%C3%A9.png\r\n"));
    let etag = field(headers, "ETag:");
    let modified = field(headers, "Last-Modified:");
    for (method, conditional, status) in [
        ("HEAD", String::new(), 200),
        ("GET", format!("If-None-Match: W/{etag}\r\n"), 304),
        ("GET", format!("If-Modified-Since: {modified}\r\n"), 304),
    ] {
        let result = exchange_bytes(
            request::Config::unsafe_local(),
            settings(),
            fixture.root(),
            |p| format!("{method} {target} HTTP/1.1\r\nHost: localhost:{p}\r\n{conditional}\r\n"),
        );
        let (h, b) = parts(&result);
        assert!(h.starts_with(&format!("HTTP/1.1 {status} ")), "{h}");
        assert!(b.is_empty());
        if method == "HEAD" {
            assert_eq!(field(h, "Content-Length:"), body.len().to_string());
        }
    }
    let expired = exchange(request::Config::unsafe_local(), |p| {
        format!(
            "GET /unsafe/exp:1/plain/local:///missing@png HTTP/1.1\r\nHost: localhost:{p}\r\n\r\n"
        )
    });
    assert!(expired.starts_with("HTTP/1.1 404 "));
}

#[test]
fn unix_streams_use_the_same_http_parser_and_response_deadlines() {
    let (mut server, mut client) = std::os::unix::net::UnixStream::pair().unwrap();
    let worker = std::thread::spawn(move || {
        let context = crate::protocol::request_with(&mut server, 8080, true, |target, context| {
            assert_eq!(target, "/health");
            context
        })
        .unwrap();
        crate::protocol::image_response(
            &mut server,
            200,
            "text/plain",
            "",
            b"ok\n",
            "no-store",
            context.head,
        )
        .unwrap();
    });
    client
        .write_all(b"HEAD /health HTTP/1.1\r\nHost: localhost:8080\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    worker.join().unwrap();
    let (headers, body) = parts(&response);
    assert!(headers.contains("Content-Length: 3\r\n"));
    assert!(body.is_empty());
}
