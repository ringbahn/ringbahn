use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::MsgFlags;

use super::{Cancellation, Event, SQEs, SQE};

pub struct Send<FD = RawFd> {
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub flags: MsgFlags,
}

impl<FD: UringFd + Copy> Event for Send<FD> {
    fn sqes_needed(&self) -> u32 {
        1
    }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_send(self.fd, &self.buf[..], self.flags);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}
