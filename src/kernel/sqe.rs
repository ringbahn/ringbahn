use std::os::unix::io::RawFd;
use std::ptr::NonNull;

use crate::completion::Completion;

#[repr(transparent)]
pub struct SQE {
    inner: uring_sys::io_uring_sqe,
}

impl SQE {
    pub(super) fn from_raw(sqe: &mut uring_sys::io_uring_sqe) -> &mut SQE {
        sqe.user_data = 0;
        unsafe { &mut *NonNull::from(sqe).cast().as_ptr() }
    }

    pub(crate) fn set_completion(&mut self, completion: &Completion) {
        self.inner.user_data = completion.addr();
    }

    pub fn prep_nop(&mut self) {
        unsafe { uring_sys::io_uring_prep_nop(&mut self.inner) };
    }

    pub unsafe fn prep_write(&mut self, fd: RawFd, buf: &[u8], off: u64) {
        let len = buf.len();
        let addr = buf.as_ptr();
        uring_sys::io_uring_prep_write(&mut self.inner, fd, addr as _, len as _, off as _);
    }

    pub unsafe fn prep_read(&mut self, fd: RawFd, buf: &mut [u8], off: u64) {
        let len = buf.len();
        let addr = buf.as_mut_ptr();
        uring_sys::io_uring_prep_read(&mut self.inner, fd, addr as _, len as _, off as _);
    }

    pub unsafe fn prep_openat(&mut self, dfd: RawFd, path: *const libc::c_char, flags: i32, mode: u32) {
        uring_sys::io_uring_prep_openat(&mut self.inner, dfd, path, flags, mode);
    }

    pub unsafe fn prep_connect(&mut self, fd: RawFd, addr: *mut libc::sockaddr_storage, len: libc::socklen_t) {
        uring_sys::io_uring_prep_connect(&mut self.inner, fd, addr as *mut libc::sockaddr, len)
    }

    pub unsafe fn prep_accept(&mut self, fd: RawFd, addr: *mut libc::sockaddr_storage, len: *mut libc::socklen_t, flags: i32) {
        uring_sys::io_uring_prep_accept(&mut self.inner, fd, addr as *mut libc::sockaddr, len, flags)
    }

    pub unsafe fn prep_statx(&mut self, fd: RawFd, path: *const libc::c_char, flags: i32, mask: u32, statx: *mut libc::statx) {
        uring_sys::io_uring_prep_statx(&mut self.inner, fd, path, flags, mask, statx);
    }

    pub fn prep_close(&mut self, fd: RawFd) {
        unsafe { uring_sys::io_uring_prep_close(&mut self.inner, fd); }
    }

    pub fn prep_cancel(&mut self, user_data: u64, flags: i32) {
        unsafe { uring_sys::io_uring_prep_cancel(&mut self.inner, user_data as _, flags) }
    }

    pub fn set_flag(&mut self, flag: u8) {
        self.inner.flags &= flag;
    }

    pub fn unset_flag(&mut self, flag: u8) {
        self.inner.flags &= !flag;
    }
}
