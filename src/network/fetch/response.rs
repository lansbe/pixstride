//! Only bounded selected origin metadata survives the transfer.
use crate::Result;
#[derive(Default)]
pub(super) struct Response {
    pub location: Option<String>,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
    pub expires: Option<String>,
    pub cookies: Vec<String>,
}
impl Response {
    pub fn parse(headers: &[u8], capture_cookies: bool) -> Result<Self> {
        let mut result = Self::default();
        for line in headers.split(|b| *b == b'\n') {
            if line.starts_with(b"HTTP/") {
                result = Self::default();
            }
            let Some(colon) = line.iter().position(|b| *b == b':') else {
                continue;
            };
            let name = &line[..colon];
            let cookie = capture_cookies && name.eq_ignore_ascii_case(b"set-cookie");
            if !cookie
                && ![
                    b"location".as_slice(),
                    b"last-modified",
                    b"cache-control",
                    b"expires",
                ]
                .iter()
                .any(|v| name.eq_ignore_ascii_case(v))
            {
                continue;
            }
            let value = std::str::from_utf8(&line[colon + 1..])
                .map_err(|_| "invalid origin metadata")?
                .trim();
            let max = if name.eq_ignore_ascii_case(b"location") {
                12000
            } else if cookie {
                4096
            } else {
                2048
            };
            if value.len() > max || !value.bytes().all(|b| (32..127).contains(&b)) {
                return Err("origin metadata exceeds limits".into());
            }
            if cookie {
                if result.cookies.len() >= 64 {
                    return Err("too many source cookies".into());
                }
                result.cookies.push(value.to_owned());
                continue;
            }
            let field = if name.eq_ignore_ascii_case(b"location") {
                &mut result.location
            } else if name.eq_ignore_ascii_case(b"last-modified") {
                &mut result.last_modified
            } else if name.eq_ignore_ascii_case(b"expires") {
                &mut result.expires
            } else {
                &mut result.cache_control
            };
            if name.eq_ignore_ascii_case(b"cache-control") {
                let combined = field.get_or_insert_with(String::new);
                if !combined.is_empty() {
                    combined.push_str(", ");
                }
                combined.push_str(value);
                if combined.len() > 2048 {
                    return Err("origin cache policy exceeds limit".into());
                }
            } else {
                *field = Some(value.to_owned());
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interim_headers_cannot_supply_final_cookies_or_cache_policy() {
        let r = Response::parse(b"HTTP/1.1 103 Early Hints\r\nSet-Cookie: early=secret\r\nCache-Control: public\r\nHTTP/1.1 200 OK\r\nCache-Control: private\r\nCache-Control: max-age=12\r\n", true).unwrap();
        assert!(r.cookies.is_empty());
        assert_eq!(r.cache_control.as_deref(), Some("private, max-age=12"));
        assert!(Response::parse(b"Cache-Control: public\x00\r\n", false).is_err());
    }
}
