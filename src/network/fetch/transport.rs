//! System libcurl is transport only. Handles keep connections, never response bodies.
use super::response::Response;
use super::{policy, Url};
use crate::Result;
use std::{
    ffi::{c_char, c_int, c_long, c_uint, c_void, CString},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Condvar, Mutex, OnceLock},
};
#[link(name = "curl")]
unsafe extern "C" {
    fn curl_global_init(flags: c_long) -> c_int;
    fn curl_version_info(age: c_int) -> *const VersionPrefix;
    fn curl_easy_init() -> *mut c_void;
    fn curl_easy_cleanup(handle: *mut c_void);
    fn curl_easy_reset(handle: *mut c_void);
    fn curl_easy_setopt(handle: *mut c_void, option: c_int, ...) -> c_int;
    fn curl_easy_getinfo(handle: *mut c_void, info: c_int, ...) -> c_int;
    fn curl_easy_perform(handle: *mut c_void) -> c_int;
}
// Stable prefix of curl_version_info_data, requesting CURLVERSION_FIRST.
#[repr(C)]
pub(crate) struct VersionPrefix {
    age: c_int,
    version: *const c_char,
    version_num: c_uint,
    host: *const c_char,
    features: c_int,
}

pub struct LazyPool(OnceLock<Result<Pool>>);
impl LazyPool {
    /// Called on the main thread before workers exist. Older libcurl builds
    /// without thread-safe global initialization retain eager initialization.
    pub fn new() -> Result<Self> {
        let version = unsafe { curl_version_info(0) };
        if version.is_null() {
            return Err("HTTPS version unavailable".into());
        }
        let pool = Self(OnceLock::new());
        let thread_safe = unsafe { (*version).features } & (1 << 30) != 0;
        if !thread_safe {
            let eager = Pool::new()?;
            let _ = pool.0.set(Ok(eager));
        }
        Ok(pool)
    }
    pub fn fetch(&self, url: &str, max: usize) -> Result<Download> {
        self.fetch_with_cookies(url, max, None)
    }
    pub fn fetch_with_cookies(
        &self,
        url: &str,
        max: usize,
        jar: Option<&mut crate::network::cookies::Jar>,
    ) -> Result<Download> {
        match self.0.get_or_init(Pool::new) {
            Ok(pool) => pool.fetch(url, max, jar),
            Err(error) => Err(error.clone()),
        }
    }
}
unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
}
#[repr(C)]
pub(crate) struct Address {
    family: c_int,
    kind: c_int,
    protocol: c_int,
    length: c_uint,
    address: [u8; 16],
}
pub(crate) struct Sink {
    body: Vec<u8>,
    headers: Vec<u8>,
    max: usize,
}
unsafe extern "C" fn write_body(p: *mut c_char, size: usize, n: usize, user: *mut c_void) -> usize {
    let Some(len) = size.checked_mul(n) else {
        return 0;
    };
    if len == 0 || p.is_null() || user.is_null() {
        return 0;
    }
    let sink = unsafe { &mut *(user as *mut Sink) };
    if len > sink.max.saturating_sub(sink.body.len()) || sink.body.try_reserve(len).is_err() {
        return 0;
    }
    sink.body
        .extend_from_slice(unsafe { std::slice::from_raw_parts(p as *const u8, len) });
    len
}
unsafe extern "C" fn write_header(
    p: *mut c_char,
    size: usize,
    n: usize,
    user: *mut c_void,
) -> usize {
    let Some(len) = size.checked_mul(n) else {
        return 0;
    };
    if len == 0 || p.is_null() || user.is_null() {
        return 0;
    }
    let sink = unsafe { &mut *(user as *mut Sink) };
    if len > 65536usize.saturating_sub(sink.headers.len()) || sink.headers.try_reserve(len).is_err()
    {
        return 0;
    }
    sink.headers
        .extend_from_slice(unsafe { std::slice::from_raw_parts(p as *const u8, len) });
    len
}
fn valid_family(address: &[u8; 16]) -> bool {
    #[cfg(target_os = "macos")]
    {
        address[0] == 16 && address[1] == 2
    }
    #[cfg(target_os = "linux")]
    {
        u16::from_ne_bytes([address[0], address[1]]) == 2
    }
}
unsafe extern "C" fn open_socket(_: *mut c_void, purpose: c_int, address: *mut Address) -> c_int {
    if address.is_null() || purpose != 0 {
        return -1;
    }
    let a = unsafe { &*address };
    // Linux curl may request atomic close-on-exec/nonblocking socket flags.
    #[cfg(target_os = "linux")]
    let socket_flags = 0x80000 | 0x800;
    #[cfg(target_os = "macos")]
    let socket_flags = 0;
    if a.kind & !socket_flags != 1 || !matches!(a.protocol, 0 | 6) {
        return -1;
    }
    if !policy::allows_port(u16::from_be_bytes([a.address[2], a.address[3]]))
        || !unsafe { destination(address) }.is_some_and(policy::allows_ip)
    {
        return -1;
    }
    unsafe { socket(a.family, a.kind, a.protocol) }
}
// libcurl's opensocket ABI supplies addrlen bytes at addr, extending the
// nominal sockaddr field for IPv6. Validate its family/size before reading it.
unsafe fn destination(address: *const Address) -> Option<IpAddr> {
    let a = unsafe { &*address };
    if a.family == 2 && a.length == 16 && valid_family(&a.address) {
        return Some(Ipv4Addr::new(a.address[4], a.address[5], a.address[6], a.address[7]).into());
    }
    #[cfg(target_os = "linux")]
    let ipv6 = 10;
    #[cfg(target_os = "macos")]
    let ipv6 = 30;
    if a.family != ipv6 || a.length != 28 {
        return None;
    }
    #[cfg(target_os = "linux")]
    let valid = u16::from_ne_bytes([a.address[0], a.address[1]]) == ipv6 as u16;
    #[cfg(target_os = "macos")]
    let valid = a.address[0] == 28 && a.address[1] == ipv6 as u8;
    if !valid {
        return None;
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::addr_of!((*address).address).cast::<u8>(), 28)
    };
    Some(Ipv6Addr::from(<[u8; 16]>::try_from(&bytes[8..24]).ok()?).into())
}
pub(crate) struct Client(*mut c_void);
// Each easy handle has one exclusive Lease. It may move threads, never run concurrently.
unsafe impl Send for Client {}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe { curl_easy_cleanup(self.0) }
    }
}
pub(crate) struct Reset(*mut c_void);
impl Drop for Reset {
    fn drop(&mut self) {
        unsafe { curl_easy_reset(self.0) }
    }
}
pub struct Download {
    pub body: Vec<u8>,
    pub new_connections: u64,
    pub upstream_bytes: u64,
    pub transfers: u32,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
    pub expires: Option<String>,
}
pub(crate) struct Transfer {
    body: Vec<u8>,
    response: Response,
    status: c_long,
    new_connections: u64,
    bytes: u64,
}
impl Client {
    fn long(&self, option: c_int, value: c_long) -> Result<()> {
        check(unsafe { curl_easy_setopt(self.0, option, value) })
    }
    fn info(&self, kind: c_int) -> Result<c_long> {
        let mut out: c_long = 0;
        check(unsafe { curl_easy_getinfo(self.0, kind, &mut out) })?;
        Ok(out)
    }
    fn one(
        &mut self,
        url: &Url,
        max: usize,
        remaining: std::time::Duration,
        cookie: Option<&str>,
        capture_cookies: bool,
    ) -> Result<Transfer> {
        if !policy::allows(&url.host) || !policy::allows_port(url.port) {
            return Err("source host is not allowed".into());
        }
        if remaining.is_zero() {
            return Err("upstream deadline exceeded".into());
        }
        let target = CString::new(url.full.as_str()).map_err(|_| "Invalid URL")?;
        let cookie = cookie
            .map(CString::new)
            .transpose()
            .map_err(|_| "invalid source cookie")?;
        let agent = CString::new(policy::settings().user_agent.as_str()).unwrap();
        let mut sink = Sink {
            body: Vec::new(),
            headers: Vec::new(),
            max,
        };
        // reset preserves connection/DNS/TLS-session caches, but clears borrowed callbacks.
        unsafe { curl_easy_reset(self.0) };
        let reset = Reset(self.0);
        self.long(99, 1)?;
        self.long(155, remaining.as_millis().clamp(1, 120000) as c_long)?;
        self.long(156, 5000)?;
        self.long(181, 3)?;
        self.long(52, 0)?;
        self.long(182, 3)?; // HTTP(S) protocols only.
        self.long(64, c_long::from(policy::settings().verify_tls))?;
        self.long(81, if policy::settings().verify_tls { 2 } else { 0 })?;
        self.long(113, 0)?; // Both IP families; the socket callback enforces policy.
        self.long(71, 4)?;
        self.long(84, 4)?;
        self.long(213, 1)?;
        check(unsafe { curl_easy_setopt(self.0, 10004, c"".as_ptr()) })?;
        check(unsafe { curl_easy_setopt(self.0, 10002, target.as_ptr()) })?;
        check(unsafe { curl_easy_setopt(self.0, 10018, agent.as_ptr()) })?;
        check(unsafe { curl_easy_setopt(self.0, 10102, c"gzip, deflate".as_ptr()) })?;
        if let Some(cookie) = &cookie {
            // CURLOPT_COOKIE borrows this per-transfer string until Reset clears it.
            // No libcurl cookie engine or shared cookie jar is enabled.
            check(unsafe { curl_easy_setopt(self.0, 10022, cookie.as_ptr()) })?;
        }
        check(unsafe { curl_easy_setopt(self.0, 30117, max as i64) })?;
        check(unsafe {
            curl_easy_setopt(
                self.0,
                20011,
                write_body as unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize,
            )
        })?;
        check(unsafe {
            curl_easy_setopt(
                self.0,
                20079,
                write_header
                    as unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize,
            )
        })?;
        let userdata = std::ptr::addr_of_mut!(sink).cast::<c_void>();
        check(unsafe { curl_easy_setopt(self.0, 10001, userdata) })?;
        check(unsafe { curl_easy_setopt(self.0, 10029, userdata) })?;
        check(unsafe {
            curl_easy_setopt(
                self.0,
                20163,
                open_socket as unsafe extern "C" fn(*mut c_void, c_int, *mut Address) -> c_int,
            )
        })?;
        check(unsafe { curl_easy_perform(self.0) })?;
        let status = self.info(0x200002)?;
        let connects = self.info(0x20001a)?;
        let mut bytes: i64 = 0;
        check(unsafe { curl_easy_getinfo(self.0, 0x600008, &mut bytes) })?;
        if bytes < 0 || connects < 0 {
            return Err("Inconsistent transfer accounting".into());
        }
        let response = Response::parse(&sink.headers, capture_cookies)?;
        // Clear callback pointers while the complete Sink is still in scope.
        drop(reset);
        Ok(Transfer {
            body: sink.body,
            response,
            status,
            new_connections: connects as u64,
            bytes: bytes as u64,
        })
    }
    fn fetch(
        &mut self,
        raw: &str,
        max: usize,
        mut jar: Option<&mut crate::network::cookies::Jar>,
    ) -> Result<Download> {
        let policy = policy::settings();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(policy.timeout);
        let mut url = Url::parse(raw)?;
        let mut connects = 0;
        let mut bytes = 0;
        for hop in 0..=policy.redirects {
            let cookie = jar
                .as_ref()
                .map(|j| j.header(&url.full))
                .transpose()
                .map_err(str::to_owned)?
                .flatten();
            let r = self.one(
                &url,
                max,
                deadline.saturating_duration_since(std::time::Instant::now()),
                cookie.as_deref(),
                jar.is_some(),
            )?;
            if let Some(jar) = jar.as_mut() {
                for value in &r.response.cookies {
                    jar.response(&url.full, value).map_err(str::to_owned)?;
                }
            }
            connects += r.new_connections;
            bytes += r.bytes;
            if r.status == 200 {
                return Ok(Download {
                    body: r.body,
                    new_connections: connects,
                    upstream_bytes: bytes,
                    transfers: hop + 1,
                    last_modified: r.response.last_modified,
                    cache_control: r.response.cache_control,
                    expires: r.response.expires,
                });
            }
            if matches!(r.status, 301 | 302 | 303 | 307 | 308) && hop < policy.redirects {
                let next = url.redirect(
                    r.response
                        .location
                        .as_deref()
                        .ok_or("redirect has no location")?,
                )?;
                if url.scheme == "https" && next.scheme != "https" {
                    return Err("HTTPS downgrade refused".into());
                }
                url = next;
                continue;
            }
            return Err(format!(
                "Upstream returned HTTP {} or too many redirects",
                r.status
            ));
        }
        Err("Too many redirects".into())
    }
}
fn check(code: c_int) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("HTTP(S) transport failed (libcurl {code})"))
    }
}
pub struct Pool {
    available: Mutex<Vec<Client>>,
    ready: Condvar,
}
pub(crate) struct Lease<'a> {
    pool: &'a Pool,
    client: Option<Client>,
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.client.take() {
            self.pool
                .available
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(c);
            self.pool.ready.notify_one();
        }
    }
}
impl Pool {
    pub fn new() -> Result<Self> {
        // LazyPool serializes this initialization and only defers it when
        // libcurl explicitly advertises thread-safe global initialization.
        check(unsafe { curl_global_init(3) })?;
        let mut clients = Vec::new();
        for _ in 0..4 {
            let h = unsafe { curl_easy_init() };
            if h.is_null() {
                return Err("HTTPS initialization failed".into());
            }
            clients.push(Client(h));
        }
        Ok(Self {
            available: Mutex::new(clients),
            ready: Condvar::new(),
        })
    }
    pub fn fetch(
        &self,
        url: &str,
        max: usize,
        jar: Option<&mut crate::network::cookies::Jar>,
    ) -> Result<Download> {
        let mut available = self.available.lock().map_err(|_| "Transport unavailable")?;
        while available.is_empty() {
            available = self
                .ready
                .wait(available)
                .map_err(|_| "Transport unavailable")?;
        }
        let mut lease = Lease {
            pool: self,
            client: available.pop(),
        };
        drop(available);
        lease.client.as_mut().unwrap().fetch(url, max, jar)
    }
}

#[cfg(test)]
#[path = "transport_regressions.rs"]
mod security_regressions;
