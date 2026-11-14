//! Minimal vendor error notices. No source URLs, headers, filenames or stack locals.
use super::{client, payload::Event};
use crate::Error;
use reqwest::{header::HeaderMap, Url};
use serde_json::{json, Value};
enum Vendor {
    Sentry { release: String },
    Honeybadger,
    Airbrake,
    Bugsnag { key: String },
}
pub(super) struct Destination {
    url: Url,
    headers: HeaderMap,
    vendor: Vendor,
    environment: String,
}
fn value(name: &str) -> Result<String, Error> {
    let value = crate::config::secret(name)?.unwrap_or_default();
    if value.len() > 1024 || value.chars().any(char::is_control) {
        return Err("invalid notice configuration");
    }
    Ok(value)
}
pub(super) fn destinations() -> Result<Vec<Destination>, Error> {
    let mut result = Vec::new();
    if let Some(dsn) = crate::config::secret("PIXSTRIDE_SENTRY_DSN")? {
        let mut url = Url::parse(&dsn).map_err(|_| "invalid Sentry DSN")?;
        let key = url.username().to_owned();
        let mut segments = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
        let project = segments.pop().unwrap_or("").to_owned();
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric())
            || project.is_empty()
            || !project.bytes().all(|b| b.is_ascii_digit())
            || url.password().is_some()
        {
            return Err("invalid Sentry DSN");
        }
        let path = format!("/{}/api/{project}/envelope/", segments.join("/")).replace("//", "/");
        url.set_path(&path);
        url.set_username("").map_err(|_| "invalid Sentry DSN")?;
        let url = client::url(url.as_str())?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-sentry-auth",
            client::header(&format!("Sentry sentry_version=7, sentry_key={key}"))?,
        );
        result.push(Destination {
            url,
            headers,
            vendor: Vendor::Sentry {
                release: value("PIXSTRIDE_SENTRY_RELEASE")?,
            },
            environment: value("PIXSTRIDE_SENTRY_ENVIRONMENT")?,
        });
    }
    for (key_name, endpoint, default, environment, vendor_name) in [
        (
            "PIXSTRIDE_HONEYBADGER_KEY",
            "PIXSTRIDE_HONEYBADGER_ENDPOINT",
            "https://api.honeybadger.io/v1/notices",
            "PIXSTRIDE_HONEYBADGER_ENV",
            0,
        ),
        (
            "PIXSTRIDE_BUGSNAG_KEY",
            "PIXSTRIDE_BUGSNAG_ENDPOINT",
            "https://notify.bugsnag.com",
            "PIXSTRIDE_BUGSNAG_STAGE",
            1,
        ),
        (
            "PIXSTRIDE_AIRBRAKE_PROJECT_KEY",
            "PIXSTRIDE_AIRBRAKE_ENDPOINT",
            "https://api.airbrake.io",
            "PIXSTRIDE_AIRBRAKE_ENVIRONMENT",
            2,
        ),
    ] {
        if let Some(key) = crate::config::secret(key_name)? {
            let base = std::env::var(endpoint).unwrap_or_else(|_| default.into());
            let mut url = client::url(&base)?;
            let mut headers = HeaderMap::new();
            let vendor = match vendor_name {
                0 => {
                    headers.insert("x-api-key", client::header(&key)?);
                    Vendor::Honeybadger
                }
                1 => {
                    headers.insert("bugsnag-api-key", client::header(&key)?);
                    headers.insert("bugsnag-payload-version", client::header("5")?);
                    Vendor::Bugsnag { key }
                }
                _ => {
                    let id = value("PIXSTRIDE_AIRBRAKE_PROJECT_ID")?;
                    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                        return Err("invalid Airbrake project ID");
                    }
                    url.set_path(&format!(
                        "{}/api/v3/projects/{id}/notices",
                        url.path().trim_end_matches('/')
                    ));
                    url.query_pairs_mut().append_pair("key", &key);
                    Vendor::Airbrake
                }
            };
            result.push(Destination {
                url,
                headers,
                vendor,
                environment: value(environment)?,
            });
        }
    }
    Ok(result)
}
impl Destination {
    pub fn request(
        &self,
        client: &reqwest::Client,
        event: &Event,
        id: &str,
    ) -> reqwest::RequestBuilder {
        let notifier = json!({"name":"pixstride","version":env!("CARGO_PKG_VERSION"),"url":"https://github.com/lansbe/pixstride"});
        let message = format!("Image request failed with HTTP {}", event.status);
        let body: Value = match &self.vendor {
            Vendor::Sentry { release, .. } => {
                json!({"event_id":id,"platform":"native","level":"error","message":message,"release":release,"environment":self.environment,"tags":{"http.status_code":event.status}})
            }
            Vendor::Honeybadger => {
                json!({"notifier":notifier,"error":{"class":"PixstrideError","message":message,"backtrace":[]},"server":{"environment_name":self.environment},"request":{"context":{"http_status":event.status}}})
            }
            Vendor::Airbrake => {
                json!({"errors":[{"type":"PixstrideError","message":message,"backtrace":[]}],"context":{"notifier":notifier,"environment":self.environment,"language":"rust","severity":"error","version":env!("CARGO_PKG_VERSION")}})
            }
            Vendor::Bugsnag { key } => {
                json!({"apiKey":key,"notifier":notifier,"events":[{"exceptions":[{"errorClass":"PixstrideError","message":message,"stacktrace":[]}],"severity":"error","unhandled":false,"severityReason":{"type":"handledError"},"app":{"version":env!("CARGO_PKG_VERSION"),"releaseStage":self.environment},"metaData":{"http":{"status":event.status}}}]})
            }
        };
        let (content_type, body) = if let Vendor::Sentry { .. } = &self.vendor {
            let header = json!({"event_id":id});
            (
                "application/x-sentry-envelope",
                format!(
                    "{}\n{}\n{}\n",
                    header,
                    json!({"type":"event","length":body.to_string().len()}),
                    body
                ),
            )
        } else {
            ("application/json", body.to_string())
        };
        client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .header("content-type", content_type)
            .body(body)
    }
}
