use std::io::IoSlice; 
use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

/// A `writev` event.
pub struct WriteVectored {
    pub fd: RawFd,
    pub bufs: Box<[Box<[u8]>]>,
    pub offset: u64,
}

impl WriteVectored {
    fn iovecs(&self) -> &[IoSlice] {
        unsafe { & *(&self.bufs[..] as *const [Box<[u8]>] as *const [IoSlice]) }
    }
}

impl Event for WriteVectored {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_write_vectored(self.fd, self.iovecs(), self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).bufs)
    }
}
