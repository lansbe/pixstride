//! Independent, bounded Prometheus listener with the service's Bearer policy.
use crate::{
    network::{lifecycle, metrics, protocol, settings::Settings},
    Error,
};
use std::{
    net::TcpListener,
    sync::Arc,
    thread::{self, JoinHandle},
};

pub(crate) struct Listener(Option<JoinHandle<()>>);
impl Listener {
    pub fn start(settings: Arc<Settings>) -> Result<Self, Error> {
        let Ok(bind) = std::env::var("PIXSTRIDE_PROMETHEUS_BIND") else {
            return Ok(Self(None));
        };
        let address = bind
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "invalid Prometheus address")?;
        let namespace =
            std::env::var("PIXSTRIDE_PROMETHEUS_NAMESPACE").unwrap_or_else(|_| "pixstride".into());
        if namespace.is_empty()
            || namespace.len() > 64
            || !namespace
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || i > 0 && b.is_ascii_digit())
        {
            return Err("invalid Prometheus namespace");
        }
        let listener = TcpListener::bind(address).map_err(|_| "cannot bind Prometheus listener")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "cannot configure Prometheus listener")?;
        let worker = thread::Builder::new()
            .name("prometheus".into())
            .spawn(move || {
                while lifecycle::wait(&listener).unwrap_or(false) {
                    let Ok((mut socket, _)) = listener.accept() else {
                        continue;
                    };
                    if socket.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let request = protocol::request_with(
                        &mut socket,
                        address.port(),
                        false,
                        |target, context| (target == "/metrics", context),
                    );
                    if let Ok((valid, context)) = request {
                        let authorized = context.authorize(settings.secret.as_deref());
                        let (status, body) = if !authorized {
                            (401, "Unauthorized".into())
                        } else if !valid {
                            (404, "Not Found".into())
                        } else {
                            (
                                200,
                                metrics::render().replace("pixstride_", &format!("{namespace}_")),
                            )
                        };
                        let _ = protocol::image_response(
                            &mut socket,
                            status,
                            "text/plain; version=0.0.4",
                            "",
                            body.as_bytes(),
                            "no-store",
                            context.head,
                        );
                    }
                }
            })
            .map_err(|_| "cannot start Prometheus listener")?;
        Ok(Self(Some(worker)))
    }
    pub fn finish(mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}
