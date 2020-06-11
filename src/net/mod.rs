mod listener;
mod stream;

use std::alloc::{alloc, Layout};
use std::io;
use std::mem;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::RawFd;
use std::ptr;

pub use listener::{TcpListener, Accept, Close, Incoming};
pub use stream::{TcpStream, Connect};

fn socket<A: ToSocketAddrs>(addr: A) -> io::Result<(RawFd, SocketAddr)> {
    use io::{Error, ErrorKind};

    let mut error = Error::new(ErrorKind::InvalidInput, "could not resolve to any addresses");

    for addr in addr.to_socket_addrs()? {
        let domain = match addr.is_ipv6() {
            true    => libc::AF_INET6,
            false   => libc::AF_INET,
        };

        let ty = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
        let protocol = libc::IPPROTO_TCP;

        match unsafe { libc::socket(domain, ty, protocol) } {
            fd if fd >= 0   => return Ok((fd, addr)),
            _               => error = io::Error::last_os_error(),
        }
    }

    Err(error)
}

pub(crate) unsafe fn addr_from_c(addr: &libc::sockaddr, len: usize) -> io::Result<SocketAddr> {
    match addr.sa_family as libc::c_int {
        libc::AF_INET   => {
            debug_assert!(len >= mem::size_of::<libc::sockaddr_in>());
            Ok(SocketAddr::V4(mem::transmute_copy(addr)))
        }
        libc::AF_INET6  => {
            debug_assert!(len >= mem::size_of::<libc::sockaddr_in6>());
            Ok(SocketAddr::V6(mem::transmute_copy(addr)))
        }
        _               => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid protocol"))
    }
}

pub(crate) fn addr_to_c(addr: SocketAddr) -> (Box<libc::sockaddr_storage>, libc::socklen_t) {
    unsafe {
        match addr {
            SocketAddr::V4(addr)    => {
                let addr: libc::sockaddr_in = mem::transmute(addr);
                let ptr: *mut u8 = alloc(Layout::new::<libc::sockaddr_storage>());
                ptr::write(ptr as *mut libc::sockaddr_in, addr);
                let len = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
                (Box::from_raw(ptr as *mut libc::sockaddr_storage), len)
            }
            SocketAddr::V6(addr)    => {
                let addr: libc::sockaddr_in6 = mem::transmute(addr);
                let ptr: *mut u8 = alloc(Layout::new::<libc::sockaddr_storage>());
                ptr::write(ptr as *mut libc::sockaddr_in6, addr);
                let len = mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t;
                (Box::from_raw(ptr as *mut libc::sockaddr_storage), len)
            }
        }
    }
}
