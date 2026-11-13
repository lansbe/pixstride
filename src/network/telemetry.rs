//! Opt-in bounded telemetry and vendor notices without request/source identifiers.
use crate::Error;
#[cfg(feature = "telemetry")]
mod client;
#[cfg(feature = "telemetry")]
mod cloudwatch;
#[cfg(feature = "telemetry")]
mod notices;
#[cfg(feature = "telemetry")]
mod payload;
#[cfg(feature = "telemetry")]
mod exporter {
    use super::*;
    use payload::{Event, Parent};
    use std::{
        cell::RefCell,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, SyncSender},
            Arc, OnceLock,
        },
        time::Duration,
    };
    static EVENTS: OnceLock<SyncSender<Event>> = OnceLock::new();
    static PROPAGATE: AtomicBool = AtomicBool::new(false);
    static DROPPED: AtomicU64 = AtomicU64::new(0);
    static FAILED: AtomicU64 = AtomicU64::new(0);
    thread_local! {static PARENT:RefCell<Option<Parent>>=const{RefCell::new(None)};}
    pub(crate) struct Exporter {
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Exporter {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    fn flag(name: &str) -> Result<bool, Error> {
        match std::env::var(name).as_deref() {
            Ok("1" | "true") => Ok(true),
            Ok("0" | "false") | Err(std::env::VarError::NotPresent) => Ok(false),
            _ => Err("invalid telemetry flag"),
        }
    }
    pub fn initialize() -> Result<Option<Exporter>, Error> {
        let endpoints = client::endpoints()?;
        let notices = notices::destinations()?;
        let cloudwatch = cloudwatch::CloudWatch::from_env()?;
        if endpoints.is_empty() && notices.is_empty() && cloudwatch.is_none() {
            return Ok(None);
        }
        PROPAGATE.store(
            flag("PIXSTRIDE_OTLP_PROPAGATE_EXTERNAL")?,
            Ordering::Relaxed,
        );
        let service =
            std::env::var("PIXSTRIDE_NEW_RELIC_APP_NAME").unwrap_or_else(|_| "pixstride".into());
        if service.is_empty() || service.len() > 128 || service.chars().any(char::is_control) {
            return Err("invalid telemetry service name");
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "telemetry runtime unavailable")?;
        let client = {
            let _enter = runtime.enter();
            client::client()?
        };
        let mut random = [0u8; 16];
        // SAFETY: getentropy writes at most the 16 bytes in this live array.
        // Both supported Unix platforms expose this libc API (maximum 256 bytes).
        if unsafe { libc::getentropy(random.as_mut_ptr().cast(), random.len()) } != 0 {
            return Err("telemetry entropy unavailable");
        }
        let seed = u128::from_be_bytes(random);
        let (sender, receiver) = mpsc::sync_channel::<Event>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = std::thread::Builder::new()
            .name("telemetry-exporter".into())
            .spawn(move || {
                let mut sequence = 0u64;
                while !done.load(Ordering::Acquire) {
                    let event = match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(e) => e,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    sequence = sequence.wrapping_add(1);
                    runtime.block_on(async {
                        for endpoint in &endpoints {
                            if let Some(payload) =
                                payload::build(endpoint.kind, &event, seed, sequence, &service)
                            {
                                let success = client
                                    .post(endpoint.url.clone())
                                    .headers(endpoint.headers.clone())
                                    .header("content-type", "application/json")
                                    .body(payload.to_string())
                                    .send()
                                    .await
                                    .is_ok_and(|r| r.status().is_success());
                                if !success {
                                    FAILED.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        if let Some(cloudwatch) = &cloudwatch {
                            let success = match cloudwatch.request(&client, &event) {
                                Ok(request) => {
                                    request.send().await.is_ok_and(|r| r.status().is_success())
                                }
                                Err(_) => false,
                            };
                            if !success {
                                FAILED.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        if event.status >= 500 {
                            let id = format!("{:032x}", seed.wrapping_add(u128::from(sequence)));
                            for notice in &notices {
                                if !notice
                                    .request(&client, &event, &id)
                                    .send()
                                    .await
                                    .is_ok_and(|r| r.status().is_success())
                                {
                                    FAILED.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    });
                }
            })
            .map_err(|_| "cannot start telemetry exporter")?;
        EVENTS
            .set(sender)
            .map_err(|_| "telemetry already initialized")?;
        Ok(Some(Exporter {
            stop,
            worker: Some(worker),
        }))
    }
    pub fn reset() {
        PARENT.with(|p| *p.borrow_mut() = None);
    }
    pub fn parent(value: &str) {
        if PROPAGATE.load(Ordering::Relaxed) {
            PARENT.with(|p| *p.borrow_mut() = payload::parent(value));
        }
    }
    pub fn record(start: u128, duration: u64, status: u16) {
        if let Some(sender) = EVENTS.get() {
            let parent = PARENT.with(|p| p.borrow().clone());
            if sender
                .try_send(Event {
                    start,
                    duration,
                    status,
                    parent,
                })
                .is_err()
            {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub fn counters() -> (u64, u64) {
        (
            DROPPED.load(Ordering::Relaxed),
            FAILED.load(Ordering::Relaxed),
        )
    }
}
#[cfg(feature = "telemetry")]
pub(crate) use exporter::*;
#[cfg(not(feature = "telemetry"))]
pub(crate) struct Exporter;
#[cfg(not(feature = "telemetry"))]
pub fn initialize() -> Result<Option<Exporter>, Error> {
    for name in [
        "PIXSTRIDE_OTLP_ENDPOINT",
        "PIXSTRIDE_OTLP_METRICS_ENDPOINT",
        "PIXSTRIDE_OTLP_LOGS_ENDPOINT",
        "PIXSTRIDE_SENTRY_DSN",
        "PIXSTRIDE_HONEYBADGER_KEY",
        "PIXSTRIDE_AIRBRAKE_PROJECT_KEY",
        "PIXSTRIDE_BUGSNAG_KEY",
        "PIXSTRIDE_NEW_RELIC_KEY",
        "PIXSTRIDE_DATADOG_KEY",
        "PIXSTRIDE_CLOUD_WATCH_SERVICE_NAME",
    ] {
        if crate::config::secret(name)?.is_some() {
            return Err("telemetry configuration requires telemetry feature");
        }
    }
    Ok(None)
}
#[cfg(not(feature = "telemetry"))]
pub fn reset() {}
#[cfg(not(feature = "telemetry"))]
pub fn parent(_: &str) {}
#[cfg(not(feature = "telemetry"))]
pub fn record(_: u128, _: u64, _: u16) {}
#[cfg(not(feature = "telemetry"))]
pub fn counters() -> (u64, u64) {
    (0, 0)
}
