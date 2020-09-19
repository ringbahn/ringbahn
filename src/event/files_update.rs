use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;
use std::slice;

use super::{Event, SQE, SQEs, Cancellation};

pub struct FilesUpdate {
    pub files: Box<[RawFd]>,
    pub offset: u32,
}

impl Event for FilesUpdate {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_files_update(&self.files[..], self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn callback(data: *mut (), len: usize) {
            drop(Box::from_raw(slice::from_raw_parts_mut(data as *mut RawFd, len)))
        }
        let mut files: ManuallyDrop<Box<[RawFd]>> = ManuallyDrop::new(ManuallyDrop::take(this).files);
        let cap = files.len();
        Cancellation::new(files.as_mut_ptr() as *mut (), cap, callback)
    }
}
