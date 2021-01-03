use std::os::unix::io::RawFd;

use iou::sqe::SpliceFlags;

use super::{Event, SQE};

pub struct Splice {
    pub fd_in: RawFd,
    pub off_in: i64,
    pub fd_out: RawFd,
    pub off_out: i64,
    pub bytes: u32,
    pub flags: SpliceFlags,
}

impl Event for Splice {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_splice(
            self.fd_in,
            self.off_in,
            self.fd_out,
            self.off_out,
            self.bytes,
            self.flags,
        );
    }
}
