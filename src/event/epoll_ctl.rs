use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::sqe::{EpollEvent, EpollOp};

use super::{Cancellation, Event, SQE};

pub struct EpollCtl {
    pub epoll_fd: RawFd,
    pub op: EpollOp,
    pub fd: RawFd,
    pub event: Option<Box<EpollEvent>>,
}

impl Event for EpollCtl {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_epoll_ctl(self.epoll_fd, self.op, self.fd, self.event.as_deref_mut());
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).event)
    }
}
