//! Administrator-selected telemetry destinations, TLS and bounded HTTP transport.
use super::payload::Kind;
use crate::Error;
use reqwest::{
    header::{HeaderMap, HeaderValue},
    Url,
};
pub(super) struct Endpoint {
    pub url: Url,
    pub kind: Kind,
    pub headers: HeaderMap,
}
pub(super) fn url(value: &str) -> Result<Url, Error> {
    if value.len() > 2048 {
        return Err("telemetry endpoint exceeds limit");
    }
    let url = Url::parse(value).map_err(|_| "invalid telemetry endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid telemetry endpoint");
    }
    Ok(url)
}
pub(super) fn header(value: &str) -> Result<HeaderValue, Error> {
    HeaderValue::from_str(value).map_err(|_| "invalid telemetry credential header")
}
pub(super) fn endpoints() -> Result<Vec<Endpoint>, Error> {
    let mut result = Vec::new();
    let mut headers = HeaderMap::new();
    if let Some(value) = crate::config::secret("PIXSTRIDE_OTLP_AUTHORIZATION")? {
        headers.insert(reqwest::header::AUTHORIZATION, header(&value)?);
    }
    for (name, kind) in [
        ("PIXSTRIDE_OTLP_ENDPOINT", Kind::Traces),
        ("PIXSTRIDE_OTLP_METRICS_ENDPOINT", Kind::Metrics),
        ("PIXSTRIDE_OTLP_LOGS_ENDPOINT", Kind::Logs),
    ] {
        if let Ok(value) = std::env::var(name) {
            result.push(Endpoint {
                url: url(&value)?,
                kind,
                headers: headers.clone(),
            });
        }
    }
    if let Some(key) = crate::config::secret("PIXSTRIDE_NEW_RELIC_KEY")? {
        let mut headers = HeaderMap::new();
        headers.insert("api-key", header(&key)?);
        let base = std::env::var("PIXSTRIDE_NEW_RELIC_ENDPOINT")
            .unwrap_or_else(|_| "https://otlp.nr-data.net".into());
        for (path, kind) in [
            ("traces", Kind::Traces),
            ("metrics", Kind::Metrics),
            ("logs", Kind::Logs),
        ] {
            result.push(Endpoint {
                url: url(&format!("{}/v1/{path}", base.trim_end_matches('/')))?,
                kind,
                headers: headers.clone(),
            });
        }
    }
    if let Some(key) = crate::config::secret("PIXSTRIDE_DATADOG_KEY")? {
        let mut headers = HeaderMap::new();
        headers.insert("dd-api-key", header(&key)?);
        let base = std::env::var("PIXSTRIDE_DATADOG_ENDPOINT")
            .unwrap_or_else(|_| "https://otlp.datadoghq.com".into());
        for (path, kind) in [
            ("traces", Kind::Traces),
            ("metrics", Kind::Metrics),
            ("logs", Kind::Logs),
        ] {
            result.push(Endpoint {
                url: url(&format!("{}/v1/{path}", base.trim_end_matches('/')))?,
                kind,
                headers: headers.clone(),
            });
        }
    }
    Ok(result)
}
fn file(name: &str) -> Result<Option<Vec<u8>>, Error> {
    use std::io::Read;
    let Some(path) = std::env::var_os(name) else {
        return Ok(None);
    };
    let mut file = std::fs::File::open(path).map_err(|_| "cannot read telemetry certificate")?;
    let mut data = Vec::new();
    file.by_ref()
        .take(65537)
        .read_to_end(&mut data)
        .map_err(|_| "cannot read telemetry certificate")?;
    if data.len() > 65536 {
        return Err("telemetry certificate exceeds limit");
    }
    Ok(Some(data))
}
pub(super) fn client() -> Result<reqwest::Client, Error> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(1))
        .user_agent(concat!("pixstride/", env!("CARGO_PKG_VERSION")));
    if let Some(data) = file("PIXSTRIDE_OTLP_SERVER_CERT")? {
        for cert in reqwest::Certificate::from_pem_bundle(&data)
            .map_err(|_| "invalid telemetry server certificate")?
        {
            builder = builder.add_root_certificate(cert);
        }
    }
    match (
        file("PIXSTRIDE_OTLP_CLIENT_CERT")?,
        file("PIXSTRIDE_OTLP_CLIENT_KEY")?,
    ) {
        (Some(cert), Some(key)) => {
            builder = builder.identity(
                reqwest::Identity::from_pkcs8_pem(&cert, &key)
                    .map_err(|_| "invalid telemetry client identity")?,
            )
        }
        (None, None) => {}
        _ => return Err("telemetry client certificate and key must be paired"),
    }
    builder
        .build()
        .map_err(|_| "telemetry transport unavailable")
}
