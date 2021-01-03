use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::SockAddr;

use super::{Cancellation, Event, SQE};

pub struct Connect<FD = RawFd> {
    pub fd: FD,
    pub addr: Box<SockAddr>,
}

impl<FD: UringFd + Copy> Event for Connect<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_connect(self.fd, &mut *self.addr);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).addr)
    }
}
