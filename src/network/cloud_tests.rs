//! SDK integration tests with synthetic credentials and an in-memory HTTP peer.
use opendal::{
    services, Buffer, HttpBody, HttpTransport, HttpTransporter, OperationContext, Operator,
};
use std::sync::{Arc, Mutex};
const DATA: &[u8] = b"synthetic-image";

#[derive(Clone)]
struct Peer {
    kind: &'static str,
    calls: Arc<Mutex<Vec<(String, bool)>>>,
}
impl HttpTransport for Peer {
    async fn fetch(&self, r: http::Request<Buffer>) -> opendal::Result<http::Response<HttpBody>> {
        let method = r.method().as_str();
        let stat = method == "HEAD"
            || (self.kind == "gcs" && !r.uri().query().unwrap_or("").contains("alt=media"));
        let auth = match self.kind {
            "s3" => r
                .headers()
                .get("authorization")
                .is_some_and(|h| h.as_bytes().starts_with(b"AWS4-HMAC-SHA256")),
            "gcs" => r
                .headers()
                .get("authorization")
                .is_some_and(|h| h.as_bytes() == b"Bearer fixture-token"),
            "abs" => r
                .headers()
                .get("authorization")
                .is_some_and(|h| h.as_bytes().starts_with(b"SharedKey ")),
            _ => r
                .headers()
                .get("x-auth-token")
                .is_some_and(|h| h.as_bytes() == b"fixture-token"),
        };
        self.calls.lock().unwrap().push((method.into(), auth));
        let data = if stat && self.kind == "gcs" {
            format!("{{\"size\":\"{}\",\"contentType\":\"image/png\",\"etag\":\"fixture\",\"generation\":\"1\",\"updated\":\"2026-01-01T00:00:00Z\"}}",DATA.len()).into_bytes()
        } else if stat {
            Vec::new()
        } else {
            DATA.to_vec()
        };
        let size = data.len() as u64;
        let mut out = http::Response::builder()
            .status(if stat { 200 } else { 206 })
            .header("etag", "\"fixture\"")
            .header("last-modified", "Thu, 01 Jan 2026 00:00:00 GMT")
            .header(
                "content-length",
                if stat && self.kind != "gcs" {
                    DATA.len()
                } else {
                    data.len()
                },
            )
            .header(
                "content-type",
                if stat && self.kind == "gcs" {
                    "application/json"
                } else {
                    "image/png"
                },
            );
        if self.kind == "abs" {
            out = out.header("x-ms-blob-type", "BlockBlob");
        }
        if !stat {
            out = out.header(
                "content-range",
                format!("bytes 0-{}/{}", DATA.len() - 1, DATA.len()),
            );
        }
        Ok(out
            .body(HttpBody::new(
                futures_util::stream::iter([Ok(Buffer::from(data))]),
                Some(size),
            ))
            .unwrap())
    }
}

#[test]
fn all_four_providers_sign_stat_and_read_with_the_injected_transport() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for kind in ["s3", "gcs", "abs", "swift"] {
            let operator = match kind {
                "s3" => Operator::new(
                    services::S3::default()
                        .bucket("images")
                        .region("us-east-1")
                        .endpoint("https://s3.example.test")
                        .access_key_id("fixture-key")
                        .secret_access_key("fixture-secret"),
                ),
                "gcs" => Operator::new(
                    services::Gcs::default()
                        .bucket("images")
                        .endpoint("https://storage.example.test")
                        .token("fixture-token".into()),
                ),
                "abs" => Operator::new(
                    services::Azblob::default()
                        .container("images")
                        .account_name("fixture")
                        .account_key("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                        .endpoint("https://fixture.example.test"),
                ),
                _ => Operator::new(
                    services::Swift::default()
                        .container("images")
                        .endpoint("https://swift.example.test/v1/AUTH_fixture")
                        .token("fixture-token"),
                ),
            }
            .unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let operator = operator.with_context(OperationContext::new().with_http_transport(
                HttpTransporter::new(Peer {
                    kind,
                    calls: Arc::clone(&calls),
                }),
            ));
            let stat = operator.stat("sample.png").await.unwrap();
            assert_eq!(stat.content_length(), DATA.len() as u64, "{kind}");
            let bytes = operator
                .read_with("sample.png")
                .range(0..DATA.len() as u64)
                .if_match(stat.etag().unwrap())
                .await
                .unwrap();
            assert_eq!(bytes.to_vec(), DATA, "{kind}");
            let calls = calls.lock().unwrap();
            assert!(
                calls.len() >= 2 && calls.iter().all(|(_, auth)| *auth),
                "unsigned {kind} SDK request"
            );
        }
    });
}

#[test]
fn transport_rejects_private_literals_and_unsupported_protocols_before_connecting() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let transport = super::cloud_transport::Transport::new(1024).unwrap();
        for url in [
            "https://127.0.0.1/x",
            "https://10.0.0.1/x",
            "https://169.254.169.254/x",
            "https://[::1]/x",
            "file:///tmp/x",
            "http://example.test/x",
        ] {
            if let Ok(request) = http::Request::builder().uri(url).body(Buffer::new()) {
                assert!(transport.fetch(request).await.is_err(), "{url}");
            }
        }
    });
}
