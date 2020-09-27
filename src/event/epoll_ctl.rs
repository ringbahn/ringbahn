use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::sqe::{EpollOp, EpollEvent};

use super::{Event, SQE, SQEs, Cancellation};

pub struct EpollCtl {
    pub epoll_fd: RawFd,
    pub op: EpollOp,
    pub fd: RawFd,
    pub event: Option<Box<EpollEvent>>,
}

impl Event for EpollCtl {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_epoll_ctl(self.epoll_fd, self.op, self.fd, self.event.as_mut().map(|e| &mut **e));
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).event)
    }
}
