use super::handle;
use crate::{fetch, request, signature, watermark};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{Mutex, OnceLock},
    time::Duration,
};

fn exchange(config: request::Config, build_request: impl FnOnce(u16) -> String) -> String {
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
            &None,
            &config,
            &watermark,
            pool,
            &Mutex::new(()),
        );
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    client.write_all(request.as_bytes()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
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
