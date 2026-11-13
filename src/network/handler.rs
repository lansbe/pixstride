//! One authenticated request: source admission, processing, HTTP validators and response.
use crate::{fetch, local::LocalRoot, pipeline, protocol, request};
use std::time::{Duration, Instant};

const MAX_SOURCE: usize = 64 * 1024 * 1024;
pub(crate) enum Source {
    Local(crate::Buffer),
    Remote(Vec<u8>),
}
impl Source {
    fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Local(b) => b.as_mut_slice(),
            Self::Remote(b) => b,
        }
    }
}
// A short-lived stack value avoids a per-request heap allocation.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Route {
    Health,
    Favicon,
    Metrics,
    Image(std::result::Result<request::Request, request::Error>),
}
pub(super) fn handle(
    socket: &mut dyn super::socket::Socket,
    port: u16,
    root: &Option<LocalRoot>,
    config: &request::Config,
    watermark: &crate::watermark::Config,
    pool: &fetch::Pool,
    engine: &super::engine::Engine,
    settings: &super::settings::Settings,
) {
    let _metrics = super::metrics::begin();
    let _ = socket.set_nodelay(true);
    let _ = socket.set_write_timeout(Some(Duration::from_secs(15)));
    let (route, context) = match protocol::request_with(
        socket,
        port,
        config.requires_local_host(),
        |target, context| {
            let target = target.strip_prefix(&settings.path_prefix).unwrap_or("");
            let route = match target {
                path if path == settings.health_path
                    || path == "/health" && settings.health_path.is_empty() =>
                {
                    Route::Health
                }
                "/favicon.ico" => Route::Favicon,
                "/metrics" => Route::Metrics,
                _ => Route::Image(request::parse_with_hints(
                    target,
                    config,
                    if settings.client_hints {
                        context.hint_defaults()
                    } else {
                        (None, None)
                    },
                )),
            };
            (route, context)
        },
    ) {
        Ok(result) => result,
        Err(_) => {
            let _ = protocol::text(socket, 400, "Invalid HTTP request");
            return;
        }
    };
    protocol::request_keep_alive(
        !context
            .connection
            .split(',')
            .any(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "close" | "upgrade")),
    );
    if context.preflight {
        match context.preflight_headers(settings.origin.as_deref()) {
            Ok(headers) => {
                let _ = protocol::image_response(
                    socket,
                    204,
                    "text/plain",
                    &headers,
                    b"",
                    "no-store",
                    true,
                );
            }
            Err(error) => {
                let _ = failure(socket, false, 403, error);
            }
        }
        return;
    }
    if !context.authorize(settings.secret.as_deref()) {
        let _ = protocol::image_response(
            socket,
            403,
            "text/plain",
            "WWW-Authenticate: Bearer\r\n",
            b"Forbidden",
            "no-store",
            context.head,
        );
        return;
    }
    super::telemetry::parent(&context.traceparent);
    let mut request = match route {
        Route::Health => {
            let _ = protocol::image_response(
                socket,
                200,
                "text/plain",
                "",
                b"ok\n",
                "no-store",
                context.head,
            );
            return;
        }
        Route::Metrics => {
            let _ = protocol::image_response(
                socket,
                200,
                "text/plain; version=0.0.4",
                "",
                super::metrics::render().as_bytes(),
                "no-store",
                context.head,
            );
            return;
        }
        Route::Favicon => {
            let _ = failure(socket, context.head, 204, "");
            return;
        }
        Route::Image(Ok(r)) => r,
        Route::Image(Err(e)) => {
            let _ = failure(
                socket,
                context.head,
                if matches!(e, "invalid signature" | "URL security options are disabled") {
                    403
                } else {
                    404
                },
                e,
            );
            return;
        }
    };
    let vary = context.negotiate(&mut request, &settings.auto_formats)
        | context.enforce(&mut request, &settings.forced_formats);
    let mut fallback_status = None;
    if request
        .watermark
        .as_ref()
        .is_some_and(|spec| spec.opacity > 0.0)
        && !watermark.is_configured()
    {
        let _ = failure(
            socket,
            context.head,
            422,
            "watermark image is not configured",
        );
        return;
    }
    let started = Instant::now();
    let (mut source, transfers, upstream, new_connections, modified) =
        if request.source.starts_with("local:///") {
            match root
                .as_ref()
                .ok_or_else(|| "local sources are disabled".to_string())
                .and_then(|root| root.read_with_modified(&request.source))
            {
                Ok((b, modified)) => (Source::Local(b), 0, 0, 0, modified),
                Err(e) => {
                    if let Some((loaded, code)) = fallback(settings, 422) {
                        fallback_status = Some(code);
                        loaded
                    } else {
                        let _ = failure(socket, context.head, 422, &e);
                        return;
                    }
                }
            }
        } else {
            let download = if ["s3://", "gs://", "gcs://", "abs://", "swift://"]
                .iter()
                .any(|p| request.source.starts_with(p))
            {
                super::cloud::fetch(
                    &request.source,
                    MAX_SOURCE.min(request.extra.max_source_bytes),
                )
                .map_err(str::to_owned)
            } else {
                pool.fetch(
                    &request.source,
                    MAX_SOURCE.min(request.extra.max_source_bytes),
                )
            };
            match download {
                Ok(d) => (
                    Source::Remote(d.body),
                    d.transfers,
                    d.upstream_bytes,
                    d.new_connections,
                    d.last_modified
                        .and_then(|s| httpdate::parse_http_date(&s).ok()),
                ),
                Err(_) => {
                    if let Some((loaded, code)) = fallback(settings, 502) {
                        fallback_status = Some(code);
                        loaded
                    } else {
                        let _ = failure(
                            socket,
                            context.head,
                            502,
                            "Unable to retrieve the complete public HTTP(S) source",
                        );
                        return;
                    }
                }
            }
        };
    if source.bytes_mut().len() > request.extra.max_source_bytes {
        let _ = failure(
            socket,
            context.head,
            422,
            "source exceeds requested byte limit",
        );
        return;
    }
    let fetch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let queued = Instant::now();
    let lock = match engine.acquire() {
        Ok(l) => l,
        Err(_) => {
            let _ = failure(socket, context.head, 503, "Image worker unavailable");
            return;
        }
    };
    let queue_ms = queued.elapsed().as_secs_f64() * 1000.0;
    let processing = Instant::now();
    let output = pipeline::process(source.bytes_mut(), &request, watermark);
    let process_ms = processing.elapsed().as_secs_f64() * 1000.0;
    drop(lock);
    drop(source);
    match output {
        Ok(out) => {
            let mut extra=format!("Server-Timing: fetch;dur={fetch_ms:.3}, queue;dur={queue_ms:.3}, engine;dur={process_ms:.3}\r\nX-Image-Engine: pixstride\r\nX-Upstream-Transfers: {transfers}\r\nX-Upstream-Bytes: {upstream}\r\nX-Image-Width: {}\r\nX-Image-Height: {}\r\n",out.width,out.height);
            if request.source.starts_with("local:///")
                || request.source.starts_with("http://")
                || request.source.starts_with("https://")
            {
                extra.push_str(&format!(
                    "X-Upstream-New-Connections: {new_connections}\r\n"
                ));
            }
            let mut varies = Vec::new();
            if vary {
                varies.push("Accept");
            }
            if settings.client_hints {
                varies.extend(["Sec-CH-DPR", "DPR", "Sec-CH-Width", "Width"]);
                extra.push_str(&format!("Content-DPR: {}\r\n", request.extra.dpr));
            }
            if !varies.is_empty() {
                extra.push_str(&format!("Vary: {}\r\n", varies.join(", ")));
            }
            if settings.canonical
                && (request.source.starts_with("http://") || request.source.starts_with("https://"))
            {
                extra.push_str(&format!(
                    "Link: <{}>; rel=\"canonical\"\r\n",
                    request.source.replace(['<', '>', '\r', '\n'], "")
                ));
            }
            if let Some(origin) = &settings.origin {
                extra.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
            }
            extra.push_str(&super::headers::disposition(&request, out.mime));
            let mut status = fallback_status.unwrap_or(200);
            if settings.etag {
                let etag =
                    super::headers::etag_with_buster(out.body.as_slice(), &settings.etag_buster);
                extra.push_str(&format!("ETag: {etag}\r\n"));
                if status == 200 && super::headers::matches_etag(&context.if_none_match, &etag) {
                    status = 304;
                }
            }
            if settings.last_modified && fallback_status.is_none() {
                if let Some(modified) = modified.filter(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .is_ok_and(|d| d.as_secs() <= 253_402_300_799)
                }) {
                    let floor =
                        std::time::UNIX_EPOCH + Duration::from_secs(settings.modified_buster);
                    let modified = modified.max(floor);
                    extra.push_str(&format!(
                        "Last-Modified: {}\r\n",
                        httpdate::fmt_http_date(modified)
                    ));
                    if context.if_none_match.is_empty()
                        && httpdate::parse_http_date(&context.if_modified_since).is_ok_and(
                            |since| {
                                modified
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs()
                                    <= since
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                            },
                        )
                    {
                        status = 304;
                    }
                }
            }
            let mut ttl = if fallback_status.is_some() {
                settings.fallback_ttl
            } else {
                settings.ttl
            };
            if let Some(expiry) = request.extra.expires {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                ttl = ttl.min(expiry.saturating_sub(now));
            }
            let cache = if ttl == 0 {
                "no-store".into()
            } else {
                format!("public, max-age={ttl}")
            };
            let body = if status == 304 {
                &[][..]
            } else {
                out.body.as_slice()
            };
            let _ = protocol::image_response(
                socket,
                status,
                out.mime,
                &extra,
                body,
                &cache,
                context.head,
            );
        }
        Err(e) => {
            let _ = failure(socket, context.head, 422, e);
        }
    }
}

type Loaded = (Source, u32, u64, u64, Option<std::time::SystemTime>);
fn fallback(settings: &super::settings::Settings, error: u16) -> Option<(Loaded, u16)> {
    let data = settings.fallback.as_ref()?.encoded().ok()?;
    Some((
        (Source::Remote(data), 0, 0, 0, None),
        if settings.fallback_code == 0 {
            error
        } else {
            settings.fallback_code
        },
    ))
}

fn failure(
    socket: &mut dyn super::socket::Socket,
    head: bool,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    protocol::image_response(
        socket,
        status,
        "text/plain",
        "",
        body.as_bytes(),
        "no-store",
        head,
    )
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod http_tests;
