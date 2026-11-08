#![allow(dead_code)]
//! Compile the actual source modules with minimal stand-ins for unrelated code.
//! The stand-ins are never used to exercise fetching or filesystem behavior.

use std::net::Ipv4Addr;
pub type Result<T> = std::result::Result<T, String>;

pub struct Buffer(Vec<u8>);
impl Buffer {
    fn new(len: usize) -> Result<Self> { Ok(Self(vec![0; len])) }
    fn as_mut_slice(&mut self) -> &mut [u8] { &mut self.0 }
}
pub struct Url { full: String, host: String, scheme: String }
impl Url {
    fn parse(_: &str) -> Result<Self> { Err("ABI stand-in".into()) }
    fn redirect(&self, _: &str) -> Result<Self> { Err("ABI stand-in".into()) }
}
fn public(_: Ipv4Addr) -> bool { false }
mod policy { pub fn allows(_: &str) -> bool { false } }

#[macro_use]
mod abi_probe_tests;
#[path = "transport_abi_probe.rs"]
mod transport;
#[path = "local_abi_probe.rs"]
mod local;

unsafe extern "C" {
    fn abi_layout_count() -> usize;
    fn abi_layout(index: usize) -> usize;
    fn abi_option_count() -> usize;
    fn abi_option(index: usize) -> i32;
    fn abi_info_count() -> usize;
    fn abi_info(index: usize) -> i32;
    fn abi_init_value_count() -> usize;
    fn abi_init_value(index: usize) -> i32;
    fn abi_file_flag_count() -> usize;
    fn abi_file_flag(index: usize) -> i32;
    fn abi_socket_flags() -> i32;
    fn abi_ipv4_octet_offset() -> usize;
    fn abi_make_sockaddr(out: *mut u8, valid_family: i32);
}

const OPTIONS: &[i32] = &include!("curl_options.rs");
const INFOS: &[i32] = &include!("curl_infos.rs");
const INIT_VALUES: &[i32] = &include!("curl_init_values.rs");
const IPV4_OCTETS: &[usize] = &include!("ipv4_octets.rs");
include!("curl_socket_flags.rs");

fn main() {
    transport::check_abi();
    local::check_abi();
    assert_eq!(SOCKET_FLAGS, unsafe { abi_socket_flags() }, "socket flags");
    for (index, value) in IPV4_OCTETS.iter().enumerate() {
        assert_eq!(*value, unsafe { abi_ipv4_octet_offset() } + index, "IPv4 octet {index}");
    }
    assert_eq!(OPTIONS.len(), unsafe { abi_option_count() });
    for (index, value) in OPTIONS.iter().enumerate() {
        assert_eq!(*value, unsafe { abi_option(index) }, "curl option {index}");
    }
    assert_eq!(INFOS.len(), unsafe { abi_info_count() });
    for (index, value) in INFOS.iter().enumerate() {
        assert_eq!(*value, unsafe { abi_info(index) }, "curl info {index}");
    }
    assert_eq!(INIT_VALUES.len(), unsafe { abi_init_value_count() });
    for (index, value) in INIT_VALUES.iter().enumerate() {
        assert_eq!(*value, unsafe { abi_init_value(index) }, "curl init value {index}");
    }
    println!("C and Rust ABI checks passed");
}
