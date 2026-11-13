//! Selected HTTP headers kept only after the complete request passed validation.
use crate::{
    request::{OutputFormat, Request},
    Error,
};

#[derive(Default)]
pub(crate) struct Context {
    pub accept: String,
    pub authorization: String,
    pub if_none_match: String,
    pub if_modified_since: String,
    pub head: bool,
    pub preflight: bool,
    pub origin: String,
    pub requested_method: String,
    pub requested_headers: String,
    pub hints: [String; 4],
    pub traceparent: String,
}

impl Context {
    pub fn parse(header: &str) -> Result<Self, Error> {
        let mut result = Self {
            head: header.starts_with("HEAD "),
            preflight: header.starts_with("OPTIONS "),
            ..Default::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for line in header.split("\r\n").skip(1) {
            let (name, value) = line.split_once(':').ok_or("invalid header")?;
            let name = name.to_ascii_lowercase();
            let field = match name.as_str() {
                "sec-ch-dpr" => &mut result.hints[0],
                "dpr" => &mut result.hints[1],
                "sec-ch-width" => &mut result.hints[2],
                "width" => &mut result.hints[3],
                "traceparent" => &mut result.traceparent,
                "accept" => &mut result.accept,
                "authorization" => &mut result.authorization,
                "if-none-match" => &mut result.if_none_match,
                "if-modified-since" => &mut result.if_modified_since,
                "origin" => &mut result.origin,
                "access-control-request-method" => &mut result.requested_method,
                "access-control-request-headers" => &mut result.requested_headers,
                _ => continue,
            };
            if !seen.insert(name) {
                return Err("duplicate control header");
            }
            if value.len() > 8192 {
                return Err("control header exceeds limit");
            }
            *field = value.trim().into();
        }
        Ok(result)
    }

    pub fn authorize(&self, secret: Option<&str>) -> bool {
        let Some(secret) = secret else {
            return true;
        };
        let Some(token) = self.authorization.strip_prefix("Bearer ") else {
            return false;
        };
        // Token length is public; compare every byte of equal-length tokens.
        token.len() == secret.len()
            && token
                .bytes()
                .zip(secret.bytes())
                .fold(0, |diff, (a, b)| diff | (a ^ b))
                == 0
    }

    pub fn preflight_headers(&self, allowed: Option<&str>) -> Result<String, Error> {
        let origin = allowed
            .filter(|a| !self.origin.is_empty() && (*a == "*" || *a == self.origin))
            .ok_or("CORS origin is not allowed")?;
        if !matches!(self.requested_method.as_str(), "GET" | "HEAD")
            || self
                .requested_headers
                .split(',')
                .filter(|h| !h.trim().is_empty())
                .any(|h| {
                    ![
                        "authorization",
                        "if-none-match",
                        "if-modified-since",
                        "accept",
                    ]
                    .contains(&h.trim().to_ascii_lowercase().as_str())
                })
        {
            return Err("CORS method or headers are not allowed");
        }
        Ok(format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, HEAD, OPTIONS\r\nAccess-Control-Allow-Headers: Authorization, If-None-Match, If-Modified-Since, Accept\r\nAccess-Control-Max-Age: 3600\r\nVary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers\r\n"))
    }

    pub fn hint_defaults(&self) -> (Option<usize>, Option<f64>) {
        let dpr = self.hints[..2].iter().find_map(|v| {
            v.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.0 && *n <= 8.0)
        });
        let width = self.hints[2..].iter().find_map(|v| {
            v.parse::<usize>()
                .ok()
                .filter(|n| (1..=100_000).contains(n))
        });
        (width, dpr)
    }
    pub fn enforce(&self, r: &mut Request, formats: &[OutputFormat]) -> bool {
        r.extra.force_formats = formats
            .iter()
            .copied()
            .filter(|f| quality(&self.accept, f.mime()) > 0.0)
            .collect();
        r.extra.native |= !r.extra.force_formats.is_empty();
        !formats.is_empty()
    }
    pub fn negotiate(&self, r: &mut Request, formats: &[OutputFormat]) -> bool {
        r.extra.auto_formats = formats
            .iter()
            .copied()
            .filter(|f| quality(&self.accept, f.mime()) > 0.0)
            .collect();
        r.extra.native |= !r.extra.auto_formats.is_empty();
        !formats.is_empty()
    }
}

fn quality(accept: &str, mime: &str) -> f64 {
    let mut found = None;
    for part in accept.split(',') {
        let mut fields = part.trim().split(';');
        if !fields.next().unwrap_or("").eq_ignore_ascii_case(mime) {
            continue;
        }
        let mut q = 1.0f64;
        for field in fields {
            if let Some(v) = field.trim().strip_prefix("q=") {
                q = v
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .unwrap_or(0.0);
            }
        }
        found = Some(found.map_or(q, |old: f64| old.max(q)));
    }
    found.unwrap_or(0.0)
}

pub(crate) fn disposition(r: &Request, mime: &str) -> String {
    if r.extra.filename.is_none() && !r.extra.attachment {
        return String::new();
    }
    let name = r.extra.filename.as_deref().unwrap_or("image");
    let encoded = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    let extension = [
        OutputFormat::Png,
        OutputFormat::Jpeg,
        OutputFormat::Webp,
        OutputFormat::Gif,
        OutputFormat::Bmp,
        OutputFormat::Tiff,
        OutputFormat::Ico,
        OutputFormat::Avif,
        OutputFormat::Heic,
        OutputFormat::Jxl,
        OutputFormat::Svg,
    ]
    .into_iter()
    .find(|f| f.mime() == mime)
    .map_or("bin", |f| f.extension());
    format!(
        "Content-Disposition: {}; filename*=UTF-8''{encoded}.{extension}\r\n",
        if r.extra.attachment {
            "attachment"
        } else {
            "inline"
        }
    )
}

pub(crate) fn etag_with_buster(bytes: &[u8], buster: &str) -> String {
    let mut hash = crate::signature::Sha256::new();
    if !buster.is_empty() {
        hash.update(&(buster.len() as u64).to_be_bytes());
        hash.update(buster.as_bytes());
    }
    hash.update(bytes);
    format!(
        "\"{}\"",
        hash.finish()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

pub(crate) fn matches_etag(header: &str, etag: &str) -> bool {
    header
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_headers_auth_and_accept_are_unambiguous() {
        assert!(
            Context::parse("GET / HTTP/1.1\r\nAccept: image/webp\r\naccept: image/png").is_err()
        );
        let h=Context::parse("GET / HTTP/1.1\r\nAuthorization: Bearer secret\r\nAccept: image/webp;q=0,image/avif;q=0.5").unwrap();
        assert!(h.authorize(Some("secret")));
        assert!(!h.authorize(Some("secrex")));
        let mut r = crate::request::parse(
            "/unsafe/plain/local:///x",
            &crate::request::Config::unsafe_local(),
        )
        .unwrap();
        assert!(h.negotiate(&mut r, &[OutputFormat::Webp, OutputFormat::Avif]));
        assert_eq!(r.extra.auto_formats, vec![OutputFormat::Avif]);
        assert!(matches_etag("W/\"test\",\"other\"", "\"test\""));
    }
}
