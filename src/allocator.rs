//! GNU native builds share one allocator across Rust and dynamic C codecs.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub(crate) const WORKER_ARENA_RESERVE: &str = "64MiB";

#[cfg(test)]
mod tests {
    #[test]
    fn rust_libc_and_dynamic_glib_allocations_share_the_same_heap() {
        let rust = vec![0x5au8; 4096];
        // SAFETY: this live allocation is only inspected for allocator ownership.
        assert!(unsafe { libmimalloc_sys::mi_is_in_heap_region(rust.as_ptr().cast()) });
        // SAFETY: allocate a small block through libc, inspect it without
        // dereferencing, then release it through the matching C allocator API.
        let c_owned = unsafe {
            let pointer = libc::malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libc::free(pointer);
            owned
        };
        assert!(c_owned, "libc allocations were not interposed");
        // SAFETY: GLib is dynamically linked; pair its allocation and free APIs
        // while the allocation remains live for the ownership query.
        let glib_owned = unsafe {
            let pointer = libvips::bindings::g_try_malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_free(pointer);
            owned
        };
        assert!(glib_owned, "dynamic codecs still use a different heap");
        // SAFETY: these dynamic GLib APIs use their matching sized/slice free
        // entry points, with the exact sizes used for allocation.
        unsafe {
            let pointer = libvips::bindings::g_try_malloc(4096);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_free_sized(pointer, 4096);
            assert!(owned);
            let pointer = libvips::bindings::g_slice_alloc0(112);
            assert!(!pointer.is_null());
            let owned = libmimalloc_sys::mi_is_in_heap_region(pointer);
            libvips::bindings::g_slice_free1(112, pointer);
            assert!(owned);
        }
    }
    #[test]
    fn interposed_alignment_zeroing_and_reallocation_keep_their_c_contracts() {
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
            assert!(owned);
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
            assert!(owned);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn worker_arena_supports_the_inherited_address_space_ceiling() {
        const MARKER: &str = "NATIVE_ALLOCATOR_TEST_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args([
                "--exact",
                "allocator::tests::worker_arena_supports_the_inherited_address_space_ceiling",
                "--test-threads=1",
            ]);
            for (name, _) in std::env::vars_os() {
                if name.to_str().is_some_and(|n| n.starts_with("MIMALLOC_")) {
                    child.env_remove(name);
                }
            }
            let status = child
                .env(MARKER, "1")
                .env("MIMALLOC_ARENA_RESERVE", super::WORKER_ARENA_RESERVE)
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
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
                "allocator reservation exceeds the worker ceiling before admission"
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
        assert!(unsafe { libmimalloc_sys::mi_is_in_heap_region(bytes.as_ptr().cast()) });
    }
}
