//! Native concurrency uses fixed memory reservations, separate from HTTP admission.
use crate::Error;
use std::sync::{Condvar, Mutex};
pub(crate) struct Engine {
    active: Mutex<usize>,
    ready: Condvar,
    limit: usize,
}
pub(crate) struct Permit<'a>(&'a Engine);
impl Engine {
    pub fn new(limit: usize) -> Self {
        Self {
            active: Mutex::new(0),
            ready: Condvar::new(),
            limit,
        }
    }
    pub fn acquire(&self) -> Result<Permit<'_>, Error> {
        let count = self.active.lock().map_err(|_| "image worker unavailable")?;
        let (mut count, timeout) = self
            .ready
            .wait_timeout_while(count, std::time::Duration::from_secs(15), |n| {
                *n >= self.limit
            })
            .map_err(|_| "image worker unavailable")?;
        if timeout.timed_out() && *count >= self.limit {
            return Err("image queue deadline exceeded");
        }
        *count += 1;
        Ok(Permit(self))
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut count) = self.0.active.lock() {
            *count -= 1;
            self.0.ready.notify_one();
        }
    }
}

pub(crate) fn policy() -> Result<(usize, usize), Error> {
    let number = |name: &str, default: usize| {
        std::env::var(name).map_or(Ok(default), |v| {
            v.parse::<usize>()
                .map_err(|_| "invalid concurrency configuration")
        })
    };
    let processing = number("PIXSTRIDE_CONCURRENCY", 1)?;
    let admitted = number("PIXSTRIDE_MAX_CLIENTS", 4)?;
    if !(1..=4).contains(&processing) || !(processing..=16).contains(&admitted) {
        return Err("concurrency must be 1..4; clients must be concurrency..16");
    }
    if processing > 1
        && (!cfg!(feature = "native-codecs")
            || std::env::var("PIXSTRIDE_ENGINE").as_deref() != Ok("native"))
    {
        return Err("parallel processing requires the isolated native engine");
    }
    if processing > 1 || admitted > 4 {
        let budget = number("PIXSTRIDE_MEMORY_BUDGET_MIB", 0)?;
        // Each Linux child has a 1 GiB address-space limit. Reserve another
        // 512 MiB per active request and 64 MiB per admitted source in parent.
        // This is admission accounting; container limits enforce actual RSS.
        if budget < processing * 1536 + admitted * 64 || budget > 16384 {
            return Err("memory budget does not cover native concurrency reservations");
        }
    }
    Ok((processing, admitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permits_bound_processing_and_release_on_early_return() {
        let engine = std::sync::Arc::new(Engine::new(2));
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..12 {
                let e = engine.clone();
                let c = count.clone();
                scope.spawn(move || {
                    let _permit = e.acquire().unwrap();
                    let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    assert!(n <= 2);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    c.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
        });
        assert_eq!(*engine.active.lock().unwrap(), 0);
    }
}
