use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::{RegisteredBuf, UringFd};

use super::{Cancellation, Event, SQE};

/// A basic read event.
pub struct Read<FD = RawFd> {
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for Read<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_read(self.fd, &mut self.buf[..], self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}

pub struct ReadFixed<FD = RawFd> {
    pub fd: FD,
    pub buf: RegisteredBuf,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for ReadFixed<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_read(self.fd, self.buf.as_mut(), self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).buf)
    }
}
