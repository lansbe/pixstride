//! Opt-in cookies live for one source request and its bounded redirect chain.
use crate::Error;
#[derive(Default)]
pub(crate) struct Policy {
    enabled: bool,
    all: bool,
    base: Option<String>,
}
fn flag(name: &str) -> Result<bool, Error> {
    match std::env::var(name).as_deref() {
        Ok("1" | "true") => Ok(true),
        Ok("0" | "false") | Err(std::env::VarError::NotPresent) => Ok(false),
        _ => Err("invalid cookie policy flag"),
    }
}
impl Policy {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn from_env() -> Result<Self, Error> {
        let policy = Self {
            enabled: flag("PIXSTRIDE_COOKIE_PASSTHROUGH")?,
            all: flag("PIXSTRIDE_COOKIE_PASSTHROUGH_ALL")?,
            base: std::env::var("PIXSTRIDE_COOKIE_BASE_URL").ok(),
        };
        if (policy.enabled || policy.all || policy.base.is_some())
            && !cfg!(feature = "source-cookies")
        {
            return Err("cookie configuration requires source-cookies");
        }
        if let Some(base) = &policy.base {
            #[cfg(feature = "source-cookies")]
            parse_url(base)?;
            if base.len() > 2048 {
                return Err("cookie base URL exceeds limit");
            }
        }
        Ok(policy)
    }
    pub fn jar(&self, header: &str, host: &str) -> Result<Option<Jar>, Error> {
        if !self.enabled {
            return Ok(None);
        }
        #[cfg(feature = "source-cookies")]
        {
            let base = self
                .base
                .clone()
                .unwrap_or_else(|| format!("http://{host}/"));
            let base = parse_url(&base)?;
            let mut jar = Jar {
                store: Default::default(),
                unrestricted: Vec::new(),
                all: self.all,
            };
            for cookie in cookie_store::RawCookie::split_parse(header).flatten() {
                jar.insert(cookie, &base)?;
            }
            Ok(Some(jar))
        }
        #[cfg(not(feature = "source-cookies"))]
        {
            let _ = (header, host);
            Err("source-cookies is not enabled")
        }
    }
}
#[cfg(feature = "source-cookies")]
fn parse_url(value: &str) -> Result<url::Url, Error> {
    let url = url::Url::parse(value).map_err(|_| "invalid cookie URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("invalid cookie URL");
    }
    Ok(url)
}
#[cfg(feature = "source-cookies")]
pub(crate) struct Jar {
    store: cookie_store::CookieStore,
    unrestricted: Vec<(String, String)>,
    all: bool,
}
#[cfg(feature = "source-cookies")]
impl Jar {
    fn insert(&mut self, cookie: cookie_store::RawCookie<'_>, url: &url::Url) -> Result<(), Error> {
        if cookie.name().len() + cookie.value().len() > 4096 {
            return Err("source cookie exceeds limit");
        }
        if self.all {
            if self.unrestricted.len() >= 64 {
                return Err("too many source cookies");
            }
            self.unrestricted
                .push((cookie.name().into(), cookie.value().into()));
        } else {
            // CookieStore implements RFC6265 domain/path/secure/expiry handling.
            // Its optional dynamic suffix list is replaced with the pinned,
            // compiled Mozilla list so startup never downloads policy data.
            if cookie.domain().is_some_and(|d| {
                let d = d.trim_start_matches('.').trim_end_matches('.');
                let Ok(host) = url::Host::parse(d) else {
                    return true;
                };
                let ascii = host.to_string().to_ascii_lowercase();
                psl::suffix_str(&ascii) == Some(ascii.as_str())
            }) {
                return Ok(());
            }
            let _ = self.store.insert_raw(&cookie, url);
            if self.store.iter_any().count() > 64 {
                return Err("too many source cookies");
            }
        }
        Ok(())
    }
    pub fn header(&self, source: &str) -> Result<Option<String>, Error> {
        let url = parse_url(source)?;
        let values = if self.all {
            self.unrestricted
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect::<Vec<_>>()
        } else {
            // Match Go's jar: Secure requires HTTPS, including on loopback.
            self.store
                .matches(&url)
                .into_iter()
                .filter(|c| c.secure() != Some(true) || url.scheme() == "https")
                .map(|c| (c.name(), c.value()))
                .collect::<Vec<_>>()
        };
        if values.is_empty() {
            return Ok(None);
        }
        let value = values
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if value.len() > 8192 || value.bytes().any(|b| !(32..127).contains(&b)) {
            return Err("invalid source Cookie header");
        }
        Ok(Some(value))
    }
    pub fn response(&mut self, source: &str, value: &str) -> Result<(), Error> {
        if value.len() > 4096 {
            return Err("source Set-Cookie exceeds limit");
        }
        if let Ok(cookie) = cookie_store::RawCookie::parse(value) {
            self.insert(cookie, &parse_url(source)?)?;
        }
        Ok(())
    }
}
#[cfg(not(feature = "source-cookies"))]
pub(crate) struct Jar;
#[cfg(not(feature = "source-cookies"))]
impl Jar {
    pub fn header(&self, _: &str) -> Result<Option<String>, Error> {
        Ok(None)
    }
    pub fn response(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }
}
#[cfg(all(test, feature = "source-cookies"))]
mod tests {
    use super::*;
    #[test]
    fn domain_path_secure_expiry_and_public_suffix_rules_survive_redirects() {
        let policy = Policy {
            enabled: true,
            all: false,
            base: Some("https://images.example.com/".into()),
        };
        let mut jar = policy
            .jar("session=synthetic; mode=small", "unused")
            .unwrap()
            .unwrap();
        assert!(jar
            .header("https://images.example.com/a")
            .unwrap()
            .unwrap()
            .contains("session=synthetic"));
        assert!(jar.header("https://other.example.com/a").unwrap().is_none());
        jar.response(
            "https://images.example.com/dir/a",
            "scoped=value; Path=/dir; Secure",
        )
        .unwrap();
        assert!(jar
            .header("https://images.example.com/dir/b")
            .unwrap()
            .unwrap()
            .contains("scoped=value"));
        assert!(!jar
            .header("http://images.example.com/dir/b")
            .unwrap()
            .unwrap()
            .contains("scoped="));
        jar.response("https://images.example.com/a", "attack=x; Domain=com")
            .unwrap();
        jar.response("https://images.example.com/a", "attack=x; Domain=co.uk")
            .unwrap();
        jar.response(
            "https://images.example.com/a",
            "attack=x; Domain=other.example.com",
        )
        .unwrap();
        jar.response("https://images.example.com/a", "session=; Max-Age=0")
            .unwrap();
        assert!(!jar
            .header("https://images.example.com/")
            .unwrap()
            .unwrap()
            .contains("session="));
        assert!(jar.header("https://other.example.com/").unwrap().is_none());
        jar.response("https://image.xn--55qx5d.cn/", "attack=x; Domain=公司.cn")
            .unwrap();
        assert!(jar
            .header("https://other.xn--55qx5d.cn/")
            .unwrap()
            .is_none());
        let clean = policy.jar("", "unused").unwrap().unwrap();
        assert!(clean
            .header("https://images.example.com/")
            .unwrap()
            .is_none());
    }
    #[test]
    fn unrestricted_mode_is_explicit_and_its_storage_is_bounded() {
        let policy = Policy {
            enabled: true,
            all: true,
            base: Some("https://images.example.com/".into()),
        };
        let mut jar = policy.jar("a=b", "unused").unwrap().unwrap();
        assert_eq!(
            jar.header("https://other.example.net/").unwrap().unwrap(),
            "a=b"
        );
        for i in 0..63 {
            jar.response("https://images.example.com/", &format!("k{i}=v"))
                .unwrap();
        }
        assert!(jar
            .response("https://images.example.com/", "last=x")
            .is_err());
    }
}
