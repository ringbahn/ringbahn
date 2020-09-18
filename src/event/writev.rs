use std::io::IoSlice; 
use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

/// A `writev` event.
pub struct WriteV {
    pub fd: RawFd,
    pub bufs: Vec<Box<[u8]>>,
    pub offset: u64,
}

impl WriteV {
    fn iovecs(&self) -> &[IoSlice] {
        unsafe { & *(&self.bufs[..] as *const [Box<[u8]>] as *const [IoSlice]) }
    }
}

impl Event for WriteV {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_write_vectored(self.fd, self.iovecs(), self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn drop(data: *mut (), cap: usize) {
            std::mem::drop(Vec::from_raw_parts(data as *mut Box<[u8]>, cap, cap))
        }
        let mut bufs: ManuallyDrop<Vec<Box<[u8]>>> = ManuallyDrop::new(ManuallyDrop::take(this).bufs);
        Cancellation::new(bufs.as_mut_ptr() as *mut (), bufs.capacity(), drop)
    }
}
