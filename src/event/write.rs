use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use iou::registrar::{UringFd, RegisteredBuf};

use super::{Event, SQE, SQEs, Cancellation};

/// A basic write event.
pub struct Write<FD = RawFd> {
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for Write<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_write(self.fd, &self.buf[..], self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).buf)
    }
}

pub struct WriteFixed<FD = RawFd> {
    pub fd: FD,
    pub buf: RegisteredBuf,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for WriteFixed<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_write(self.fd, self.buf.as_ref(), self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).buf.into_inner())
    }
}
