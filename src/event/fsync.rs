use std::os::unix::io::RawFd;

use iou::registrar::UringFd;
use iou::sqe::FsyncFlags;

use super::{Event, SQE};

pub struct Fsync<FD = RawFd> {
    pub fd: FD,
    pub flags: FsyncFlags,
}

impl<FD: UringFd + Copy> Event for Fsync<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_fsync(self.fd, self.flags);
    }
}
