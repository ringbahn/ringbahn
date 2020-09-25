use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use iou::registrar::{UringFd, RegisteredBuf};

use super::{Event, SQE, SQEs, Cancellation};

/// A basic read event.
pub struct Read<FD = RawFd>{
    pub fd: FD,
    pub buf: Box<[u8]>,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for Read<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_read(self.fd, &mut self.buf[..], self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).buf)
    }
}

pub struct ReadFixed<FD = RawFd> {
    pub fd: FD,
    pub buf: RegisteredBuf,
    pub offset: u64,
}

impl<FD: UringFd + Copy> Event for ReadFixed<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_read(self.fd, self.buf.as_mut(), self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).buf.into_inner())
    }
}
