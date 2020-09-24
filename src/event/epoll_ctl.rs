use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;
use std::ptr;

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

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(addr: *mut (), _: usize) {
            if addr != ptr::null_mut() {
                drop(Box::from_raw(addr as *mut EpollEvent));
            }
        }
        let addr: *mut EpollEvent = this.event.as_mut().map_or(ptr::null_mut(), |addr| {
            &mut **addr
        });
        Cancellation::new(addr as *mut (), 0, callback)
    }
}
