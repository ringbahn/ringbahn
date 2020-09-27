use std::io::IoSlice; 
use std::mem::ManuallyDrop;
use std::os::unix::io::RawFd;

use iou::registrar::UringFd;

use super::{Event, SQE, SQEs, Cancellation};

/// A `writev` event.
pub struct WriteVectored<FD = RawFd> {
    pub fd: FD,
    pub bufs: Box<[Box<[u8]>]>,
    pub offset: u64,
}

impl<FD> WriteVectored<FD> {
    fn iovecs(&self) -> &[IoSlice] {
        unsafe { & *(&self.bufs[..] as *const [Box<[u8]>] as *const [IoSlice]) }
    }
}

impl<FD: UringFd + Copy> Event for WriteVectored<FD> {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_write_vectored(self.fd, self.iovecs(), self.offset);
        sqe
    }

    fn cancel(this: ManuallyDrop<Self>) -> Cancellation {
        Cancellation::from(ManuallyDrop::into_inner(this).bufs)
    }
}
