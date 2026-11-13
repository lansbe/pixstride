//! Keystone v3 password exchange. A fresh scoped token is obtained per request.
use crate::{config, Error};
use opendal::{Buffer, HttpTransport};

pub(super) async fn token(transport: &impl HttpTransport) -> Result<String, Error> {
    let endpoint = std::env::var("PIXSTRIDE_SWIFT_AUTH_URL")
        .map_err(|_| "Swift token or Keystone auth URL required")?;
    let username = config::secret("PIXSTRIDE_SWIFT_USERNAME")?.ok_or("Swift username required")?;
    let password = config::secret("PIXSTRIDE_SWIFT_PASSWORD")?.ok_or("Swift password required")?;
    let project = std::env::var("PIXSTRIDE_SWIFT_PROJECT").map_err(|_| "Swift project required")?;
    let domain = std::env::var("PIXSTRIDE_SWIFT_DOMAIN").unwrap_or_else(|_| "Default".into());
    exchange(
        transport, &endpoint, &username, &password, &project, &domain,
    )
    .await
}

async fn exchange(
    transport: &impl HttpTransport,
    endpoint: &str,
    username: &str,
    password: &str,
    project: &str,
    domain: &str,
) -> Result<String, Error> {
    let payload=serde_json::json!({"auth":{"identity":{"methods":["password"],"password":{"user":{"name":username,"password":password,"domain":{"name":domain}}}},"scope":{"project":{"name":project,"domain":{"name":domain}}}}}).to_string();
    if payload.len() > 65536 {
        return Err("Keystone credentials exceed limit");
    }
    let request = http::Request::builder()
        .method("POST")
        .uri(endpoint)
        .header("content-type", "application/json")
        .body(Buffer::from(payload.into_bytes()))
        .map_err(|_| "invalid Keystone request")?;
    let response = transport
        .fetch(request)
        .await
        .map_err(|_| "Keystone authentication failed")?;
    if response.status() != http::StatusCode::CREATED {
        return Err("Keystone authentication failed");
    }
    let token = response
        .headers()
        .get("x-subject-token")
        .and_then(|h| h.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 8192)
        .ok_or("invalid Keystone token")?;
    Ok(token.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Peer(AtomicUsize);
    impl HttpTransport for Peer {
        async fn fetch(
            &self,
            r: http::Request<Buffer>,
        ) -> opendal::Result<http::Response<opendal::HttpBody>> {
            assert_eq!(r.method(), "POST");
            assert_eq!(r.uri(), "https://identity.example.test/v3/auth/tokens");
            let data: serde_json::Value = serde_json::from_slice(&r.body().to_vec()).unwrap();
            assert_eq!(
                data["auth"]["identity"]["password"]["user"]["name"],
                "fixture-user"
            );
            assert_eq!(data["auth"]["scope"]["project"]["name"], "fixture-project");
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(http::Response::builder()
                .status(201)
                .header("x-subject-token", format!("fixture-token-{n}"))
                .body(opendal::HttpBody::new(
                    futures_util::stream::empty(),
                    Some(0),
                ))
                .unwrap())
        }
    }
    #[test]
    fn each_request_exchanges_credentials_instead_of_reusing_an_expired_token() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let peer = Peer(AtomicUsize::new(0));
            for n in 1..=2 {
                assert_eq!(
                    exchange(
                        &peer,
                        "https://identity.example.test/v3/auth/tokens",
                        "fixture-user",
                        "fixture-password",
                        "fixture-project",
                        "Default"
                    )
                    .await
                    .unwrap(),
                    format!("fixture-token-{n}")
                );
            }
        });
    }
}
