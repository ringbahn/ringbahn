use std::ffi::CString;
use std::mem::{self, ManuallyDrop};
use std::os::unix::io::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use iou::sqe::{StatxFlags, StatxMode};
use iou::registrar::UringFd;

use super::{Event, SQE, SQEs, Cancellation};

pub struct Statx<FD = RawFd> {
    pub dir_fd: FD,
    pub path: CString,
    pub flags: StatxFlags,
    pub mask: StatxMode,
    pub statx: Box<libc::statx>,
}

impl Statx {
    pub fn without_dir(path: impl AsRef<Path>, flags: StatxFlags, mask: StatxMode) -> Statx {
        let path = CString::new(path.as_ref().as_os_str().as_bytes()).unwrap();
        let statx = unsafe { Box::new(mem::zeroed()) };
        Statx { path, dir_fd: libc::AT_FDCWD, flags, mask, statx }
    }
}

impl<FD: UringFd> Statx<FD> {
    pub fn without_path(fd: FD, mut flags: StatxFlags, mask: StatxMode) -> Statx<FD> {
        unsafe {
            // TODO don't allocate? Use Cow? Use NULL?
            let path = CString::new("").unwrap();
            let statx = Box::new(mem::zeroed());
            flags.insert(StatxFlags::AT_EMPTY_PATH);
            Statx { dir_fd: fd, path, flags, mask, statx }
        }
    }
}

impl<FD: UringFd + Copy> Event for Statx<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_statx(self.dir_fd, self.path.as_c_str(), self.flags, self.mask, &mut *self.statx);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        let this = ManuallyDrop::into_inner(this);
        Cancellation::from((this.statx, this.path))
    }
}
