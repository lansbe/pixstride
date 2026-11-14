//! Fresh Swift v1/Keystone v2/v3 credentials and explicit/canonical catalog endpoints.
use crate::{config, Error};
use opendal::{Buffer, HttpTransport};

pub(super) async fn session(transport: &impl HttpTransport) -> Result<(String, String), Error> {
    let storage = std::env::var("PIXSTRIDE_SWIFT_ENDPOINT").ok();
    if let Some(token) = config::secret("PIXSTRIDE_SWIFT_TOKEN")? {
        return Ok((token, storage.ok_or("Swift endpoint required")?));
    }
    let endpoint = std::env::var("PIXSTRIDE_SWIFT_AUTH_URL")
        .map_err(|_| "Swift token or Keystone auth URL required")?;
    let username = config::secret("PIXSTRIDE_SWIFT_USERNAME")?.ok_or("Swift username required")?;
    let password = config::secret("PIXSTRIDE_SWIFT_PASSWORD")?
        .or(config::secret("PIXSTRIDE_SWIFT_API_KEY")?)
        .ok_or("Swift password or API key required")?;
    let project = std::env::var("PIXSTRIDE_SWIFT_PROJECT")
        .or_else(|_| std::env::var("PIXSTRIDE_SWIFT_TENANT"))
        .ok();
    let domain = std::env::var("PIXSTRIDE_SWIFT_DOMAIN").unwrap_or_else(|_| "Default".into());
    let user_domain =
        std::env::var("PIXSTRIDE_SWIFT_USER_DOMAIN").unwrap_or_else(|_| domain.clone());
    let project_domain = std::env::var("PIXSTRIDE_SWIFT_PROJECT_DOMAIN").unwrap_or(domain);
    let version = std::env::var("PIXSTRIDE_SWIFT_AUTH_VERSION")
        .unwrap_or_else(|_| "0".into())
        .parse::<u8>()
        .map_err(|_| "invalid Swift auth version")?;
    let version = if version == 0 {
        if endpoint.contains("/v3") {
            3
        } else if endpoint.contains("/v2") {
            2
        } else {
            1
        }
    } else {
        version
    };
    let auth = Credentials {
        username,
        password,
        project,
        user_domain,
        project_domain,
        version,
        region: std::env::var("PIXSTRIDE_SWIFT_REGION").ok(),
    };
    let (token, discovered) = perform(transport, &endpoint, &auth).await?;
    Ok((
        token,
        storage
            .or(discovered)
            .ok_or("Swift catalog contains no public object endpoint")?,
    ))
}
struct Credentials {
    username: String,
    password: String,
    project: Option<String>,
    user_domain: String,
    project_domain: String,
    version: u8,
    region: Option<String>,
}
async fn perform(
    transport: &impl HttpTransport,
    endpoint: &str,
    auth: &Credentials,
) -> Result<(String, Option<String>), Error> {
    let mut endpoint = endpoint.trim_end_matches('/').to_owned();
    if auth.version == 3 && !endpoint.ends_with("/auth/tokens") {
        endpoint.push_str("/auth/tokens");
    }
    if auth.version == 2 && !endpoint.ends_with("/tokens") {
        endpoint.push_str("/tokens");
    }
    let mut request = http::Request::builder().uri(&endpoint);
    let payload = match auth.version {
        1 => {
            request = request
                .method("GET")
                .header("x-auth-user", &auth.username)
                .header("x-auth-key", &auth.password);
            String::new()
        }
        2 => {
            request = request
                .method("POST")
                .header("content-type", "application/json");
            let mut v = serde_json::json!({"auth":{"passwordCredentials":{"username":auth.username,"password":auth.password}}});
            if let Some(project) = &auth.project {
                v["auth"]["tenantName"] = project.clone().into();
            }
            v.to_string()
        }
        3 => {
            request = request
                .method("POST")
                .header("content-type", "application/json");
            let mut v = serde_json::json!({"auth":{"identity":{"methods":["password"],"password":{"user":{"name":auth.username,"password":auth.password,"domain":{"name":auth.user_domain}}}}}});
            if let Some(project) = &auth.project {
                v["auth"]["scope"] = serde_json::json!({"project":{"name":project,"domain":{"name":auth.project_domain}}});
            }
            v.to_string()
        }
        _ => return Err("Swift auth version must be 0..3"),
    };
    if payload.len() > 65536 {
        return Err("Keystone credentials exceed limit");
    }
    let request = request
        .body(Buffer::from(payload.into_bytes()))
        .map_err(|_| "invalid Keystone request")?;
    let mut response = transport
        .fetch(request)
        .await
        .map_err(|_| "Keystone authentication failed")?;
    if !response.status().is_success() {
        return Err("Keystone authentication failed");
    }
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty() && v.len() <= 8192)
            .map(str::to_owned)
    };
    if auth.version == 1 {
        return Ok((
            header("x-auth-token")
                .or_else(|| header("x-storage-token"))
                .ok_or("invalid Swift token")?,
            header("x-storage-url"),
        ));
    }
    let token = header("x-subject-token");
    let data = response
        .body_mut()
        .to_buffer()
        .await
        .map_err(|_| "cannot read Keystone catalog")?
        .to_vec();
    if data.len() > 1024 * 1024 {
        return Err("Keystone catalog exceeds limit");
    }
    let data: serde_json::Value = if data.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&data).map_err(|_| "invalid Keystone catalog")?
    };
    let token = if auth.version == 2 {
        data["access"]["token"]["id"].as_str().map(str::to_owned)
    } else {
        token
    }
    .filter(|v| !v.is_empty() && v.len() <= 8192)
    .ok_or("invalid Keystone token")?;
    let catalog = if auth.version == 2 {
        &data["access"]["serviceCatalog"]
    } else {
        &data["token"]["catalog"]
    };
    let mut storage = None;
    if let Some(services) = catalog.as_array() {
        for service in services.iter().filter(|s| s["type"] == "object-store") {
            if let Some(endpoints) = service["endpoints"].as_array() {
                for endpoint in endpoints {
                    if auth
                        .region
                        .as_ref()
                        .is_some_and(|r| endpoint["region"].as_str() != Some(r))
                    {
                        continue;
                    }
                    let url = if auth.version == 2 {
                        endpoint["publicURL"].as_str()
                    } else if endpoint["interface"] == "public" {
                        endpoint["url"].as_str()
                    } else {
                        None
                    };
                    if let Some(url) = url.filter(|v| !v.is_empty() && v.len() <= 2048) {
                        storage = Some(url.into());
                        break;
                    }
                }
            }
            if storage.is_some() {
                break;
            }
        }
    }
    Ok((token, storage))
}

#[cfg(test)]
async fn exchange(
    transport: &impl HttpTransport,
    endpoint: &str,
    username: &str,
    password: &str,
    project: &str,
    domain: &str,
) -> Result<String, Error> {
    perform(
        transport,
        endpoint,
        &Credentials {
            username: username.into(),
            password: password.into(),
            project: Some(project.into()),
            user_domain: domain.into(),
            project_domain: domain.into(),
            version: 3,
            region: None,
        },
    )
    .await
    .map(|v| v.0)
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
    struct Catalog(u8);
    impl HttpTransport for Catalog {
        async fn fetch(
            &self,
            r: http::Request<Buffer>,
        ) -> opendal::Result<http::Response<opendal::HttpBody>> {
            let storage = "https://storage.example.test/v1/AUTH_fixture";
            let mut response =
                http::Response::builder().status(if self.0 == 3 { 201 } else { 200 });
            let body=match self.0 {
                1 => {
                    assert_eq!(r.method(),"GET");assert_eq!(r.headers()["x-auth-user"],"fixture-user");assert_eq!(r.headers()["x-auth-key"],"fixture-password");
                    response=response.header("x-auth-token","synthetic-catalog-token").header("x-storage-url",storage);String::new()
                }
                2 => {
                    assert!(r.uri().path().ends_with("/v2/tokens"));
                    let data:serde_json::Value=serde_json::from_slice(&r.body().to_vec()).unwrap();assert_eq!(data["auth"]["tenantName"],"fixture-project");
                    serde_json::json!({"access":{"token":{"id":"synthetic-catalog-token"},"serviceCatalog":[{"type":"object-store","endpoints":[{"region":"fixture-region","publicURL":storage}]}]}}).to_string()
                }
                _ => {
                    assert!(r.uri().path().ends_with("/v3/auth/tokens"));
                    let data:serde_json::Value=serde_json::from_slice(&r.body().to_vec()).unwrap();
                    assert_eq!(data["auth"]["identity"]["password"]["user"]["domain"]["name"],"users");
                    assert_eq!(data["auth"]["scope"]["project"]["domain"]["name"],"projects");
                    response=response.header("x-subject-token","synthetic-catalog-token");
                    serde_json::json!({"token":{"catalog":[{"type":"object-store","endpoints":[{"interface":"internal","region":"fixture-region","url":"http://127.0.0.1/private"},{"interface":"public","region":"fixture-region","url":storage}]}]}}).to_string()
                }
            }.into_bytes();
            let length = body.len() as u64;
            Ok(response
                .body(opendal::HttpBody::new(
                    futures_util::stream::iter([Ok(Buffer::from(body))]),
                    Some(length),
                ))
                .unwrap())
        }
    }
    #[test]
    fn swift_auth_versions_discover_public_regional_endpoints_and_keep_domains_distinct() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                for version in 1..=3 {
                    let auth = Credentials {
                        username: "fixture-user".into(),
                        password: "fixture-password".into(),
                        project: Some("fixture-project".into()),
                        user_domain: "users".into(),
                        project_domain: "projects".into(),
                        version,
                        region: Some("fixture-region".into()),
                    };
                    let (token, endpoint) = perform(
                        &Catalog(version),
                        &format!("https://identity.example.test/v{version}"),
                        &auth,
                    )
                    .await
                    .unwrap();
                    assert_eq!(token, "synthetic-catalog-token");
                    assert_eq!(
                        endpoint.as_deref(),
                        Some("https://storage.example.test/v1/AUTH_fixture")
                    );
                }
            });
    }
}
