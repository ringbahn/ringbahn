use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::SockAddr;

use super::{Cancellation, Event, SQEs, SQE};

pub struct Connect<FD = RawFd> {
    pub fd: FD,
    pub addr: Box<SockAddr>,
}

impl<FD: UringFd + Copy> Event for Connect<FD> {
    fn sqes_needed(&self) -> u32 {
        1
    }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_connect(self.fd, &mut *self.addr);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).addr)
    }
}
