use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::sqe::FallocateFlags;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Fallocate {
    pub fd: RawFd,
    pub offset: u64,
    pub size: u64,
    pub flags: FallocateFlags,
}

impl Event for Fallocate {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_fallocate(self.fd, self.offset, self.size, self.flags);
        sqe
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
