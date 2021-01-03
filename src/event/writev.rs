use std::io::IoSlice;
use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;

use super::{Cancellation, Event, SQE};

/// A `writev` event.
pub struct WriteVectored<FD = RawFd> {
    pub fd: FD,
    pub bufs: Box<[Box<[u8]>]>,
    pub offset: u64,
}

impl<FD> WriteVectored<FD> {
    fn iovecs(&self) -> &[IoSlice] {
        unsafe { &*(&self.bufs[..] as *const [Box<[u8]>] as *const [IoSlice]) }
    }
}

impl<FD: UringFd + Copy> Event for WriteVectored<FD> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_write_vectored(self.fd, self.iovecs(), self.offset);
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).bufs)
    }
}
