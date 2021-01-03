use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::PosixFadviseAdvice;

use super::{Event, SQEs, SQE};

pub struct Fadvise<FD = RawFd> {
    pub fd: FD,
    pub offset: u64,
    pub size: u64,
    pub flags: PosixFadviseAdvice,
}

impl<FD: UringFd + Copy> Event for Fadvise<FD> {
    fn sqes_needed(&self) -> u32 {
        1
    }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_fadvise(self.fd, self.offset, self.size, self.flags);
        sqe
    }
}
