//! AWS Secrets Manager/SSM and Google Secret Manager configuration documents.
use super::{transport::Transport, Values};
use crate::Error;
use bytes::Bytes;
use reqsign_core::{ProvideCredential, SignRequest};
use serde_json::{json, Value};

pub(super) async fn load(values: &mut Values) -> Result<(), Error> {
    let transport = Transport::new(values)?;
    if let Some(id) = values.get("PIXSTRIDE_ENV_AWS_SECRET_ID") {
        let mut body = json!({"SecretId":id});
        if let Some(v) = values.get("PIXSTRIDE_ENV_AWS_SECRET_VERSION_ID") {
            body["VersionId"] = v.into();
        } else if let Some(v) = values.get("PIXSTRIDE_ENV_AWS_SECRET_VERSION_STAGE") {
            body["VersionStage"] = v.into();
        }
        let result = aws(
            values,
            &transport,
            "secretsmanager",
            "secretsmanager.GetSecretValue",
            body,
        )
        .await?;
        let doc = result
            .get("SecretString")
            .and_then(Value::as_str)
            .ok_or("cloud secret must contain an environment document")?;
        values.document(doc.as_bytes())?;
    }
    if let Some(path) = values.get("PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_PATH") {
        let mut next = None;
        for page in 0..16 {
            let mut body = json!({"Path":path,"WithDecryption":true,"MaxResults":10});
            if let Some(token) = next {
                body["NextToken"] = Value::String(token);
            }
            let result = aws(
                values,
                &transport,
                "ssm",
                "AmazonSSM.GetParametersByPath",
                body,
            )
            .await?;
            for p in result
                .get("Parameters")
                .and_then(Value::as_array)
                .ok_or("invalid parameter response")?
            {
                let name = p
                    .get("Name")
                    .and_then(Value::as_str)
                    .ok_or("invalid parameter name")?;
                let value = p
                    .get("Value")
                    .and_then(Value::as_str)
                    .ok_or("invalid parameter value")?;
                values.insert(
                    name.rsplit('/')
                        .next()
                        .ok_or("invalid parameter name")?
                        .into(),
                    value.into(),
                )?;
            }
            next = result
                .get("NextToken")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            if next.is_none() {
                break;
            }
            if next.as_ref().is_some_and(|v| v.len() > 4096) || page == 15 {
                return Err("parameter pagination exceeds limit");
            }
        }
    }
    if let Some(id) = values.get("PIXSTRIDE_ENV_GCP_SECRET_ID") {
        let project = values
            .get("PIXSTRIDE_ENV_GCP_SECRET_PROJECT_ID")
            .ok_or("secret project is required")?;
        let version = values
            .get("PIXSTRIDE_ENV_GCP_SECRET_VERSION_ID")
            .unwrap_or_else(|| "latest".into());
        for v in [&id, &project, &version] {
            if v.is_empty()
                || v.len() > 253
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            {
                return Err("invalid Google secret resource");
            }
        }
        let endpoint = values
            .get("PIXSTRIDE_ENV_GCP_ENDPOINT")
            .unwrap_or_else(|| "https://secretmanager.googleapis.com/".into());
        let url = reqwest::Url::parse(&endpoint)
            .and_then(|u| {
                u.join(&format!(
                    "v1/projects/{project}/secrets/{id}/versions/{version}:access"
                ))
            })
            .map_err(|_| "invalid Google secret endpoint")?;
        let (mut parts, body) = http::Request::get(url.as_str())
            .body(Bytes::new())
            .map_err(|_| "invalid Google secret request")?
            .into_parts();
        if let Some(token) = values.secret("PIXSTRIDE_ENV_GCP_ACCESS_TOKEN")? {
            parts.headers.insert(
                "authorization",
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| "invalid Google access token")?,
            );
        } else {
            let ctx = transport.context(values);
            let credential = if let Some(key) = values.secret("PIXSTRIDE_ENV_GCP_KEY")? {
                reqsign_google::StaticCredentialProvider::new(key)
                    .with_scope("https://www.googleapis.com/auth/cloud-platform")
                    .provide_credential(&ctx)
                    .await
            } else {
                reqsign_google::DefaultCredentialProvider::new()
                    .provide_credential(&ctx)
                    .await
            }
            .map_err(|_| "Google credentials unavailable")?
            .ok_or("Google credentials unavailable")?;
            reqsign_google::RequestSigner::new("secretmanager")
                .with_scope("https://www.googleapis.com/auth/cloud-platform")
                .sign_request(&ctx, &mut parts, Some(&credential), None)
                .await
                .map_err(|_| "cannot authorize Google secret access")?;
        }
        let result = response(
            transport
                .send(http::Request::from_parts(parts, body))
                .await?,
        )?;
        let payload = result
            .get("payload")
            .ok_or("invalid Google secret payload")?;
        let encoded = payload
            .get("data")
            .and_then(Value::as_str)
            .ok_or("invalid Google secret data")?;
        let data =
            crate::signature::decode_base64url(&encoded.replace('+', "-").replace('/', "_"), true)?;
        if let Some(crc) = payload.get("dataCrc32c") {
            let expected = crc
                .as_str()
                .and_then(|v| v.parse::<u32>().ok())
                .or_else(|| crc.as_u64().and_then(|v| u32::try_from(v).ok()))
                .ok_or("invalid secret checksum")?;
            if expected != crc32c(&data) {
                return Err("secret checksum mismatch");
            }
        }
        values.document(&data)?;
    }
    Ok(())
}
pub(super) async fn aws(
    values: &Values,
    transport: &Transport,
    service: &str,
    target: &str,
    document: Value,
) -> Result<Value, Error> {
    let (region_name, endpoint_name) = if service == "ssm" {
        (
            "PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_REGION",
            "PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_ENDPOINT",
        )
    } else if service == "kms" {
        ("PIXSTRIDE_S3_KMS_REGION", "PIXSTRIDE_S3_KMS_ENDPOINT")
    } else {
        (
            "PIXSTRIDE_ENV_AWS_SECRET_REGION",
            "PIXSTRIDE_ENV_AWS_SECRET_ENDPOINT",
        )
    };
    let region = values
        .get(region_name)
        .or_else(|| {
            if service == "kms" {
                values.get("PIXSTRIDE_S3_REGION")
            } else {
                None
            }
        })
        .or_else(|| values.get("AWS_REGION"))
        .or_else(|| values.get("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|| {
            if service == "kms" {
                "us-east-1".into()
            } else {
                "us-west-1".into()
            }
        });
    if region.is_empty()
        || region.len() > 64
        || !region
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid cloud configuration region");
    }
    let domain = if region.starts_with("cn-") {
        "amazonaws.com.cn"
    } else {
        "amazonaws.com"
    };
    let endpoint = values
        .get(endpoint_name)
        .unwrap_or_else(|| format!("https://{service}.{region}.{domain}/"));
    let body = serde_json::to_vec(&document).map_err(|_| "cannot encode configuration request")?;
    let mut hash = crate::signature::Sha256::new();
    hash.update(&body);
    let digest = hash
        .finish()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let (mut parts, body) = http::Request::post(endpoint)
        .header("content-type", "application/x-amz-json-1.1")
        .header("x-amz-target", target)
        .header("x-amz-content-sha256", digest)
        .body(Bytes::from(body))
        .map_err(|_| "invalid AWS configuration request")?
        .into_parts();
    let ctx = transport.context(values);
    let credential = if service == "kms" {
        crate::network::aws_identity::load(&region, true).await?
    } else {
        let (key, secret, token) = {
            (
                values.secret("AWS_ACCESS_KEY_ID")?,
                values.secret("AWS_SECRET_ACCESS_KEY")?,
                values.secret("AWS_SESSION_TOKEN")?,
            )
        };
        let credential = match (key, secret) {
            (Some(key), Some(secret)) => {
                let mut provider = reqsign_aws_v4::StaticCredentialProvider::new(&key, &secret);
                if let Some(token) = token {
                    provider = provider.with_session_token(&token);
                }
                provider.provide_credential(&ctx).await
            }
            (None, None) => {
                reqsign_aws_v4::DefaultCredentialProvider::new()
                    .provide_credential(&ctx)
                    .await
            }
            _ => return Err("incomplete AWS configuration credentials"),
        }
        .map_err(|_| "AWS credentials unavailable")?
        .ok_or("AWS credentials unavailable")?;
        credential
    };
    reqsign_aws_v4::RequestSigner::new(service, &region)
        .sign_request(&ctx, &mut parts, Some(&credential), None)
        .await
        .map_err(|_| "cannot authorize AWS configuration request")?;
    response(
        transport
            .send(http::Request::from_parts(parts, body))
            .await?,
    )
}
fn response(result: http::Response<Bytes>) -> Result<Value, Error> {
    if !result.status().is_success() {
        return Err("cloud configuration service refused access");
    }
    serde_json::from_slice(result.body()).map_err(|_| "invalid cloud configuration response")
}
fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82f63b78 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
#[cfg(test)]
mod tests {
    #[test]
    fn castagnoli_check_vector() {
        assert_eq!(super::crc32c(b"123456789"), 0xe3069283);
    }
}
