// These macros are appended to temporary copies of the actual source modules.
// Keeping the checks inside those modules grants access to their private types
// and constants without duplicating their definitions.
macro_rules! transport_abi_checks {
    () => {
        pub(crate) fn check_abi() {
            use std::mem::{align_of, offset_of, size_of};
            fn field_size<T>(_: fn(&Address) -> &T) -> usize { size_of::<T>() }
            let address_size = field_size(|value| &value.address);
            let rust = [
                size_of::<Address>(), align_of::<Address>(),
                offset_of!(Address, family), offset_of!(Address, kind),
                offset_of!(Address, protocol), offset_of!(Address, length),
                offset_of!(Address, address), address_size,
                size_of::<VersionPrefix>(), align_of::<VersionPrefix>(),
                offset_of!(VersionPrefix, age), offset_of!(VersionPrefix, version),
                offset_of!(VersionPrefix, version_num), offset_of!(VersionPrefix, host),
                offset_of!(VersionPrefix, features), address_size,
                size_of::<usize>(), size_of::<c_long>(), size_of::<c_int>(),
                size_of::<c_uint>(), size_of::<*mut c_void>(),
                size_of::<i64>(), align_of::<i64>(),
            ];
            assert_eq!(rust.len(), unsafe { crate::abi_layout_count() });
            for (index, value) in rust.into_iter().enumerate() {
                assert_eq!(value, unsafe { crate::abi_layout(index) }, "layout {index}");
            }
            let mut address = Address {
                family: 0, kind: 0, protocol: 0, length: 0, address: Default::default(),
            };
            unsafe { crate::abi_make_sockaddr(address.address.as_mut_ptr(), 1) };
            assert!(valid_family(&address.address), "C IPv4 family layout");
            unsafe { crate::abi_make_sockaddr(address.address.as_mut_ptr(), 0) };
            assert!(!valid_family(&address.address), "C non-IPv4 family layout");
            #[repr(C)]
            struct Extended { family: c_int, kind: c_int, protocol: c_int, length: c_uint, address: [u8; 28] }
            let mut v6 = Extended { family: 0, kind: 1, protocol: 6, length: 28, address: [0; 28] };
            v6.family = unsafe { crate::abi_make_sockaddr_v6(v6.address.as_mut_ptr()) };
            assert_eq!(unsafe { destination((&v6 as *const Extended).cast()) }, Some("2606:4700::1".parse().unwrap()));
            v6.length = 16;
            assert!(unsafe { destination((&v6 as *const Extended).cast()) }.is_none());
            let _: unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize = write_body;
            let _: unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize = write_header;
            let _: unsafe extern "C" fn(*mut c_void, c_int, *mut Address) -> c_int = open_socket;
        }
    };
}
macro_rules! local_abi_checks {
    () => {
        pub(crate) fn check_abi() {
            let rust = [O_RDONLY, O_NONBLOCK, O_NOFOLLOW, O_DIRECTORY, O_CLOEXEC];
            assert_eq!(rust.len() + 2, unsafe { crate::abi_file_flag_count() });
            for (index, value) in rust.into_iter().enumerate() {
                assert_eq!(value, unsafe { crate::abi_file_flag(index) }, "file flag {index}");
            }
            let file_flags = O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC;
            let create = unsafe { crate::abi_file_flag(5) };
            let tmpfile = unsafe { crate::abi_file_flag(6) };
            assert_eq!(file_flags & create, 0, "openat would need a mode argument");
            if tmpfile != 0 {
                assert_ne!(file_flags & tmpfile, tmpfile, "openat would need a mode argument");
            }
            let _: unsafe extern "C" fn(c_int, *const c_char, c_int, ...) -> c_int = openat;
        }
    };
}
