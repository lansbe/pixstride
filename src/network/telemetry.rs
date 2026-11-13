//! Opt-in OTLP/HTTP JSON spans, bounded queue and no request identifiers.
use crate::Error;
#[cfg(feature = "telemetry")]
mod exporter {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, OnceLock,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    struct Event {
        start: u128,
        duration: u64,
        status: u16,
    }
    static EVENTS: OnceLock<SyncSender<Event>> = OnceLock::new();
    static DROPPED: AtomicU64 = AtomicU64::new(0);
    static FAILED: AtomicU64 = AtomicU64::new(0);
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
    pub fn initialize() -> Result<Option<Exporter>, Error> {
        let Some(endpoint) = std::env::var("PIXSTRIDE_OTLP_ENDPOINT").ok() else {
            return Ok(None);
        };
        let url = reqwest::Url::parse(&endpoint).map_err(|_| "invalid OTLP endpoint")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid OTLP endpoint");
        }
        let authorization = crate::config::secret("PIXSTRIDE_OTLP_AUTHORIZATION")?
            .map(|s| reqwest::header::HeaderValue::from_str(&s))
            .transpose()
            .map_err(|_| "invalid OTLP authorization")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "telemetry runtime unavailable")?;
        let client = {
            let _enter = runtime.enter();
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(1))
                .build()
                .map_err(|_| "telemetry transport unavailable")?
        };
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let (sender, receiver) = mpsc::sync_channel::<Event>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker=std::thread::Builder::new().name("otlp-exporter".into()).spawn(move||{
            let mut sequence=0u64;
            while !done.load(Ordering::Acquire) {
                let event=match receiver.recv_timeout(Duration::from_millis(100)){Ok(e)=>e,Err(mpsc::RecvTimeoutError::Timeout)=>continue,Err(_)=>break};
                sequence=sequence.wrapping_add(1);
                // Identifiers are generated locally, never derived from a URL,
                // source, user header, authorization token or signing secret.
                let trace=format!("{:032x}",seed.wrapping_add(u128::from(sequence)));
                let span=format!("{sequence:016x}");
                let payload=serde_json::json!({"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"pixstride"}},{"key":"service.version","value":{"stringValue":env!("CARGO_PKG_VERSION")}}]},"scopeSpans":[{"scope":{"name":"pixstride.http"},"spans":[{"traceId":trace,"spanId":span,"name":"image.request","kind":2,"startTimeUnixNano":event.start.to_string(),"endTimeUnixNano":(event.start+u128::from(event.duration)).to_string(),"attributes":[{"key":"http.response.status_code","value":{"intValue":event.status.to_string()}}],"status":{"code":if event.status>=500{2}else{1}}}]}]}]});
                let success=runtime.block_on(async {let mut request=client.post(url.clone()).header("Content-Type","application/json").body(payload.to_string());if let Some(auth)=&authorization{request=request.header(reqwest::header::AUTHORIZATION,auth.clone());}request.send().await.is_ok_and(|r|r.status().is_success())});
                if !success{FAILED.fetch_add(1,Ordering::Relaxed);}
            }
        }).map_err(|_|"cannot start telemetry exporter")?;
        EVENTS
            .set(sender)
            .map_err(|_| "telemetry already initialized")?;
        Ok(Some(Exporter {
            stop,
            worker: Some(worker),
        }))
    }
    pub fn record(start: u128, duration: u64, status: u16) {
        if let Some(sender) = EVENTS.get() {
            if sender
                .try_send(Event {
                    start,
                    duration,
                    status,
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
    if std::env::var_os("PIXSTRIDE_OTLP_ENDPOINT").is_some() {
        Err("OTLP requires the telemetry feature")
    } else {
        Ok(None)
    }
}
#[cfg(not(feature = "telemetry"))]
pub fn record(_: u128, _: u64, _: u16) {}
#[cfg(not(feature = "telemetry"))]
pub fn counters() -> (u64, u64) {
    (0, 0)
}
