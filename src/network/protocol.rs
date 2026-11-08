//! Bounded local HTTP/1.1 parser and complete-response writer.
use crate::Result;
use std::{
    fmt,
    io::{self, IoSlice, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};
// Invoke the consumer only after the complete header has passed validation.
// Its borrowed target lives in the stack buffer or fragmented-header buffer;
// the consumer's result must own anything it needs after this function returns.
pub fn request_with<T>(
    stream: &mut TcpStream,
    port: u16,
    local_host_only: bool,
    consume: impl FnOnce(&str) -> T,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(5);
    // A complete first read is parsed directly from the stack. Fragmented
    // headers use a bounded Vec and only scan bytes not already inspected.
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 2048];
    let mut scan_from = 0usize;
    loop {
        if Instant::now() >= deadline {
            return Err("Header deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            ))
            .map_err(|e| e.to_string())?;
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Incomplete request".into());
        }
        if bytes.is_empty() {
            if let Some(end) = chunk[..n].windows(4).position(|w| w == b"\r\n\r\n") {
                return parse_header(&chunk[..end], port, local_host_only).map(consume);
            }
        }
        if n > 16384usize.saturating_sub(bytes.len()) {
            return Err("Header too large".into());
        }
        bytes
            .try_reserve(n)
            .map_err(|_| "Header allocation failed".to_string())?;
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(at) = bytes[scan_from..].windows(4).position(|w| w == b"\r\n\r\n") {
            return parse_header(&bytes[..scan_from + at], port, local_host_only).map(consume);
        }
        // Keep the last three bytes so delimiters fragmented across reads are
        // detected without rescanning the earlier, already inspected prefix.
        scan_from = bytes.len().saturating_sub(3);
    }
}

fn local_host_matches(value: &str, port: u16) -> bool {
    let host_end = if value.starts_with('[') {
        match value.find(']') {
            Some(index) => index + 1,
            None => return false,
        }
    } else {
        value.find(':').unwrap_or(value.len())
    };
    let (host, suffix) = value.split_at(host_end);
    if !host.eq_ignore_ascii_case("localhost") && host != "127.0.0.1" && host != "[::1]" {
        return false;
    }
    if suffix.is_empty() {
        return port == 80;
    }
    let Some(port_text) = suffix.strip_prefix(':') else {
        return false;
    };
    !port_text.is_empty()
        && port_text.bytes().all(|b| b.is_ascii_digit())
        && port != 0
        && port_text.parse::<u16>() == Ok(port)
}

fn parse_header(bytes: &[u8], port: u16, local_host_only: bool) -> Result<&str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid header encoding")?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap().split(' ');
    if first.next() != Some("GET") {
        return Err("GET required".into());
    }
    let path = first.next().ok_or("Missing target")?;
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("HTTP/1.1 required".into());
    }
    if !path.starts_with('/') || path.starts_with("//") || path.bytes().any(|b| b <= 32 || b >= 127)
    {
        return Err("invalid request target".into());
    }
    let mut host = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("Invalid header")?;
        let value = value.trim_matches([' ', '\t']);
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.bytes().any(|b| b < 32 || b == 127)
        {
            return Err("invalid header".into());
        }
        if name.eq_ignore_ascii_case("host") {
            if host
                || value.is_empty()
                || value.len() > 255
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
                || (local_host_only && !local_host_matches(value, port))
            {
                return Err("invalid host".into());
            }
            host = true;
        } else if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            return Err("request bodies are unsupported".into());
        }
    }

    if !host {
        return Err("Host required".into());
    }
    Ok(path)
}
pub(crate) struct HeaderBuffer {
    bytes: [u8; 1024],
    used: usize,
}
impl std::fmt::Write for HeaderBuffer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.used.checked_add(text.len()).ok_or(std::fmt::Error)?;
        let dst = self.bytes.get_mut(self.used..end).ok_or(std::fmt::Error)?;
        dst.copy_from_slice(text.as_bytes());
        self.used = end;
        Ok(())
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        503 => "Service Unavailable",
        _ => "Bad Gateway",
    }
}
fn formatted_header(
    status: u16,
    mime: &str,
    len: u64,
    extra: fmt::Arguments<'_>,
) -> Option<HeaderBuffer> {
    let reason = reason(status);
    let mut header = HeaderBuffer {
        bytes: [0; 1024],
        used: 0,
    };
    let formatted = std::fmt::write(&mut header, format_args!("HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {len}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n{extra}\r\n"));
    if formatted.is_ok() {
        Some(header)
    } else {
        None
    }
}
fn long_header_to<W: Write>(
    s: &mut W,
    status: u16,
    mime: &str,
    len: u64,
    extra: fmt::Arguments<'_>,
) -> io::Result<()> {
    let reason = reason(status);
    // Preserve the existing fallback for internal values exceeding 1024 bytes.
    write!(s,"HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {len}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n{extra}\r\n")
}

fn write_all_pair<W: Write>(s: &mut W, mut head: &[u8], mut body: &[u8]) -> io::Result<()> {
    while !head.is_empty() && !body.is_empty() {
        match s.write_vectored(&[IoSlice::new(head), IoSlice::new(body)]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write response",
                ))
            }
            Ok(n) if n <= head.len() => head = &head[n..],
            Ok(n) => {
                let in_body = n - head.len();
                head = &[];
                body = &body[in_body..];
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    s.write_all(head)?;
    s.write_all(body)
}
fn response_to<W: Write>(
    s: &mut W,
    status: u16,
    mime: &str,
    extra: fmt::Arguments<'_>,
    body: &[u8],
) -> io::Result<()> {
    let len = body.len() as u64;
    if let Some(header) = formatted_header(status, mime, len, extra) {
        write_all_pair(s, &header.bytes[..header.used], body)
    } else {
        long_header_to(s, status, mime, len, extra)?;
        s.write_all(body)
    }
}
pub(crate) struct DeadlineWriter<'a> {
    socket: &'a mut TcpStream,
    deadline: Instant,
}
impl DeadlineWriter<'_> {
    fn timeout(&self) -> io::Result<()> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response deadline exceeded",
            ));
        }
        self.socket.set_write_timeout(Some(remaining))
    }
}
impl Write for DeadlineWriter<'_> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.timeout()?;
        self.socket.write(b)
    }
    fn write_vectored(&mut self, b: &[IoSlice<'_>]) -> io::Result<usize> {
        self.timeout()?;
        self.socket.write_vectored(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
pub fn response(
    s: &mut TcpStream,
    status: u16,
    mime: &str,
    extra: fmt::Arguments<'_>,
    body: &[u8],
) -> io::Result<()> {
    // The overload path sets a shorter timeout before writing its 503 reply.
    // Avoid querying socket options on the normal response path.
    let response_timeout = Duration::from_secs(15);
    let timeout = if status == 503 {
        s.write_timeout()?
            .unwrap_or(response_timeout)
            .min(response_timeout)
    } else {
        response_timeout
    };
    let mut writer = DeadlineWriter {
        socket: s,
        deadline: Instant::now() + timeout,
    };
    response_to(&mut writer, status, mime, extra, body)
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod protocol_tests;

pub fn text(s: &mut TcpStream, status: u16, body: &str) -> io::Result<()> {
    response(
        s,
        status,
        "text/plain; charset=utf-8",
        format_args!(""),
        body.as_bytes(),
    )
}
