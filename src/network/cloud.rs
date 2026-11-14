//! Opt-in object storage readers. Credentials and SDK errors never enter logs.
use super::fetch;
#[cfg(feature = "cloud-sources")]
use crate::config;
use crate::Error;

pub(crate) fn split(source: &str) -> Result<(&str, &str, &str), Error> {
    let (scheme, rest) = source.split_once("://").ok_or("invalid storage source")?;
    if !matches!(scheme, "s3" | "gs" | "gcs" | "abs" | "swift") {
        return Err("unsupported storage source");
    }
    let (bucket, key) = rest.split_once('/').ok_or("storage object key required")?;
    if bucket.is_empty()
        || bucket.len() > 253
        || !bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
        || key.is_empty()
        || key.len() > 12000
        || key.split('/').any(|s| s == "." || s == "..")
        || key.contains(['?', '#', '\\'])
    {
        return Err("invalid storage bucket or object key");
    }
    Ok((scheme, bucket, key))
}

#[cfg(not(feature = "cloud-sources"))]
pub fn fetch(_: &str, _: usize) -> Result<fetch::Download, Error> {
    Err("cloud-sources is not enabled")
}

#[cfg(feature = "cloud-sources")]
pub fn fetch(source: &str, max: usize) -> Result<fetch::Download, Error> {
    use opendal::{services, HttpTransporter, OperationContext, Operator};
    use std::time::Duration;
    let (scheme, bucket, key) = split(source)?;
    let (enabled, allowed) = match scheme {
        "s3" => ("PIXSTRIDE_USE_S3", "PIXSTRIDE_S3_ALLOWED_BUCKETS"),
        "gs" | "gcs" => ("PIXSTRIDE_USE_GCS", "PIXSTRIDE_GCS_ALLOWED_BUCKETS"),
        "abs" => ("PIXSTRIDE_USE_ABS", "PIXSTRIDE_ABS_ALLOWED_CONTAINERS"),
        _ => ("PIXSTRIDE_USE_SWIFT", "PIXSTRIDE_SWIFT_ALLOWED_CONTAINERS"),
    };
    if std::env::var(enabled).as_deref() != Ok("1")
        || !std::env::var(allowed)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .any(|b| b == bucket)
    {
        return Err("storage provider or bucket is not enabled");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "cloud runtime unavailable")?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let transport = super::cloud_transport::Transport::new(max.min(crate::MAX_FILE))
                .map_err(|_| "invalid cloud transport")?;
            let operator = match scheme {
                "s3" => {
                    let region =
                        std::env::var("PIXSTRIDE_S3_REGION").unwrap_or_else(|_| "us-east-1".into());
                    let endpoint = std::env::var("PIXSTRIDE_S3_ENDPOINT")
                        .unwrap_or_else(|_| format!("https://s3.{region}.amazonaws.com"));
                    let mut b = services::S3::default()
                        .bucket(bucket)
                        .region(&region)
                        .endpoint(&endpoint);
                    if let Some(v) = config::secret("PIXSTRIDE_S3_ACCESS_KEY_ID")? {
                        b = b.access_key_id(&v);
                    }
                    if let Some(v) = config::secret("PIXSTRIDE_S3_SECRET_ACCESS_KEY")? {
                        b = b.secret_access_key(&v);
                    }
                    if let Some(v) = config::secret("PIXSTRIDE_S3_SESSION_TOKEN")? {
                        b = b.session_token(&v);
                    }
                    Operator::new(b)
                }
                "gs" | "gcs" => {
                    let endpoint = std::env::var("PIXSTRIDE_GCS_ENDPOINT")
                        .unwrap_or_else(|_| "https://storage.googleapis.com".into());
                    let mut b = services::Gcs::default().bucket(bucket).endpoint(&endpoint);
                    if let Some(v) = config::secret("PIXSTRIDE_GCS_CREDENTIAL")? {
                        b = b.credential(&v);
                    }
                    if let Ok(v) = std::env::var("PIXSTRIDE_GCS_CREDENTIAL_PATH") {
                        b = b.credential_path(&v);
                    }
                    Operator::new(b)
                }
                "abs" => {
                    let name = std::env::var("PIXSTRIDE_ABS_NAME")
                        .map_err(|_| "Azure account name required")?;
                    let endpoint = std::env::var("PIXSTRIDE_ABS_ENDPOINT")
                        .unwrap_or_else(|_| format!("https://{name}.blob.core.windows.net"));
                    let mut b = services::Azblob::default()
                        .container(bucket)
                        .account_name(&name)
                        .endpoint(&endpoint);
                    if let Some(v) = config::secret("PIXSTRIDE_ABS_KEY")? {
                        b = b.account_key(&v);
                    }
                    if let Some(v) = config::secret("PIXSTRIDE_ABS_SAS")? {
                        b = b.sas_token(&v);
                    }
                    Operator::new(b)
                }
                _ => {
                    let endpoint = std::env::var("PIXSTRIDE_SWIFT_ENDPOINT")
                        .map_err(|_| "Swift endpoint required")?;
                    let mut b = services::Swift::default()
                        .container(bucket)
                        .endpoint(&endpoint);
                    let token = if let Some(v) = config::secret("PIXSTRIDE_SWIFT_TOKEN")? {
                        v
                    } else {
                        super::swift_auth::token(
                            &super::cloud_transport::Transport::new(1024 * 1024)
                                .map_err(|_| "Keystone transport unavailable")?,
                        )
                        .await?
                    };
                    b = b.token(&token);
                    Operator::new(b)
                }
            }
            .map_err(|_| "invalid storage configuration")?;
            let operator = operator.with_context(
                OperationContext::new().with_http_transport(HttpTransporter::new(transport)),
            );
            tokio::time::timeout(Duration::from_secs(15), async {
                let stat = operator
                    .stat(key)
                    .await
                    .map_err(|_| "cannot inspect storage source")?;
                if !stat.is_file() || stat.content_length() > max as u64 {
                    return Err("storage source exceeds limit");
                }
                let read = operator.read_with(key).range(0..max as u64 + 1);
                let read = if let Some(etag) = stat.etag() {
                    read.if_match(etag)
                } else {
                    read
                };
                let body = read
                    .await
                    .map_err(|_| "cannot retrieve complete storage source")?
                    .to_vec();
                if body.len() > max || body.len() as u64 != stat.content_length() {
                    return Err("storage source changed or exceeded limit");
                }
                Ok(fetch::Download {
                    upstream_bytes: body.len() as u64,
                    body,
                    new_connections: 0,
                    transfers: 2,
                    cache_control: stat
                        .cache_control()
                        .filter(|v| v.len() <= 2048 && v.bytes().all(|b| (32..127).contains(&b)))
                        .map(str::to_owned),
                    expires: None,
                    last_modified: stat
                        .last_modified()
                        .map(std::time::SystemTime::from)
                        .filter(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .is_ok_and(|d| d.as_secs() <= 253_402_300_799)
                        })
                        .map(httpdate::fmt_http_date),
                })
            })
            .await
            .map_err(|_| "storage deadline exceeded")?
        })
        .await
        .map_err(|_| "storage operation deadline exceeded")?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_authorities_cannot_select_an_endpoint_or_escape_an_object_prefix() {
        assert_eq!(
            split("s3://images/a/b.png").unwrap(),
            ("s3", "images", "a/b.png")
        );
        for s in [
            "s3://images/../secret",
            "s3://user@host/a",
            "s3://host:80/a",
            "gs://images/a?token=x",
            "swift://images/./x",
            "abs://images/",
        ] {
            assert!(split(s).is_err(), "{s}");
        }
    }
}
