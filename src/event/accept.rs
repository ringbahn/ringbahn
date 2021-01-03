use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::{SockAddrStorage, SockFlag};

use super::{Cancellation, Event, SQE};

pub struct Accept<FD = RawFd> {
    pub addr: Option<Box<SockAddrStorage>>,
    pub fd: FD,
    pub flags: SockFlag,
}

impl<FD: UringFd + Copy> Event for Accept<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_accept(self.fd, self.addr.as_deref_mut(), self.flags);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).addr)
    }
}
