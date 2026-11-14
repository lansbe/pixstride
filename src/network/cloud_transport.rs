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
#[derive(Clone)]
pub(super) struct Generation<T> {
    pub inner: T,
    pub value: Option<String>,
}
impl<T: HttpTransport> HttpTransport for Generation<T> {
    async fn fetch(
        &self,
        mut request: http::Request<Buffer>,
    ) -> opendal::Result<http::Response<HttpBody>> {
        if let Some(value) = &self.value {
            if matches!(
                request.extensions().get::<opendal::raw::Operation>(),
                Some(opendal::raw::Operation::Read | opendal::raw::Operation::Stat)
            ) {
                let separator = if request.uri().query().is_some() {
                    '&'
                } else {
                    '?'
                };
                *request.uri_mut() = format!("{}{separator}generation={value}", request.uri())
                    .parse()
                    .map_err(|_| error("invalid object generation"))?;
            }
        }
        self.inner.fetch(request).await
    }
}
impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let addrs = addrs
                .filter(|a| {
                    super::fetch::policy::allows_ip(a.ip())
                        || super::credential_metadata::enabled()
                            && name.as_str() == "metadata.google.internal"
                            && a.ip() == "169.254.169.254".parse::<IpAddr>().unwrap()
                })
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
        let mut client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .http1_only()
            .http1_max_headers(128)
            .pool_max_idle_per_host(4)
            .dns_resolver(Arc::new(Resolver));
        if let Ok(path) = std::env::var("PIXSTRIDE_CLOUD_ROOT_CA_FILE") {
            let data = crate::config::read_regular_file(&path, 65536)
                .map_err(|_| error("cannot read cloud root CA"))?;
            client = client.add_root_certificate(
                reqwest::Certificate::from_pem(&data)
                    .map_err(|_| error("invalid cloud root CA"))?,
            );
        }
        let client = client
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
        let metadata = super::credential_metadata::permits(&url, &parts.method, &parts.headers);
        if url.scheme != "https" && !metadata {
            return Err(error("cloud endpoints require HTTPS"));
        }
        if !metadata && !super::fetch::policy::allows_port(url.port) {
            return Err(error("cloud endpoint port refused"));
        }
        if let Ok(ip) = url.host.parse::<IpAddr>() {
            if !super::fetch::policy::allows_ip(ip) && !metadata {
                return Err(error("private cloud endpoint refused"));
            }
        }
        let auth = identity(&url.host) || metadata;
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
