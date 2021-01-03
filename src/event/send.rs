use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::MsgFlags;

use super::{Cancellation, Event, SQE};

pub struct Send<FD = RawFd> {
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub flags: MsgFlags,
}

impl<FD: UringFd + Copy> Event for Send<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_send(self.fd, &self.buf[..], self.flags);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}
