//! Signed CloudWatch Query API metrics; credentials are reloaded for each export.
use super::{client, payload::Event};
use crate::{
    signature::{hmac_sha256, Sha256},
    Error,
};
use reqwest::Url;
pub(super) struct CloudWatch {
    url: Url,
    region: String,
    namespace: String,
    service: String,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hex(&hash.finish())
}
fn escape(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn credential(name: &str, standard: &str) -> Result<Option<String>, Error> {
    match crate::config::secret(name)? {
        Some(value) => Ok(Some(value)),
        None => crate::config::secret(standard),
    }
}
impl CloudWatch {
    pub fn from_env() -> Result<Option<Self>, Error> {
        let Ok(service) = std::env::var("PIXSTRIDE_CLOUD_WATCH_SERVICE_NAME") else {
            return Ok(None);
        };
        let region = std::env::var("PIXSTRIDE_CLOUD_WATCH_REGION")
            .or_else(|_| std::env::var("AWS_REGION"))
            .unwrap_or_else(|_| "us-west-1".into());
        let namespace =
            std::env::var("PIXSTRIDE_CLOUD_WATCH_NAMESPACE").unwrap_or_else(|_| "pixstride".into());
        if service.is_empty()
            || service.len() > 255
            || namespace.is_empty()
            || namespace.len() > 255
            || service
                .chars()
                .chain(namespace.chars())
                .any(char::is_control)
            || region.len() > 64
            || region.is_empty()
            || !region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("invalid CloudWatch configuration");
        }
        let domain = if region.starts_with("cn-") {
            "amazonaws.com.cn"
        } else {
            "amazonaws.com"
        };
        let endpoint = std::env::var("PIXSTRIDE_CLOUD_WATCH_ENDPOINT")
            .unwrap_or_else(|_| format!("https://monitoring.{region}.{domain}/"));
        let url = client::url(&endpoint)?;
        if url.query().is_some() {
            return Err("CloudWatch endpoint must not have a query");
        }
        Ok(Some(Self {
            url,
            region,
            namespace,
            service,
        }))
    }
    pub async fn request(
        &self,
        client: &reqwest::Client,
        event: &Event,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let key = credential("PIXSTRIDE_CLOUD_WATCH_ACCESS_KEY_ID", "AWS_ACCESS_KEY_ID")?;
        let secret = credential(
            "PIXSTRIDE_CLOUD_WATCH_SECRET_ACCESS_KEY",
            "AWS_SECRET_ACCESS_KEY",
        )?;
        let token = credential("PIXSTRIDE_CLOUD_WATCH_SESSION_TOKEN", "AWS_SESSION_TOKEN")?;
        let (key, secret, token) = match (key, secret) {
            (Some(key), Some(secret)) => (key, secret, token),
            (None, None) => {
                #[cfg(feature = "cloud-sources")]
                {
                    let c = crate::network::aws_identity::load(&self.region, false).await?;
                    (c.access_key_id, c.secret_access_key, c.session_token)
                }
                #[cfg(not(feature = "cloud-sources"))]
                {
                    return Err("CloudWatch SDK credentials require cloud-sources");
                }
            }
            _ => return Err("CloudWatch credentials must include key and secret"),
        };
        let date = httpdate::fmt_http_date(std::time::SystemTime::now());
        let fields = date.split_whitespace().collect::<Vec<_>>();
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let month = months
            .iter()
            .position(|m| *m == fields[2])
            .ok_or("invalid CloudWatch clock")?
            + 1;
        let day = format!("{}{:02}{}", fields[3], month, fields[1]);
        let time = format!("{day}T{}Z", fields[4].replace(':', ""));
        let mut pairs = vec![
            ("Action".into(), "PutMetricData".into()),
            ("Version".into(), "2010-08-01".into()),
            ("Namespace".into(), self.namespace.clone()),
        ];
        for (index, (name, value, unit)) in [
            ("Requests", 1.0, "Count"),
            (
                "RequestDuration",
                event.duration as f64 / 1e6,
                "Milliseconds",
            ),
            ("Errors", f64::from(u8::from(event.status >= 500)), "Count"),
        ]
        .into_iter()
        .enumerate()
        {
            let prefix = format!("MetricData.member.{}", index + 1);
            for (field, value) in [
                ("MetricName", name.into()),
                ("Value", value.to_string()),
                ("Unit", unit.into()),
                ("Dimensions.member.1.Name", "ServiceName".into()),
                ("Dimensions.member.1.Value", self.service.clone()),
            ] {
                pairs.push((format!("{prefix}.{field}"), value));
            }
        }
        let body = pairs
            .into_iter()
            .map(|(k, v)| format!("{}={}", escape(&k), escape(&v)))
            .collect::<Vec<_>>()
            .join("&");
        let host = self.url.authority();
        let content_type = "application/x-www-form-urlencoded";
        let mut headers = format!("content-type:{content_type}\nhost:{host}\nx-amz-date:{time}\n");
        let mut signed = "content-type;host;x-amz-date".to_owned();
        if let Some(token) = &token {
            headers.push_str(&format!("x-amz-security-token:{}\n", token.trim()));
            signed.push_str(";x-amz-security-token");
        }
        let canonical = format!(
            "POST\n{}\n\n{headers}\n{signed}\n{}",
            self.url.path(),
            hash(body.as_bytes())
        );
        let scope = format!("{day}/{}/monitoring/aws4_request", self.region);
        let message = format!(
            "AWS4-HMAC-SHA256\n{time}\n{scope}\n{}",
            hash(canonical.as_bytes())
        );
        let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), &[day.as_bytes()]);
        let region_key = hmac_sha256(&date_key, &[self.region.as_bytes()]);
        let service_key = hmac_sha256(&region_key, &[b"monitoring"]);
        let signing_key = hmac_sha256(&service_key, &[b"aws4_request"]);
        let signature = hex(&hmac_sha256(&signing_key, &[message.as_bytes()]));
        let auth=format!("AWS4-HMAC-SHA256 Credential={key}/{scope}, SignedHeaders={signed}, Signature={signature}");
        let mut request = client
            .post(self.url.clone())
            .header("content-type", content_type)
            .header("x-amz-date", time)
            .header("authorization", client::header(&auth)?)
            .body(body);
        if let Some(token) = token {
            request = request.header("x-amz-security-token", client::header(&token)?);
        }
        Ok(request)
    }
}
