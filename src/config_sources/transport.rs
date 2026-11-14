//! Secret/identity traffic has separate administrator policy and bounded replies.
use super::{read, Values};
use bytes::Bytes;
use reqsign_core::{Context, Env, FileRead, HttpSend};
use std::{collections::HashMap, fmt, path::PathBuf, time::Duration};
#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    metadata: bool,
    google_token_endpoint: Option<String>,
}
impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfigurationTransport")
    }
}
fn error() -> reqsign_core::Error {
    reqsign_core::Error::unexpected("configuration transport failed")
}
impl Transport {
    pub fn new(values: &Values) -> Result<Self, crate::Error> {
        let metadata = match values
            .get("PIXSTRIDE_ENV_ALLOW_METADATA_CREDENTIALS")
            .as_deref()
        {
            None | Some("0" | "false") => false,
            Some("1" | "true") => true,
            _ => return Err("invalid metadata credential flag"),
        };
        let mut client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .http1_only()
            .http1_max_headers(128)
            .pool_max_idle_per_host(2);
        if let Some(path) = values.get("PIXSTRIDE_ENV_ROOT_CA_FILE") {
            let cert = reqwest::Certificate::from_pem(&read(&path)?)
                .map_err(|_| "invalid configuration root CA")?;
            client = client.add_root_certificate(cert);
        }
        let client = client
            .build()
            .map_err(|_| "configuration TLS unavailable")?;
        Ok(Self {
            client,
            metadata,
            google_token_endpoint: values.get("PIXSTRIDE_ENV_GCP_TOKEN_ENDPOINT"),
        })
    }
    pub async fn send(
        &self,
        req: http::Request<Bytes>,
    ) -> Result<http::Response<Bytes>, crate::Error> {
        self.http_send(req)
            .await
            .map_err(|_| "cannot retrieve cloud configuration")
    }
    pub fn context(&self, values: &Values) -> Context {
        let mut envs: HashMap<String, String> = std::env::vars().collect();
        envs.extend(values.loaded.clone());
        envs.entry("GOOGLE_SCOPE".into())
            .or_insert_with(|| "https://www.googleapis.com/auth/cloud-platform".into());
        Context::new()
            .with_http_send(self.clone())
            .with_file_read(Files)
            .with_env(Snapshot(envs))
    }
}
#[derive(Clone)]
struct Snapshot(HashMap<String, String>);
impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfigurationEnvironment")
    }
}
impl Env for Snapshot {
    fn var(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn vars(&self) -> HashMap<String, String> {
        self.0.clone()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.0.get("HOME").map(PathBuf::from)
    }
}
#[derive(Debug)]
struct Files;
impl FileRead for Files {
    async fn file_read(&self, path: &str) -> reqsign_core::Result<Vec<u8>> {
        read(path).map_err(|_| error())
    }
}
impl HttpSend for Transport {
    async fn http_send(
        &self,
        req: http::Request<Bytes>,
    ) -> reqsign_core::Result<http::Response<Bytes>> {
        let (parts, body) = req.into_parts();
        let mut url = reqwest::Url::parse(&parts.uri.to_string()).map_err(|_| error())?;
        if url.host_str() == Some("oauth2.googleapis.com") && url.path() == "/token" {
            if let Some(endpoint) = &self.google_token_endpoint {
                url = reqwest::Url::parse(endpoint).map_err(|_| error())?;
            }
        }
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        let metadata = self.metadata
            && matches!(
                url.host_str(),
                Some("169.254.169.254" | "169.254.170.2" | "metadata.google.internal")
            );
        if body.len() > 1024 * 1024
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || (url.scheme() != "https" && !(url.scheme() == "http" && (loopback || metadata)))
        {
            return Err(error());
        }
        let mut response = self
            .client
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| error())?;
        if !response.status().is_success()
            || response.content_length().is_some_and(|v| v > 1024 * 1024)
            || response
                .headers()
                .iter()
                .map(|(k, v)| k.as_str().len() + v.as_bytes().len() + 4)
                .sum::<usize>()
                > 65536
        {
            return Err(error());
        }
        let status = response.status();
        let headers = response.headers().clone();
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| error())? {
            if chunk.len() > (1024 * 1024usize).saturating_sub(data.len()) {
                return Err(error());
            }
            data.try_reserve(chunk.len()).map_err(|_| error())?;
            data.extend_from_slice(&chunk);
        }
        let mut result = http::Response::new(Bytes::from(data));
        *result.status_mut() = status;
        *result.headers_mut() = headers;
        Ok(result)
    }
}
