//! Release completed codec work before considering unused allocator pages.
use crate::request::Request;
use std::cell::Cell;

thread_local! {
    static SHARPEN_SINCE_RECLAIM: Cell<u8> = const { Cell::new(0) };
}

fn reclaim_due(sharpen: bool, counter: &Cell<u8>) -> bool {
    if !sharpen {
        return false;
    }
    let due = counter.get() != 0;
    counter.set(u8::from(!due));
    due
}

pub(super) fn reclaim_for(r: &Request) -> bool {
    r.effects.sharpen_sigma > 0.0
}

fn release_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: glibc's thread-safe allocator advisory takes no user pointers and
    // only releases whole unused pages. All codec image roots/TLS drop first;
    // the still-live encoded response allocation remains owned and untouched.
    unsafe {
        libc::malloc_trim(0);
    }
}

pub(super) struct CodecCleanup {
    reclaim: bool,
}
impl CodecCleanup {
    pub(super) fn new(r: &Request) -> Self {
        Self {
            reclaim: reclaim_for(r),
        }
    }
}
impl Drop for CodecCleanup {
    fn drop(&mut self) {
        libvips::Vips::error_clear();
        libvips::Vips::thread_shutdown();
        // Amortize page-discard and next-request page-fault costs, while
        // bounding retention to one intervening sharpen operation.
        if SHARPEN_SINCE_RECLAIM.with(|c| reclaim_due(self.reclaim, c)) {
            release_heap();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reclamation_amortizes_page_fault_cost_across_sharpen_operations() {
        let counter = std::cell::Cell::new(0);
        assert!(!super::reclaim_due(true, &counter));
        assert!(!super::reclaim_due(false, &counter));
        assert!(super::reclaim_due(true, &counter));
        assert!(!super::reclaim_due(true, &counter));
        assert!(super::reclaim_due(true, &counter));
    }
    #[test]
    fn reclaim_policy_is_selected_by_the_actual_filter_request() {
        use crate::request::{self, Config};
        for (options, expected) in [("sh:1", true), ("bl:2", false), ("", false)] {
            let r = request::parse(
                &format!("/unsafe/{options}/plain/local:///fixture.jpg@jpg"),
                &Config::unsafe_local(),
            )
            .unwrap();
            assert_eq!(super::reclaim_for(&r), expected);
        }
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn gnu_reclaims_unused_pages_in_an_isolated_process() {
        if std::env::var_os("NATIVE_HEAP_RECLAIM_TEST_CHILD").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "processing::native::cleanup::tests::gnu_reclaims_unused_pages_in_an_isolated_process", "--nocapture"])
                .env("NATIVE_HEAP_RECLAIM_TEST_CHILD", "1").env("MALLOC_ARENA_MAX", "2")
                .env("MALLOC_MMAP_THRESHOLD_", "4194304").env("MALLOC_TRIM_THRESHOLD_", "4194304")
                .status().unwrap();
            assert!(status.success());
            return;
        }
        fn anonymous_bytes() -> usize {
            std::fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find_map(|s| {
                    s.strip_prefix("RssAnon:").map(|v| {
                        v.split_whitespace()
                            .next()
                            .unwrap()
                            .parse::<usize>()
                            .unwrap()
                            * 1024
                    })
                })
                .unwrap()
        }
        let chunks: Vec<Vec<u8>> = (0..12)
            .map(|_| {
                let mut v = vec![0u8; 1024 * 1024];
                for p in v.iter_mut().step_by(4096) {
                    *p = 1;
                }
                std::hint::black_box(v)
            })
            .collect();
        let guard = std::hint::black_box(vec![1u8; 64 * 1024]);
        drop(chunks);
        let before = anonymous_bytes();
        super::release_heap();
        let after = anonymous_bytes();
        assert!(
            before.saturating_sub(after) >= 4 * 1024 * 1024,
            "free pages retained: {before} -> {after}"
        );
        std::hint::black_box(guard);
    }
}
