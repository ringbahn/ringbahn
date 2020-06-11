use std::alloc::{dealloc, Layout};
use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;
use std::net::SocketAddr;

use super::{Event, Cancellation};

pub struct Connect {
    pub fd: RawFd,
    addr: Box<libc::sockaddr_storage>,
    addrlen: libc::socklen_t,
}

impl Connect {
    pub fn new(fd: RawFd, addr: SocketAddr) -> Connect {
        let (addr, addrlen) = crate::net::addr_to_c(addr);
        Connect { fd, addr, addrlen }
    }
}

impl Event for Connect {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let addr = &mut *self.addr as *mut libc::sockaddr_storage as *mut libc::sockaddr;
        uring_sys::io_uring_prep_connect(sqe.raw_mut(), self.fd, addr, self.addrlen);
    }

    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(addr: *mut (), _: usize) {
            dealloc(addr as *mut u8, Layout::new::<libc::sockaddr_storage>());
        }

        Cancellation::new(&mut *this.addr as *mut libc::sockaddr_storage as *mut (), 0, callback)
    }
}
