//! Immutable operational policy, validated before opening a listener.
use crate::{config, request::OutputFormat, Error};

#[derive(Default)]
pub(crate) struct Settings {
    pub secret: Option<String>,
    pub ttl: u64,
    pub etag: bool,
    pub auto_formats: Vec<OutputFormat>,
    pub origin: Option<String>,
    pub last_modified: bool,
    pub modified_buster: u64,
    pub fallback: Option<crate::watermark::Config>,
    pub fallback_code: u16,
    pub fallback_ttl: u64,
    pub forced_formats: Vec<OutputFormat>,
    pub client_hints: bool,
    pub path_prefix: String,
    pub health_path: String,
    pub canonical: bool,
    pub etag_buster: String,
}

impl Settings {
    pub fn from_env() -> Result<Self, Error> {
        match std::env::var("PIXSTRIDE_ENGINE").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("builtin") => {}
            Ok("native") if cfg!(feature = "native-codecs") => {}
            _ => return Err("engine must be builtin or an enabled native engine"),
        }
        if let Ok(value) = std::env::var("PIXSTRIDE_PROCESS_TIMEOUT") {
            if !value.parse::<u64>().is_ok_and(|n| (1..=120).contains(&n)) {
                return Err("processing timeout must be 1..120 seconds");
            }
        }
        for provider in [
            "PIXSTRIDE_USE_S3",
            "PIXSTRIDE_USE_GCS",
            "PIXSTRIDE_USE_ABS",
            "PIXSTRIDE_USE_SWIFT",
        ] {
            if std::env::var(provider)
                .is_ok_and(|v| v != "0" && (v != "1" || !cfg!(feature = "cloud-sources")))
            {
                return Err("cloud provider requires cloud-sources and flag 0 or 1");
            }
        }
        let secret = config::secret("PIXSTRIDE_SECRET")?;
        if secret
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.chars().any(char::is_control))
        {
            return Err("invalid HTTP authorization secret");
        }
        let flag = |name: &str| match std::env::var(name) {
            Ok(v) if v == "1" || v == "true" => Ok(true),
            Ok(v) if v == "0" || v == "false" => Ok(false),
            Err(std::env::VarError::NotPresent) => Ok(false),
            _ => Err("invalid HTTP configuration flag"),
        };
        let number = |name: &str| {
            std::env::var(name)
                .unwrap_or_else(|_| "0".into())
                .parse::<u64>()
                .map_err(|_| "invalid HTTP configuration number")
        };
        let ttl = number("PIXSTRIDE_TTL")?;
        if ttl > 31_536_000 {
            return Err("HTTP TTL exceeds one year");
        }
        let modified_buster = number("PIXSTRIDE_LAST_MODIFIED_BUSTER")?;
        if modified_buster > 253_402_300_799 {
            return Err("Last-Modified buster exceeds HTTP date range");
        }
        let mut auto_formats = Vec::new();
        for (name, format) in [
            ("PIXSTRIDE_AUTO_JXL", OutputFormat::Jxl),
            ("PIXSTRIDE_AUTO_AVIF", OutputFormat::Avif),
            ("PIXSTRIDE_AUTO_WEBP", OutputFormat::Webp),
        ] {
            if flag(name)? {
                auto_formats.push(format);
            }
        }
        if !auto_formats.is_empty() && !cfg!(feature = "native-codecs") {
            return Err("automatic modern formats require native-codecs");
        }
        let mut forced_formats = Vec::new();
        for (name, format) in [
            ("PIXSTRIDE_ENFORCE_JXL", OutputFormat::Jxl),
            ("PIXSTRIDE_ENFORCE_AVIF", OutputFormat::Avif),
            ("PIXSTRIDE_ENFORCE_WEBP", OutputFormat::Webp),
        ] {
            if flag(name)? {
                forced_formats.push(format);
            }
        }
        if !forced_formats.is_empty() && !cfg!(feature = "native-codecs") {
            return Err("enforced formats require native-codecs");
        }
        let fallback = crate::watermark::Config::fallback_from_env()?;
        let fallback = fallback.is_configured().then_some(fallback);
        let fallback_code = std::env::var("PIXSTRIDE_FALLBACK_IMAGE_HTTP_CODE")
            .unwrap_or_else(|_| "200".into())
            .parse::<u16>()
            .map_err(|_| "invalid fallback HTTP code")?;
        if fallback_code != 0 && !(200..=599).contains(&fallback_code) {
            return Err("invalid fallback HTTP code");
        }
        let fallback_ttl = number("PIXSTRIDE_FALLBACK_IMAGE_TTL")?;
        if fallback_ttl > 31_536_000 {
            return Err("fallback TTL exceeds one year");
        }
        let path_prefix = std::env::var("PIXSTRIDE_PATH_PREFIX").unwrap_or_default();
        let health_path =
            std::env::var("PIXSTRIDE_HEALTH_CHECK_PATH").unwrap_or_else(|_| "/health".into());
        for path in [&path_prefix, &health_path] {
            if path.len() > 1024
                || (!path.is_empty() && !path.starts_with('/'))
                || path
                    .bytes()
                    .any(|b| b <= 32 || b >= 127 || b"?#\\".contains(&b))
            {
                return Err("invalid HTTP route configuration");
            }
        }
        let etag_buster = std::env::var("PIXSTRIDE_ETAG_BUSTER").unwrap_or_default();
        if etag_buster.len() > 1024 {
            return Err("ETag buster exceeds limit");
        }
        let origin = std::env::var("PIXSTRIDE_ALLOW_ORIGIN").ok();
        if origin
            .as_ref()
            .is_some_and(|s| s.len() > 2048 || s.bytes().any(|b| b <= 32 || b >= 127))
        {
            return Err("invalid CORS origin");
        }
        Ok(Self {
            secret,
            ttl,
            etag: flag("PIXSTRIDE_USE_ETAG")?,
            auto_formats,
            origin,
            last_modified: flag("PIXSTRIDE_USE_LAST_MODIFIED")?,
            modified_buster,
            fallback,
            fallback_code,
            fallback_ttl,
            forced_formats,
            client_hints: flag("PIXSTRIDE_ENABLE_CLIENT_HINTS")?,
            path_prefix: path_prefix.trim_end_matches('/').into(),
            health_path,
            canonical: flag("PIXSTRIDE_SET_CANONICAL_HEADER")?,
            etag_buster,
        })
    }
}
