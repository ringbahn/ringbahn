use std::io;
use std::os::unix::io::RawFd;

mod listener;
mod stream;

pub use listener::{UnixListener, Close};
pub use stream::{UnixStream, Connect};

use nix::sys::socket as nix;

fn socket() -> io::Result<RawFd> {
    match nix::socket(nix::AddressFamily::Unix, nix::SockType::Stream, nix::SockFlag::SOCK_CLOEXEC, None) {
        Ok(fd)  => Ok(fd),
        Err(_)  => Err(io::Error::last_os_error()),
    }
}

fn socketpair() -> io::Result<(RawFd, RawFd)> {
    match nix::socketpair(nix::AddressFamily::Unix, nix::SockType::Stream, None, nix::SockFlag::SOCK_CLOEXEC) {
        Ok((fd1, fd2))  => Ok((fd1, fd2)),
        Err(_)          => Err(io::Error::last_os_error()),
    }
}
