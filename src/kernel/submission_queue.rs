use std::io;
use std::sync::Arc;

use super::{IoUring, SQE};

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
                let head = Some(&mut *sq.sqes.offset(offset as isize));
                Some(SubmissionSegment { head, remaining: n })
            } else {
                None
            }
        }
    }

    pub fn submit(&mut self) -> io::Result<u32> {
        match unsafe { uring_sys::io_uring_submit(self.ring.ring()) } {
            n if n >= 0 => {
                println!("submitted {}", n);
                Ok(n as u32)
            }
            n           => Err(io::Error::from_raw_os_error(-n)),
        }
    }
}

unsafe impl Send for SubmissionQueue { }
unsafe impl Sync for SubmissionQueue { }

pub struct SubmissionSegment<'a> {
    head: Option<&'a mut uring_sys::io_uring_sqe>,
    remaining: u32,
}

impl<'a> SubmissionSegment<'a> {
    pub fn singular(mut self) -> &'a mut SQE {
        while self.remaining > 1 {
            self.consume().unwrap().prep_nop();
        }

        self.consume().unwrap()
    }

    pub fn hard_linked(self) -> HardLinked<'a> {
        HardLinked { segment: self }
    }

    fn consume(&mut self) -> Option<&'a mut SQE> {
        let next = self.head.take()?;

        self.remaining -= 1;
        if self.remaining > 0 {
            unsafe {
                let sqe = (next as *mut uring_sys::io_uring_sqe).offset(1);
                self.head = Some(&mut *sqe);
            }
        }

        Some(SQE::from_raw(next))
    }
}

pub struct HardLinked<'a> {
    segment: SubmissionSegment<'a>,
}

impl<'a> Iterator for HardLinked<'a> {
    type Item = &'a mut SQE;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO after the user is done with it, if there are remaining SQEs, I want to set
        // IORING_SQE_HARDLINK
        //
        // Make sure that all of the unused are filled with noops
        //
        // Make it so the completion is only set on the final SQE somehow
        self.segment.consume()
    }
}
