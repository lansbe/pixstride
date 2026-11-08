//! Exact destination names, independent of connection-time IP filtering.
use std::sync::OnceLock;
static HOSTS: OnceLock<Vec<String>> = OnceLock::new();
pub fn initialize() -> Result<(), &'static str> {
    let raw = std::env::var("PIXSTRIDE_ALLOWED_HOSTS").unwrap_or_default();
    if raw.len() > 8192 {
        return Err("too many allowed hosts");
    }
    let mut hosts = Vec::new();
    if !raw.is_empty() {
        for host in raw.split(',') {
            let host = host.trim().to_ascii_lowercase();
            if host.is_empty()
                || host.len() > 253
                || host.split('.').any(|l| {
                    l.is_empty()
                        || l.len() > 63
                        || l.starts_with('-')
                        || l.ends_with('-')
                        || !l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err("allowed hosts must be exact DNS names or IPv4 addresses");
            }
            hosts.push(host);
        }
    }
    HOSTS
        .set(hosts)
        .map_err(|_| "destination policy already initialized")
}
pub(super) fn allows(host: &str) -> bool {
    HOSTS
        .get()
        .is_some_and(|hosts| hosts.iter().any(|allowed| allowed == host))
}
