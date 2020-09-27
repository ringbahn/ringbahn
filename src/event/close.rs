use std::os::unix::io::RawFd;

use iou::registrar::UringFd;

use super::{Event, SQE, SQEs};

pub struct Close<FD = RawFd> {
    pub fd: FD,
}

impl<FD: UringFd + Copy> Event for Close<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_close(self.fd);
        sqe
    }
}
