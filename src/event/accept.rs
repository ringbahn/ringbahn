use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;
use std::ptr;

use iou::sqe::{SockFlag, SockAddrStorage};

use super::{Event, SQE, SQEs, Cancellation};

pub struct Accept {
    pub addr: Option<Box<SockAddrStorage>>,
    pub fd: RawFd,
    pub flags: SockFlag,
}

impl Event for Accept {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_accept(self.fd, self.addr.as_mut().map(|addr| &mut **addr), self.flags);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(addr: *mut (), _: usize) {
            if addr != ptr::null_mut() {
                drop(Box::from_raw(addr as *mut SockAddrStorage));
            }
        }
        let addr: *mut SockAddrStorage = this.addr.as_mut().map_or(ptr::null_mut(), |addr| {
            &mut **addr
        });
        Cancellation::new(addr as *mut (), 0, callback)
    }
}
