use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Connect {
    pub fd: RawFd,
    addr: Box<iou::SockAddr>,
}

impl Connect {
    pub fn new(fd: RawFd, addr: iou::SockAddr) -> Connect {
        Connect { fd, addr: Box::new(addr) }
    }
}

impl Event for Connect {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_connect(self.fd, &mut *self.addr);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(addr: *mut (), _: usize) {
            drop(Box::from_raw(addr as *mut iou::SockAddr));
        }
        Cancellation::new(&mut *this.addr as *mut iou::SockAddr as *mut (), 0, callback)
    }
}
