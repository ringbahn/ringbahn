use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use super::{Event, SubmissionSegment, Cancellation};

pub struct Close {
    fd: RawFd,
}

impl Close {
    pub fn new(fd: RawFd) -> Close {
        Close { fd }
    }
}

impl Event for Close {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare(&mut self, sqs: &mut SubmissionSegment<'_>) {
        sqs.singular().prep_close(self.fd)
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
