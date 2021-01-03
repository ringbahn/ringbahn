use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::{RegisteredBuf, UringFd};

use super::{Cancellation, Event, SQE};

/// A basic write event.
pub struct Write<FD = RawFd> {
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for Write<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_write(self.fd, &self.buf[..], self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}

pub struct WriteFixed<FD = RawFd> {
    pub fd: FD,
    pub buf: RegisteredBuf,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for WriteFixed<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_write(self.fd, self.buf.as_ref(), self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}
