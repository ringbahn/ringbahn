use std::io;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Arc;

use super::{IoUring, CQE};

pub struct CompletionQueue {
    ring: Arc<IoUring>,
}

impl CompletionQueue {
    pub(crate) fn new(ring: Arc<IoUring>) -> CompletionQueue {
        CompletionQueue { ring }
    }

    pub fn wait_for_cqe(&mut self) -> io::Result<CQE> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();

            match uring_sys::io_uring_wait_cqes(
                self.ring.ring(),
                cqe.as_mut_ptr(),
                1,
                ptr::null(),
                ptr::null(),
            ) {
                n if n >= 0 => {
                    let cqe_slot = cqe.assume_init();
                    let cqe = CQE::from_raw(ptr::read(cqe_slot));
                    uring_sys::io_uring_cqe_seen(self.ring.ring(), cqe_slot);
                    Ok(cqe)
                }
                n           => Err(io::Error::from_raw_os_error(-n)),
            }
        }
    }

    pub fn peek_for_cqe(&mut self) -> Option<CQE> {
        unsafe {
            let mut cqe = MaybeUninit::uninit();
            let count = uring_sys::io_uring_peek_batch_cqe(self.ring.ring(), cqe.as_mut_ptr(), 1);
            if count > 0 {
                let cqe_slot = cqe.assume_init();
                let cqe = CQE::from_raw(ptr::read(cqe_slot));
                uring_sys::io_uring_cqe_seen(self.ring.ring(), cqe_slot);
                Some(cqe)
            } else {
                None
            }
        }
    }

    pub fn ready(&self) -> u32 {
        unsafe { uring_sys::io_uring_cq_ready(self.ring.ring()) }
    }
}

unsafe impl Send for CompletionQueue { }
unsafe impl Sync for CompletionQueue { }
