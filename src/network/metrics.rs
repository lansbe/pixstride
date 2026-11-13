//! Bounded-cardinality Prometheus metrics. Never label with URLs or secrets.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static COUNT: AtomicU64 = AtomicU64::new(0);
static MICROS: AtomicU64 = AtomicU64::new(0);
static STATUS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static BUCKETS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
const LIMITS: [u64; 8] = [
    1000,
    5000,
    10000,
    50000,
    100000,
    500000,
    1_000_000,
    u64::MAX,
];
thread_local! {static RESPONSE:std::cell::Cell<u16>=const{std::cell::Cell::new(200)};}
pub(crate) struct Guard(Instant, u128);
pub fn begin() -> Guard {
    ACTIVE.fetch_add(1, Ordering::Relaxed);
    RESPONSE.set(200);
    Guard(
        Instant::now(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    )
}
impl Drop for Guard {
    fn drop(&mut self) {
        let micros = self.0.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        super::telemetry::record(
            self.1,
            self.0.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
            RESPONSE.get(),
        );
        ACTIVE.fetch_sub(1, Ordering::Relaxed);
        COUNT.fetch_add(1, Ordering::Relaxed);
        MICROS.fetch_add(micros, Ordering::Relaxed);
        for (bucket, limit) in BUCKETS.iter().zip(LIMITS) {
            if micros <= limit {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
pub fn status(status: u16) {
    RESPONSE.set(status);
    if let Some(slot) = STATUS.get(usize::from(status / 100)) {
        slot.fetch_add(1, Ordering::Relaxed);
    }
}
pub fn render() -> String {
    let mut out=format!("# TYPE pixstride_requests_total counter\npixstride_requests_total {}\n# TYPE pixstride_requests_in_progress gauge\npixstride_requests_in_progress {}\n# TYPE pixstride_request_duration_seconds histogram\n",COUNT.load(Ordering::Relaxed),ACTIVE.load(Ordering::Relaxed));
    for (bucket, limit) in BUCKETS.iter().zip(LIMITS) {
        let le = if limit == u64::MAX {
            "+Inf".into()
        } else {
            format!("{}", limit as f64 / 1_000_000.0)
        };
        out.push_str(&format!(
            "pixstride_request_duration_seconds_bucket{{le=\"{le}\"}} {}\n",
            bucket.load(Ordering::Relaxed)
        ));
    }
    out.push_str(&format!("pixstride_request_duration_seconds_count {}\npixstride_request_duration_seconds_sum {}\n# TYPE pixstride_status_codes_total counter\n",COUNT.load(Ordering::Relaxed),MICROS.load(Ordering::Relaxed) as f64/1_000_000.0));
    for (class, count) in STATUS.iter().enumerate().skip(2) {
        out.push_str(&format!(
            "pixstride_status_codes_total{{class=\"{class}xx\"}} {}\n",
            count.load(Ordering::Relaxed)
        ));
    }
    let (dropped, failed) = super::telemetry::counters();
    out.push_str(&format!("# TYPE pixstride_telemetry_dropped_total counter\npixstride_telemetry_dropped_total {dropped}\n# TYPE pixstride_telemetry_failed_total counter\npixstride_telemetry_failed_total {failed}\n"));
    out
}
