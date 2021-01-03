use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::FallocateFlags;

use super::{Event, SQE};

pub struct Fallocate<FD = RawFd> {
    pub fd: FD,
    pub offset: u64,
    pub size: u64,
    pub flags: FallocateFlags,
}

impl<FD: UringFd + Copy> Event for Fallocate<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_fallocate(self.fd, self.offset, self.size, self.flags);
    }
}
