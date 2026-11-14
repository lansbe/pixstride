//! SDK transport: no proxies/redirects, checked addresses, bounded bodies.
use opendal::{Buffer, Error, ErrorKind, HttpBody, HttpTransport};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    max: usize,
}
struct Resolver;
impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let addrs = addrs
                .filter(|a| super::fetch::policy::allows_ip(a.ip()))
                .collect::<Vec<SocketAddr>>();
            if addrs.is_empty() {
                return Err(
                    Box::new(std::io::Error::other("permitted destination required"))
                        as Box<dyn std::error::Error + Send + Sync>,
                );
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}
fn error(message: &'static str) -> Error {
    Error::new(ErrorKind::Unexpected, message)
}
fn identity(host: &str) -> bool {
    matches!(
        host,
        "oauth2.googleapis.com"
            | "sts.googleapis.com"
            | "iamcredentials.googleapis.com"
            | "login.microsoftonline.com"
            | "sts.amazonaws.com"
    ) || (host.starts_with("sts.") && host.ends_with(".amazonaws.com"))
}
impl Transport {
    pub fn new(max: usize) -> opendal::Result<Self> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .http1_only()
            .http1_max_headers(128)
            .pool_max_idle_per_host(4)
            .dns_resolver(Arc::new(Resolver))
            .build()
            .map_err(|_| error("cloud transport initialization failed"))?;
        Ok(Self { client, max })
    }
}
impl HttpTransport for Transport {
    async fn fetch(
        &self,
        request: http::Request<Buffer>,
    ) -> opendal::Result<http::Response<HttpBody>> {
        let (parts, body) = request.into_parts();
        let url = super::fetch::Url::parse(&parts.uri.to_string())
            .map_err(|_| error("invalid cloud endpoint"))?;
        if url.scheme != "https" {
            return Err(error("cloud endpoints require HTTPS"));
        }
        if !super::fetch::policy::allows_port(url.port) {
            return Err(error("cloud endpoint port refused"));
        }
        if let Ok(ip) = url.host.parse::<IpAddr>() {
            if !super::fetch::policy::allows_ip(ip) {
                return Err(error("private cloud endpoint refused"));
            }
        }
        let auth = identity(&url.host);
        if !auth && !super::fetch::allows_host(&url.host) {
            return Err(error("cloud endpoint is not in allowed hosts"));
        }
        if body.len() > 1024 * 1024 {
            return Err(error("cloud request body exceeds limit"));
        }
        let mut response = self
            .client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| error("cloud HTTP request failed"))?;
        if response.status().is_redirection() {
            return Err(error(
                "cloud redirects require a correctly configured regional endpoint",
            ));
        }
        if response
            .headers()
            .iter()
            .map(|(k, v)| k.as_str().len() + v.as_bytes().len() + 4)
            .sum::<usize>()
            > 65536
        {
            return Err(error("cloud headers exceed limit"));
        }
        let max = if auth { 1024 * 1024 } else { self.max };
        if response.content_length().is_some_and(|n| n > max as u64) {
            return Err(error("cloud body exceeds limit"));
        }
        let status = response.status();
        let headers = response.headers().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| error("incomplete cloud body"))?
        {
            if chunk.len() > max.saturating_sub(bytes.len()) {
                return Err(error("cloud body exceeds limit"));
            }
            bytes
                .try_reserve(chunk.len())
                .map_err(|_| error("cloud allocation failed"))?;
            bytes.extend_from_slice(&chunk);
        }
        let size = bytes.len() as u64;
        let stream = futures_util::stream::iter([Ok(Buffer::from(bytes))]);
        let mut result = http::Response::new(HttpBody::new(stream, Some(size)));
        *result.status_mut() = status;
        *result.headers_mut() = headers;
        Ok(result)
    }
}
