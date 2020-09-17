use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use super::{Event, SQE, SQEs, Cancellation};

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

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_close(self.fd);
        sqe
    }

    unsafe fn cancel(_: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::null()
    }
}
