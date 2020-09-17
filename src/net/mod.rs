mod listener;
mod stream;

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::RawFd;

pub use listener::{TcpListener, Accept, Close, Incoming};
pub use stream::{TcpStream, Connect};

use nix::sys::socket as nix;

fn socket<A: ToSocketAddrs>(addr: A, protocol: nix::SockProtocol) -> io::Result<(RawFd, SocketAddr)> {
    use io::{Error, ErrorKind};

    let mut error = Error::new(ErrorKind::InvalidInput, "could not resolve to any addresses");

    for addr in addr.to_socket_addrs()? {
        let domain = match addr.is_ipv6() {
            true    => nix::AddressFamily::Inet6,
            false   => nix::AddressFamily::Inet,
        };

        let flags = nix::SockFlag::SOCK_CLOEXEC;

        match nix::socket(domain, nix::SockType::Stream, flags, Some(protocol)) {
            Ok(fd)          => return Ok((fd, addr)),
            _               => error = io::Error::last_os_error(),
        }
    }

    Err(error)
}
