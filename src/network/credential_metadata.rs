//! Only SDK identity paths can opt in to canonical private metadata services.
use super::fetch::Url;
pub(super) fn enabled() -> bool {
    matches!(
        std::env::var("PIXSTRIDE_ALLOW_METADATA_CREDENTIALS").as_deref(),
        Ok("1" | "true")
    )
}
pub(super) fn permits(url: &Url, method: &http::Method, headers: &http::HeaderMap) -> bool {
    permits_with(enabled(), url, method, headers)
}
fn permits_with(
    enabled: bool,
    url: &Url,
    method: &http::Method,
    headers: &http::HeaderMap,
) -> bool {
    if !enabled || url.scheme != "http" || url.port != 80 {
        return false;
    }
    let path = url.path.split('?').next().unwrap_or("");
    match url.host.as_str() {
        "169.254.169.254" => {
            (method == "PUT"
                && path == "/latest/api/token"
                && headers.contains_key("x-aws-ec2-metadata-token-ttl-seconds"))
                || (method == "GET"
                    && path.starts_with("/latest/meta-data/iam/security-credentials/")
                    && headers.contains_key("x-aws-ec2-metadata-token"))
                || (method == "GET"
                    && path == "/metadata/identity/oauth2/token"
                    && headers.get("metadata").is_some_and(|v| v == "true"))
                || google(path, method, headers)
        }
        "metadata.google.internal" => google(path, method, headers),
        "169.254.170.2" => method == "GET" && path.starts_with("/v2/credentials/"),
        _ => false,
    }
}
fn google(path: &str, method: &http::Method, headers: &http::HeaderMap) -> bool {
    method == "GET"
        && path.starts_with("/computeMetadata/v1/instance/service-accounts/")
        && path.ends_with("/token")
        && headers
            .get("metadata-flavor")
            .is_some_and(|v| v == "Google")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_metadata_opt_in_does_not_admit_arbitrary_private_http_images() {
        let u =
            Url::parse("http://169.254.169.254/latest/meta-data/iam/security-credentials/fixture")
                .unwrap();
        let mut h = http::HeaderMap::new();
        h.insert(
            "x-aws-ec2-metadata-token",
            http::HeaderValue::from_static("synthetic"),
        );
        assert!(!permits_with(false, &u, &http::Method::GET, &h));
        assert!(permits_with(true, &u, &http::Method::GET, &h));
        assert!(!permits_with(
            true,
            &Url::parse("http://169.254.169.254/image.png").unwrap(),
            &http::Method::GET,
            &h
        ));
        assert!(!permits_with(
            true,
            &Url::parse("http://127.0.0.1/latest/meta-data/iam/security-credentials/fixture")
                .unwrap(),
            &http::Method::GET,
            &h
        ));
    }
}
