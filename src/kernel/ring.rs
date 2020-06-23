use std::io;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::Arc;

use super::{SubmissionQueue, CompletionQueue};

pub struct IoUring {
    ring: uring_sys::io_uring,
}

impl IoUring {
    pub fn new(entries: u32) -> io::Result<IoUring> {
        unsafe {
            let mut ring = MaybeUninit::uninit();
            match uring_sys::io_uring_queue_init(entries as _, ring.as_mut_ptr(), 0) {
                n if n >= 0 => Ok(IoUring { ring: ring.assume_init() }),
                n           => Err(io::Error::from_raw_os_error(-n)),
            }
        }
    }

    pub fn queues(self) -> (SubmissionQueue, CompletionQueue) {
        let ring = Arc::new(self);
        (SubmissionQueue::new(ring.clone()), CompletionQueue::new(ring))
    }

    pub(super) fn ring(&self) -> *mut uring_sys::io_uring {
        NonNull::from(&self.ring).as_ptr()
    }
}

unsafe impl Send for IoUring { }
unsafe impl Sync for IoUring { }

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe { uring_sys::io_uring_queue_exit(&mut self.ring) };
    }
}
