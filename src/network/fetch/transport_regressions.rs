use super::{open_socket, Address};
use std::{net::Ipv4Addr, os::fd::FromRawFd, ptr};

fn address_for(ip: Ipv4Addr) -> Address {
    let mut address = [0u8; 16];
    #[cfg(target_os = "linux")]
    address[..2].copy_from_slice(&2u16.to_ne_bytes());
    #[cfg(target_os = "macos")]
    {
        address[0] = 16;
        address[1] = 2;
    }
    address[2..4].copy_from_slice(&443u16.to_be_bytes());
    address[4..8].copy_from_slice(&ip.octets());
    Address {
        family: 2,
        kind: 1,
        protocol: 6,
        length: 16,
        address,
    }
}

#[test]
fn socket_callback_refuses_private_and_reserved_ipv4_without_connecting() {
    for ip in [
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(169, 254, 1, 1),
        Ipv4Addr::new(172, 16, 0, 1),
        Ipv4Addr::new(192, 168, 1, 1),
        Ipv4Addr::new(198, 51, 100, 1),
    ] {
        let mut address = address_for(ip);
        // On the intended refusal path this function never calls socket().
        // If a regression creates an unconnected socket, close its descriptor.
        let descriptor = unsafe { open_socket(ptr::null_mut(), 0, &mut address) };
        if descriptor >= 0 {
            unsafe {
                drop(std::fs::File::from_raw_fd(descriptor));
            }
        }
        assert_eq!(descriptor, -1, "unexpected socket for {ip}");
    }
}

#[test]
fn socket_callback_refuses_invalid_arguments() {
    assert_eq!(
        unsafe { open_socket(ptr::null_mut(), 0, ptr::null_mut()) },
        -1
    );
    let mut address = address_for(Ipv4Addr::new(127, 0, 0, 1));
    assert_eq!(unsafe { open_socket(ptr::null_mut(), 1, &mut address) }, -1);
    address.length = 15;
    assert_eq!(unsafe { open_socket(ptr::null_mut(), 0, &mut address) }, -1);
}
