use std::os::unix::io::RawFd;

use iou::registrar::UringFd;

use super::{Event, SQE};

pub struct Close<FD = RawFd> {
    pub fd: FD,
}

impl<FD: UringFd + Copy> Event for Close<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_close(self.fd);
    }
}
