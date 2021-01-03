use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use super::{Cancellation, Event, SQE};

pub struct FilesUpdate {
    pub files: Box<[RawFd]>,
    pub offset: u32,
}

impl Event for FilesUpdate {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_files_update(&self.files[..], self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).files)
    }
}
