use crate::Result;
use std::net::Ipv4Addr;
mod policy;
mod transport;
pub use policy::initialize;
pub use transport::LazyPool as Pool;
#[derive(Debug)]
pub struct Url {
    pub full: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
}
impl Url {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.len() > 12000 || raw.bytes().any(|b| b <= 32 || b >= 127 || b == b'\\') {
            return Err(
                "An ASCII HTTP(S) URL without whitespace or control characters is required".into(),
            );
        }
        if raw.contains('#') {
            return Err("URL fragments are unsupported".into());
        }
        let (scheme, tail) = raw
            .split_once("://")
            .ok_or("An absolute HTTP(S) URL is required")?;
        if !matches!(scheme, "http" | "https") {
            return Err("Only HTTP and HTTPS are supported".into());
        }
        let end = tail.find(['/', '?']).unwrap_or(tail.len());
        let authority = &tail[..end];
        let (host, port) = match authority.split_once(':') {
            Some((h, p)) => (h, p.parse().map_err(|_| "Invalid port")?),
            None => (authority, if scheme == "https" { 443 } else { 80 }),
        };
        if host.is_empty()
            || host.len() > 253
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
            || !matches!(port, 80 | 443)
        {
            return Err("Public host required, without credentials, on port 80 or 443".into());
        }
        let path = match &tail[end..] {
            "" => "/".to_string(),
            p if p.starts_with('?') => format!("/{p}"),
            p => p.to_string(),
        };
        Ok(Self {
            full: format!("{scheme}://{authority}{path}"),
            scheme: scheme.into(),
            host: host.to_ascii_lowercase(),
            port,
            path,
        })
    }
    fn redirect(&self, location: &str) -> Result<Self> {
        if location.contains("://") {
            return Self::parse(location);
        }
        let authority = format!("{}://{}:{}", self.scheme, self.host, self.port);
        let next = if location.starts_with("//") {
            format!("{}:{location}", self.scheme)
        } else if location.starts_with('/') {
            format!("{authority}{location}")
        } else if location.starts_with('?') {
            format!(
                "{authority}{}{location}",
                self.path.split('?').next().unwrap()
            )
        } else {
            format!(
                "{authority}{}/{location}",
                self.path
                    .split('?')
                    .next()
                    .unwrap()
                    .rsplit_once('/')
                    .unwrap()
                    .0
            )
        };
        Self::parse(&next)
    }
}
pub(super) fn public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || ip.is_documentation())
}
