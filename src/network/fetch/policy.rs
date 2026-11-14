//! Immutable destination policy, checked again against the connected address.
use crate::Error;
use std::{
    net::{IpAddr, Ipv6Addr},
    sync::{LazyLock, OnceLock},
};
static POLICY: OnceLock<Policy> = OnceLock::new();
static DEFAULT: LazyLock<Policy> = LazyLock::new(Policy::default);
pub(super) struct Policy {
    hosts: Vec<String>,
    ports: Vec<u16>,
    loopback: bool,
    private: bool,
    link_local: bool,
    pub timeout: u64,
    pub redirects: u32,
    pub user_agent: String,
    pub verify_tls: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            ports: vec![80, 443],
            loopback: false,
            private: false,
            link_local: false,
            timeout: 15,
            redirects: 4,
            user_agent: format!("pixstride/{}", env!("CARGO_PKG_VERSION")),
            verify_tls: true,
        }
    }
}
fn flag(name: &str) -> Result<bool, Error> {
    match std::env::var(name).as_deref() {
        Ok("1" | "true") => Ok(true),
        Ok("0" | "false") | Err(std::env::VarError::NotPresent) => Ok(false),
        _ => Err("invalid source policy flag"),
    }
}
impl Policy {
    fn from_env() -> Result<Self, Error> {
        let mut p = Self::default();
        let raw = std::env::var("PIXSTRIDE_ALLOWED_HOSTS").unwrap_or_default();
        if raw.len() > 8192 {
            return Err("too many allowed hosts");
        }
        if !raw.is_empty() {
            for host in raw.split(',') {
                let host = host.trim().to_ascii_lowercase();
                let host = host
                    .strip_prefix('[')
                    .and_then(|h| h.strip_suffix(']'))
                    .unwrap_or(&host);
                if host.parse::<IpAddr>().is_err()
                    && (host.is_empty()
                        || host.len() > 253
                        || host.split('.').any(|l| {
                            l.is_empty()
                                || l.len() > 63
                                || l.starts_with('-')
                                || l.ends_with('-')
                                || !l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        }))
                {
                    return Err("allowed hosts must be exact DNS names or IP addresses");
                }
                p.hosts.push(
                    host.parse::<IpAddr>()
                        .map_or_else(|_| host.to_owned(), |ip| ip.to_string()),
                );
            }
        }
        if let Ok(raw) = std::env::var("PIXSTRIDE_ALLOWED_SOURCE_PORTS") {
            if raw.len() > 256 {
                return Err("too many source ports");
            }
            p.ports = raw
                .split(',')
                .map(|v| {
                    v.trim()
                        .parse::<u16>()
                        .ok()
                        .filter(|v| *v != 0)
                        .ok_or("invalid allowed source port")
                })
                .collect::<Result<_, _>>()?;
            if p.ports.is_empty() || p.ports.len() > 32 {
                return Err("invalid allowed source ports");
            }
        }
        p.loopback = flag("PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES")?;
        p.private = flag("PIXSTRIDE_ALLOW_PRIVATE_SOURCE_ADDRESSES")?;
        p.link_local = flag("PIXSTRIDE_ALLOW_LINK_LOCAL_SOURCE_ADDRESSES")?;
        p.verify_tls = !flag("PIXSTRIDE_IGNORE_SSL_VERIFICATION")?;
        if let Ok(v) = std::env::var("PIXSTRIDE_DOWNLOAD_TIMEOUT") {
            p.timeout = v
                .parse()
                .ok()
                .filter(|v| (1..=120).contains(v))
                .ok_or("download timeout must be 1..120 seconds")?;
        }
        if let Ok(v) = std::env::var("PIXSTRIDE_MAX_REDIRECTS") {
            p.redirects = v
                .parse()
                .ok()
                .filter(|v| *v <= 16)
                .ok_or("max redirects must be 0..16")?;
        }
        if let Ok(v) = std::env::var("PIXSTRIDE_USER_AGENT") {
            let v = v.replace("%current_version", env!("CARGO_PKG_VERSION"));
            if v.is_empty() || v.len() > 512 || !v.bytes().all(|b| (32..127).contains(&b)) {
                return Err("invalid source User-Agent");
            }
            p.user_agent = v;
        }
        Ok(p)
    }
    fn ip(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => {
                super::public(ip)
                    || (self.loopback && ip.is_loopback())
                    || (self.private
                        && (ip.is_private()
                            || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))))
                    || (self.link_local && ip.is_link_local())
            }
            IpAddr::V6(ip) => {
                if let Some(ip) = ip.to_ipv4_mapped() {
                    return self.ip(IpAddr::V4(ip));
                }
                public_v6(ip)
                    || (self.loopback && ip.is_loopback())
                    || (self.private && ip.is_unique_local())
                    || (self.link_local && ip.is_unicast_link_local())
            }
        }
    }
}
fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // Conservatively admit global unicast, excluding IANA special, 6to4 and documentation ranges.
    s[0] & 0xe000 == 0x2000
        && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
        && s[0] != 0x2002
        && !(s[0] == 0x3fff && s[1] < 0x1000)
}
pub fn initialize() -> Result<(), Error> {
    POLICY
        .set(Policy::from_env()?)
        .map_err(|_| "destination policy already initialized")
}
pub(super) fn settings() -> &'static Policy {
    POLICY.get().unwrap_or(&DEFAULT)
}
pub(super) fn allows(host: &str) -> bool {
    settings().hosts.iter().any(|h| h == host)
}
pub(crate) fn allows_ip(ip: IpAddr) -> bool {
    settings().ip(ip)
}
pub(crate) fn allows_port(port: u16) -> bool {
    settings().ports.contains(&port)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn address_classes_require_independent_explicit_opt_ins() {
        let mut p = Policy::default();
        for ip in ["8.8.8.8", "2606:4700:4700::1111", "2001:4860:4860::8888"] {
            assert!(p.ip(ip.parse().unwrap()));
        }
        for ip in [
            "::",
            "::1",
            "fe80::1",
            "fc00::1",
            "ff02::1",
            "2001:db8::1",
            "2002:7f00:1::",
            "3fff::1",
            "::ffff:127.0.0.1",
            "10.1.1.1",
        ] {
            assert!(!p.ip(ip.parse().unwrap()), "{ip}");
        }
        p.loopback = true;
        for ip in ["::1", "127.0.0.1", "::ffff:127.0.0.1"] {
            assert!(p.ip(ip.parse().unwrap()));
        }
        assert!(!p.ip("fe80::1".parse().unwrap()));
        p.private = true;
        for ip in ["fc00::1", "192.168.1.1", "100.64.0.1"] {
            assert!(p.ip(ip.parse().unwrap()));
        }
        p.link_local = true;
        assert!(p.ip("fe80::1".parse().unwrap()));
        assert!(!p.ip("2001:db8::1".parse().unwrap()));
    }
}
