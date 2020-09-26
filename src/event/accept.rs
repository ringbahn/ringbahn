use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::sqe::{SockFlag, SockAddrStorage};
use iou::registrar::UringFd;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Accept<FD = RawFd> {
    pub addr: Option<Box<SockAddrStorage>>,
    pub fd: FD,
    pub flags: SockFlag,
}

impl<FD: UringFd + Copy> Event for Accept<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_accept(self.fd, self.addr.as_mut().map(|addr| &mut **addr), self.flags);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        this.addr.take().map_or_else(Cancellation::null, Cancellation::object)
    }
}
