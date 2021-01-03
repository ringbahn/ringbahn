use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::PosixFadviseAdvice;

use super::{Event, SQE};

pub struct Fadvise<FD = RawFd> {
    pub fd: FD,
    pub offset: u64,
    pub size: u64,
    pub flags: PosixFadviseAdvice,
}

impl<FD: UringFd + Copy> Event for Fadvise<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_fadvise(self.fd, self.offset, self.size, self.flags);
    }
}
