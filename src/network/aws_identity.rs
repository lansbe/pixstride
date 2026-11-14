//! Resolve AWS identities through the same bounded source transport, including STS.
use crate::{config, Error};
use bytes::Bytes;
use reqsign_aws_v4::{Credential, DefaultCredentialProvider, StaticCredentialProvider};
use reqsign_core::{Context, FileRead, HttpSend, ProvideCredential};
use std::fmt;
pub(super) fn region(bucket: &str) -> Result<String, Error> {
    let mut result = std::env::var("PIXSTRIDE_S3_REGION")
        .or_else(|_| std::env::var("AWS_REGION"))
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|_| "us-east-1".into());
    let routes = std::env::var("PIXSTRIDE_S3_BUCKET_REGIONS").unwrap_or_default();
    if routes.len() > 8192 {
        return Err("S3 region mapping exceeds limit");
    }
    let mut seen = std::collections::BTreeSet::new();
    for route in routes.split(',').filter(|s| !s.trim().is_empty()) {
        let (name, region) = route
            .trim()
            .split_once('=')
            .ok_or("invalid S3 region mapping")?;
        let (name, region) = (name.trim(), region.trim());
        if name.is_empty()
            || name.len() > 253
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            || !valid_region(region)
            || !seen.insert(name)
        {
            return Err("invalid S3 region mapping");
        }
        if name == bucket {
            result = region.into();
        }
    }
    if !valid_region(&result) {
        return Err("invalid S3 region");
    }
    Ok(result)
}
fn valid_region(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub(crate) async fn load(region: &str, source: bool) -> Result<Credential, Error> {
    let ctx = Context::new()
        .with_env(reqsign_core::OsEnv)
        .with_file_read(Files)
        .with_http_send(Sender(
            super::cloud_transport::Transport::new(1024 * 1024)
                .map_err(|_| "AWS identity transport unavailable")?,
        ));
    let s3_key = if source {
        config::secret("PIXSTRIDE_S3_ACCESS_KEY_ID")?
    } else {
        None
    };
    let s3_secret = if source {
        config::secret("PIXSTRIDE_S3_SECRET_ACCESS_KEY")?
    } else {
        None
    };
    let (key, secret, token) = if s3_key.is_some() || s3_secret.is_some() {
        (
            s3_key,
            s3_secret,
            config::secret("PIXSTRIDE_S3_SESSION_TOKEN")?,
        )
    } else {
        (
            config::secret("AWS_ACCESS_KEY_ID")?,
            config::secret("AWS_SECRET_ACCESS_KEY")?,
            config::secret("AWS_SESSION_TOKEN")?,
        )
    };
    let mut credential = match (key, secret) {
        (Some(key), Some(secret)) => {
            let mut provider = StaticCredentialProvider::new(&key, &secret);
            if let Some(token) = token {
                provider = provider.with_session_token(&token);
            }
            provider.provide_credential(&ctx).await
        }
        (None, None) => {
            DefaultCredentialProvider::new()
                .provide_credential(&ctx)
                .await
        }
        _ => return Err("incomplete AWS credentials"),
    }
    .map_err(|_| "AWS credentials unavailable")?
    .ok_or("AWS credentials unavailable")?;
    if source {
        if let Some(role) = config::secret("PIXSTRIDE_S3_ASSUME_ROLE_ARN")? {
            let signer = reqsign_core::Signer::new(
                ctx.clone(),
                Fixed(credential),
                reqsign_aws_v4::RequestSigner::new("sts", region),
            );
            let mut provider = reqsign_aws_v4::AssumeRoleCredentialProvider::new(role, signer)
                .with_region(region.into())
                .with_regional_sts_endpoint();
            if let Some(id) = config::secret("PIXSTRIDE_S3_ASSUME_ROLE_EXTERNAL_ID")? {
                provider = provider.with_external_id(id);
            }
            credential = provider
                .provide_credential(&ctx)
                .await
                .map_err(|_| "AWS role credentials unavailable")?
                .ok_or("AWS role credentials unavailable")?;
        }
    }
    Ok(credential)
}
struct Fixed(Credential);
impl fmt::Debug for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FixedCredential")
    }
}
impl ProvideCredential for Fixed {
    type Credential = Credential;
    async fn provide_credential(&self, _: &Context) -> reqsign_core::Result<Option<Credential>> {
        Ok(Some(self.0.clone()))
    }
}
struct Sender(super::cloud_transport::Transport);
impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AWSIdentityTransport")
    }
}
fn error() -> reqsign_core::Error {
    reqsign_core::Error::unexpected("AWS identity operation failed")
}
impl HttpSend for Sender {
    async fn http_send(
        &self,
        req: http::Request<Bytes>,
    ) -> reqsign_core::Result<http::Response<Bytes>> {
        use opendal::HttpTransport;
        let (parts, body) = req.into_parts();
        let mut response = self
            .0
            .fetch(http::Request::from_parts(
                parts,
                opendal::Buffer::from(body),
            ))
            .await
            .map_err(|_| error())?;
        if !response.status().is_success() {
            return Err(error());
        }
        let data = response
            .body_mut()
            .to_buffer()
            .await
            .map_err(|_| error())?
            .to_vec();
        let (parts, _) = response.into_parts();
        Ok(http::Response::from_parts(parts, Bytes::from(data)))
    }
}
#[derive(Debug)]
struct Files;
impl FileRead for Files {
    async fn file_read(&self, path: &str) -> reqsign_core::Result<Vec<u8>> {
        config::read_regular_file(path, 65536).map_err(|_| error())
    }
}
