use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

/// A basic read event.
pub struct Read {
    pub io: RawFd,
    pub buf: Box<[u8]>,
    pub offset: u64
}

impl Event for Read {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_read(self.io, &mut self.buf[..], self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let mut buf: ManuallyDrop<Box<[u8]>> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
        let cap = buf.len();
        Cancellation::buffer(buf.as_mut_ptr(), cap)
    }
}
