use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use super::{Event, Cancellation};

pub struct Close {
    fd: RawFd,
}

impl Close {
    pub fn new(fd: RawFd) -> Close {
        Close { fd }
    }
}

impl Event for Close {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_close(self.fd);
    }

    fn cancellation(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
