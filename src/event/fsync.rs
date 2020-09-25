use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::FsyncFlags;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Fsync<FD = RawFd> {
    pub fd: FD,
    pub flags: FsyncFlags,
}

impl<FD: UringFd + Copy> Event for Fsync<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_fsync(self.fd, self.flags);
        sqe
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
