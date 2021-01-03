use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::path::Path;

use iou::sqe::{Mode, OFlag};

use super::{Cancellation, Event, SQE};

pub struct OpenAt {
    pub path: CString,
    pub dir_fd: RawFd,
    pub flags: OFlag,
    pub mode: Mode,
}

impl OpenAt {
    pub fn without_dir(path: impl AsRef<Path>, flags: OFlag, mode: Mode) -> OpenAt {
        let path = CString::new(path.as_ref().as_os_str().as_bytes()).unwrap();
        OpenAt {
            path,
            dir_fd: libc::AT_FDCWD,
            flags,
            mode,
        }
    }
}

impl Event for OpenAt {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_openat(self.dir_fd, &*self.path, self.flags, self.mode);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).path)
    }
}
