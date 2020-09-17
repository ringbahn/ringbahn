use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Event, SQE, SQEs, Cancellation};

pub struct OpenAt {
    path: CString,
    dfd: RawFd,
    flags: iou::OFlag,
    mode: iou::Mode,
}

impl OpenAt {
    pub fn new(path: impl AsRef<Path>, dfd: RawFd, flags: iou::OFlag, mode: iou::Mode) -> OpenAt {
        let path = CString::new(path.as_ref().as_os_str().as_bytes()).expect("invalid path");
        OpenAt { path, dfd, flags, mode }
    }
}

impl Event for OpenAt {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_openat(self.dfd, self.path.as_ptr(), self.flags, self.mode);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let path = ManuallyDrop::take(this).path;
        let mut bytes = ManuallyDrop::new(path.into_bytes_with_nul());
        let ptr = bytes.as_mut_ptr();
        let cap = bytes.capacity();
        Cancellation::buffer(ptr, cap)
    }
}
