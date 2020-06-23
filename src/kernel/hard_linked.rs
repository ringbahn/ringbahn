use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use super::{SubmissionSegment, SQE};

pub struct HardLinked<'a, 'b> {
    segment: *mut SubmissionSegment<'b>,
    marker: PhantomData<&'a mut SubmissionSegment<'b>>,
}

impl<'a, 'b> HardLinked<'a, 'b> {
    pub(super) fn new(segment: &'a mut SubmissionSegment<'b>) -> HardLinked<'a, 'b> {
        HardLinked { segment, marker: PhantomData }
    }
}

impl<'a, 'b> Iterator for HardLinked<'a, 'b> {
    type Item = HardLinkedSQE<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let is_final = (&*self.segment).remaining() > 1;
            let sqe = (&mut *self.segment).consume()?;
            Some(HardLinkedSQE { sqe, is_final })
        }
    }
}

pub struct HardLinkedSQE<'a> {
    sqe: &'a mut SQE,
    is_final: bool,
}

impl<'a> Deref for HardLinkedSQE<'a> {
    type Target = SQE;
    fn deref(&self) -> &SQE {
        self.sqe
    }
}

impl<'a> DerefMut for HardLinkedSQE<'a> {
    fn deref_mut(&mut self) -> &mut SQE {
        self.sqe
    }
}

impl Drop for HardLinkedSQE<'_> {
    fn drop(&mut self) {
        if !self.is_final {
            self.sqe.set_flag(uring_sys::IOSQE_IO_HARDLINK);
        }
    }
}
