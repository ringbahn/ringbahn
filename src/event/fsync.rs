use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::FsyncFlags;

use super::{Event, SQEs, SQE};

pub struct Fsync<FD = RawFd> {
    pub fd: FD,
    pub flags: FsyncFlags,
}

impl<FD: UringFd + Copy> Event for Fsync<FD> {
    fn sqes_needed(&self) -> u32 {
        1
    }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_fsync(self.fd, self.flags);
        sqe
    }
}
