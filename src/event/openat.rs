use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Event, SubmissionSegment, Cancellation};

pub struct OpenAt {
    path: CString,
    dfd: RawFd,
    flags: i32,
    mode: u32,
}

impl OpenAt {
    pub fn new(path: impl AsRef<Path>, dfd: RawFd, flags: i32, mode: u32) -> OpenAt {
        let path = CString::new(path.as_ref().as_os_str().as_bytes()).expect("invalid path");
        OpenAt { path, dfd, flags, mode }
    }
}

impl Event for OpenAt {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare(&mut self, sqs: &mut SubmissionSegment<'_>) {
        sqs.singular().prep_openat(self.dfd, self.path.as_ptr(), self.flags, self.mode)
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let path = ManuallyDrop::take(this).path;
        let mut bytes = ManuallyDrop::new(path.into_bytes_with_nul());
        let ptr = bytes.as_mut_ptr();
        let cap = bytes.capacity();
        Cancellation::buffer(ptr, cap)
    }
}
