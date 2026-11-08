use super::{parse_header, text};
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

#[test]
fn valid_http_header_yields_only_its_origin_form_target() {
    let header = b"GET /unsafe/plain/https://example.test/a.png HTTP/1.1\r\nHost: example.test\r\nAccept: image/png";
    assert_eq!(
        parse_header(header, 8080, false).unwrap(),
        "/unsafe/plain/https://example.test/a.png"
    );
}

#[test]
fn duplicate_host_and_body_framing_are_rejected() {
    let invalid: &[&[u8]] = &[
        b"GET / HTTP/1.1\r\nHost: example.test\r\nHost: example.test",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nhOsT: example.test",
        b"GET / HTTP/1.1\r\nHost: example.test:80\r\nhost: example.test:443",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 0",
        b"GET / HTTP/1.1\r\nHost: example.test\r\ncontent-length: 1",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nTransfer-Encoding: chunked",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 1\r\nTransfer-Encoding: chunked",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 1\r\nContent-Length: 1",
    ];
    for input in invalid {
        assert!(
            parse_header(input, 8080, false).is_err(),
            "accepted {input:?}"
        );
    }
}

#[test]
fn malformed_request_line_headers_and_host_are_rejected() {
    let invalid: &[&[u8]] = &[
        b"GET / HTTP/1.1",
        b"POST / HTTP/1.1\r\nHost: example.test",
        b"GET http://example.test/ HTTP/1.1\r\nHost: example.test",
        b"GET //example.test/ HTTP/1.1\r\nHost: example.test",
        b"GET / HTTP/1.0\r\nHost: example.test",
        b"GET / HTTP/1.1\r\nHost:",
        b"GET / HTTP/1.1\r\nHost : example.test",
        b"GET / HTTP/1.1\r\nHost: example.test\r",
        b"GET / HTTP/1.1\r\nHost: example.test\n",
        b"GET / HTTP/1.1\r\nHost: example.test\r\n Missing: value",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nMissing-Separator",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nX-Test: bad\x00value",
        b"GET / HTTP/1.1\r\nHost: example.test\r\nX-Test: \xff",
    ];
    for input in invalid {
        assert!(
            parse_header(input, 8080, false).is_err(),
            "accepted {input:?}"
        );
    }
    let oversized_host = format!("GET / HTTP/1.1\r\nHost: {}", "a".repeat(256));
    assert!(parse_header(oversized_host.as_bytes(), 8080, false).is_err());
}

#[test]
fn unsigned_hosts_require_an_exact_loopback_name_and_expected_port() {
    let accepted = [
        "localhost:8080",
        "LOCALHOST:8080",
        "127.0.0.1:8080",
        "[::1]:8080",
        "localhost:08080",
    ];
    let rejected = [
        "example.test:8080",
        "localhost",
        "localhost:80",
        "localhost:",
        "localhost:+8080",
        "localhost:8080:80",
        "localhost:0",
        "localhost:65536",
        "localhost:999999999999999999999",
        "localhost.:8080",
        "localhost.example.test:8080",
        "localhost@evil.test:8080",
        "127.0.0.2:8080",
        "127.1:8080",
        "127.0.0.1.example.test:8080",
        "::1:8080",
        "[::1]extra:8080",
        "[::1]:8080:80",
        "[::2]:8080",
        "[::1",
    ];
    for host in accepted {
        let input = format!("GET /health HTTP/1.1\r\nHost: {host}");
        assert!(
            parse_header(input.as_bytes(), 8080, true).is_ok(),
            "rejected {host}"
        );
    }
    for host in rejected {
        let input = format!("GET /health HTTP/1.1\r\nHost: {host}");
        assert!(
            parse_header(input.as_bytes(), 8080, true).is_err(),
            "accepted {host}"
        );
    }
}

#[test]
fn omitted_unsigned_port_means_http_port_80() {
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let input = format!("GET /health HTTP/1.1\r\nHost: {host}");
        assert!(parse_header(input.as_bytes(), 80, true).is_ok());
        assert!(parse_header(input.as_bytes(), 8080, true).is_err());
    }
    assert!(parse_header(b"GET / HTTP/1.1\r\nHost: localhost:0", 0, true).is_err());
}

fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

#[test]
fn short_write_timeout_is_preserved_for_complete_overload_response() {
    let (mut client, mut server) = socket_pair();
    server
        .set_write_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    text(&mut server, 503, "busy").unwrap();
    let applied = server.write_timeout().unwrap().unwrap();
    assert!(
        applied <= Duration::from_millis(300),
        "timeout was {applied:?}"
    );
    drop(server);
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503 "));
    assert!(response.contains("Content-Length: 4\r\n"));
    assert!(response.ends_with("\r\n\r\nbusy"));
}

#[test]
fn normal_responses_keep_the_existing_fifteen_second_deadline() {
    let (_client, mut server) = socket_pair();
    text(&mut server, 200, "ok").unwrap();
    let applied = server.write_timeout().unwrap().unwrap();
    assert!(applied > Duration::from_secs(14));
    assert!(applied <= Duration::from_secs(15));
}

#[test]
fn blocked_response_respects_the_shorter_total_deadline() {
    let (_client, mut server) = socket_pair();
    server.set_nonblocking(true).unwrap();
    let data = [0; 16 * 1024];
    let mut written = 0;
    let mut previously_blocked = false;
    loop {
        match server.write(&data) {
            Ok(0) => panic!("socket closed before backpressure"),
            Ok(n) => {
                written += n;
                previously_blocked = false;
                assert!(written < 64 * 1024 * 1024, "socket never blocked");
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if previously_blocked {
                    break;
                }
                previously_blocked = true;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("failed to fill socket: {error}"),
        }
    }
    server.set_nonblocking(false).unwrap();
    server
        .set_write_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let started = Instant::now();
    assert!(text(&mut server, 503, "busy").is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}
