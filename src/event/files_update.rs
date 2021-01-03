use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use super::{Cancellation, Event, SQEs, SQE};

pub struct FilesUpdate {
    pub files: Box<[RawFd]>,
    pub offset: u32,
}

impl Event for FilesUpdate {
    fn sqes_needed(&self) -> u32 {
        1
    }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_files_update(&self.files[..], self.offset);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).files)
    }
}
