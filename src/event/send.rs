use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use iou::sqe::MsgFlags;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Send {
    pub fd: RawFd,
    pub buf: Box<[u8]>,
    pub flags: MsgFlags,
}

impl Event for Send {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_send(self.fd, &self.buf[..], self.flags);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let mut buf: ManuallyDrop<Box<[u8]>> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
        let cap = buf.len();
        Cancellation::buffer(buf.as_mut_ptr(), cap)
    }
}
