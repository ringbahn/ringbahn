use std::io;
use std::mem;
use std::sync::Arc;
use std::task::Context;

use crate::completion::Completion;
use crate::drive::Completion as ExternalCompletion;

use super::{HardLinked, IoUring, SQE};

pub struct SubmissionQueue {
    ring: Arc<IoUring>,
}

impl SubmissionQueue {
    pub(crate) fn new(ring: Arc<IoUring>) -> SubmissionQueue {
        SubmissionQueue { ring }
    }

    pub fn prepare(&mut self, n: u32) -> Option<SubmissionSegment<'_>> {
        unsafe {
            let ring = self.ring.ring();
            let sq = &mut (*ring).sq;
            if ((*ring).flags & uring_sys::IORING_SETUP_SQPOLL) == 0 {
                use std::sync::atomic::*;
                fence(Ordering::Acquire);
            };
            let head = *sq.khead;

            let next = sq.sqe_tail + n;
            if next - head <= *sq.kring_entries {
                let offset = sq.sqe_tail & *sq.kring_mask;
                sq.sqe_tail = next;
                let head = &mut *sq.sqes.offset(offset as isize);
                Some(SubmissionSegment { head, remaining: n })
            } else {
                None
            }
        }
    }

    pub fn submit(&mut self) -> io::Result<u32> {
        match unsafe { uring_sys::io_uring_submit(self.ring.ring()) } {
            n if n >= 0 => {
                Ok(n as u32)
            }
            n           => Err(io::Error::from_raw_os_error(-n)),
        }
    }
}

unsafe impl Send for SubmissionQueue { }
unsafe impl Sync for SubmissionQueue { }

pub struct SubmissionSegment<'a> {
    head: &'a mut uring_sys::io_uring_sqe,
    remaining: u32,
}

impl<'a> SubmissionSegment<'a> {
    pub fn singular(&mut self) -> &mut SQE {
        while self.consume().is_some() { }
        SQE::from_raw(self.head)
    }

    pub fn hard_linked(&mut self) -> HardLinked<'_, 'a> {
        HardLinked::new(self)
    }

    pub(super) fn consume(&mut self) -> Option<&mut SQE> {
        if self.remaining > 1 {
            // TODO replace with the proper offsetting
            let next = unsafe { &mut *(self.head as *mut uring_sys::io_uring_sqe).offset(1) };
            self.remaining -= 1;
            let sqe = SQE::from_raw(mem::replace(&mut self.head, next));
            sqe.prep_nop();
            Some(sqe)
        } else if self.remaining == 1 {
            self.remaining -= 1;
            Some(SQE::from_raw(&mut *self.head))
        } else {
            None
        }
    }

    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    pub fn finish<'cx>(&mut self, ctx: &mut Context<'cx>) -> ExternalCompletion<'cx> {
        let completion = Completion::new(ctx.waker().clone());
        SQE::from_raw(self.head).set_completion(&completion);

        if self.remaining != 0 {
            self.head.flags &= !(uring_sys::IOSQE_IO_LINK & uring_sys::IOSQE_IO_HARDLINK);
            while self.consume().is_some() { }
        }

        ExternalCompletion::new(completion, ctx)
    }
}
