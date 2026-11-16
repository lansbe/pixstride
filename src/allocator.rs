//! GNU native workers use the system heap with one arena and base pages.
pub(crate) const WORKER_ARENAS: &str = "1";
pub(crate) const WORKER_TUNABLES: &str = "glibc.malloc.hugetlb=0";

/// Request base pages for this process; a denied optimization is nonfatal.
pub(crate) fn prefer_base_pages() -> bool {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: this changes only the calling process's page policy, takes
        // no pointers, and never changes host settings or resource ceilings.
        unsafe {
            libc::prctl(
                libc::PR_SET_THP_DISABLE,
                1 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            ) == 0
        }
    }
    #[cfg(not(target_os = "linux"))]
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_policy_preserves_live_rust_and_foreign_allocations() {
        let rust = vec![0x5au8; 8192];
        // SAFETY: allocate initialized foreign bytes, retain them across
        // page-policy changes, inspect only the live extent and pair with GLib free.
        unsafe {
            let pointer = libvips::bindings::g_try_malloc(8192).cast::<u8>();
            assert!(!pointer.is_null());
            pointer.write_bytes(0x37, 8192);
            let _ = super::prefer_base_pages();
            assert!(std::slice::from_raw_parts(pointer, 8192)
                .iter()
                .all(|b| *b == 0x37));
            libvips::bindings::g_free(pointer.cast());
        }
        assert!(rust.iter().all(|b| *b == 0x5a));
    }

    #[test]
    fn rust_libc_and_dynamic_glib_keep_the_system_heap() {
        let rust = vec![0x5au8; 4096];
        // SAFETY: this live allocation is only inspected for allocator ownership.
        assert!(!unsafe { libmimalloc_sys::mi_is_in_heap_region(rust.as_ptr().cast()) });
        // SAFETY: allocate a small block through libc, inspect it without
        // dereferencing, then release it through the matching C allocator API.
        let c_owned = unsafe {
            let pointer = libc::malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libc::free(pointer);
            owned
        };
        assert!(!c_owned, "libc allocations unexpectedly use mimalloc");
        // SAFETY: GLib is dynamically linked; pair its allocation and free APIs
        // while the allocation remains live for the ownership query.
        let glib_owned = unsafe {
            let pointer = libvips::bindings::g_try_malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_free(pointer);
            owned
        };
        assert!(!glib_owned, "dynamic codecs unexpectedly use mimalloc");
        // SAFETY: these dynamic GLib APIs use their matching sized/slice free
        // entry points, with the exact sizes used for allocation.
        unsafe {
            let pointer = libvips::bindings::g_try_malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_free_sized(pointer, 4096);
            assert!(!owned);
            let pointer = libvips::bindings::g_slice_alloc0(112);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_slice_free1(112, pointer);
            assert!(!owned);
        }
    }
    #[test]
    fn system_alignment_zeroing_and_reallocation_keep_their_c_contracts() {
        // SAFETY: nonzero allocation sizes and a valid power-of-two alignment;
        // dereference only initialized calloc bytes and pair every C allocation
        // with its matching free API after the ownership checks.
        unsafe {
            let mut aligned = std::ptr::null_mut();
            assert_eq!(libc::posix_memalign(&mut aligned, 4096, 8192), 0);
            assert!(!aligned.is_null());
            let alignment = aligned as usize % 4096;
            let owned = libmimalloc_sys::mi_is_in_heap_region(aligned);
            libc::free(aligned);
            assert_eq!(alignment, 0);
            assert!(!owned);
            let initial = libc::calloc(3, 256).cast::<u8>();
            assert!(!initial.is_null());
            assert!(std::slice::from_raw_parts(initial, 768)
                .iter()
                .all(|v| *v == 0));
            initial.write(0x5a);
            let grown = libc::realloc(initial.cast(), 8192).cast::<u8>();
            if grown.is_null() {
                libc::free(initial.cast());
            }
            assert!(!grown.is_null());
            assert_eq!(grown.read(), 0x5a);
            assert!(std::slice::from_raw_parts(grown.add(1), 767)
                .iter()
                .all(|v| *v == 0));
            let owned = libmimalloc_sys::mi_is_in_heap_region(grown.cast());
            #[cfg(target_os = "linux")]
            assert!(libc::malloc_usable_size(grown.cast()) >= 8192);
            libc::free(grown.cast());
            assert!(!owned);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn system_heap_preserves_source_budget_and_inherited_address_space_ceiling() {
        const MARKER: &str = "NATIVE_ALLOCATOR_TEST_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args([
                "--exact",
                "allocator::tests::system_heap_preserves_source_budget_and_inherited_address_space_ceiling",
                "--test-threads=1",
            ]);
            for (name, _) in std::env::vars_os() {
                if name.to_str().is_some_and(|n| n.starts_with("MIMALLOC_")) {
                    child.env_remove(name);
                }
            }
            let status = child
                .env(MARKER, "1")
                // Only this test binary links mimalloc for negative ownership
                // queries. Bound its diagnostic reservation; service builds
                // have no mimalloc code or reservation.
                .env("MIMALLOC_ARENA_RESERVE", "64MiB")
                .env("MALLOC_ARENA_MAX", super::WORKER_ARENAS)
                .env("MALLOC_MMAP_THRESHOLD_", "4194304")
                .env("MALLOC_TRIM_THRESHOLD_", "4194304")
                .env("GLIBC_TUNABLES", super::WORKER_TUNABLES)
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        assert!(super::prefer_base_pages());
        // SAFETY: reads this isolated child's page-policy flag, no pointers.
        assert_eq!(
            unsafe {
                libc::prctl(
                    libc::PR_GET_THP_DISABLE,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                )
            },
            1
        );
        let pages: u64 = std::fs::read_to_string("/proc/self/statm")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: sysconf takes no pointers; change only this isolated test
        // child's address-space ceiling, matching the native worker limit.
        unsafe {
            let page_size = libc::sysconf(libc::_SC_PAGESIZE);
            assert!(page_size > 0);
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            assert_eq!(libc::getrlimit(libc::RLIMIT_AS, &mut limit), 0);
            limit.rlim_cur = limit.rlim_cur.min(1024 * 1024 * 1024);
            limit.rlim_max = limit.rlim_max.min(1024 * 1024 * 1024);
            assert!(
                pages * (page_size as u64) < limit.rlim_cur,
                "initial mappings exceed the worker ceiling before admission"
            );
            assert_eq!(libc::setrlimit(libc::RLIMIT_AS, &limit), 0);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(crate::MAX_FILE)
            .expect("source-sized allocation under the AS ceiling");
        bytes.resize(crate::MAX_FILE, 0x5a_u8);
        assert_eq!(bytes[crate::MAX_FILE - 1], 0x5a);
        // SAFETY: the allocation is live and its address is only inspected.
        assert!(!unsafe { libmimalloc_sys::mi_is_in_heap_region(bytes.as_ptr().cast()) });
        let resident = || {
            std::fs::read_to_string("/proc/self/statm")
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u64>()
                .unwrap()
        };
        std::hint::black_box(&bytes);
        let before = resident();
        drop(bytes);
        // SAFETY: sysconf takes no pointers. The isolated child admits a
        // maximum-sized source and returns its pages without forced collection.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
        assert!(resident() * page_size + 32 * 1024 * 1024 < before * page_size);
    }
}
