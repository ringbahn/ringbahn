use std::io::IoSlice; 
use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use super::{Event, Cancellation};

/// A `writev` event.
pub struct WriteV<'a, T> {
    pub io: &'a T,
    pub bufs: Vec<Box<[u8]>>,
    pub offset: usize
}

impl<'a, T: AsRawFd + Unpin> WriteV<'a, T> {
    pub fn new(io: &'a T, bufs: Vec<Box<[u8]>>, offset: usize) -> WriteV<T> {
        WriteV { io, bufs, offset }
    }

    fn iovecs(&self) -> &[IoSlice] {
        unsafe { & *(&self.bufs[..] as *const [Box<[u8]>] as *const [IoSlice]) }
    }
}

impl<'a, T: AsRawFd + Unpin> Event for WriteV<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_write_vectored(self.io.as_raw_fd(), self.iovecs(), self.offset);
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn drop(data: *mut (), cap: usize) {
            std::mem::drop(Vec::from_raw_parts(data as *mut Box<[u8]>, cap, cap))
        }
        let mut bufs: ManuallyDrop<Vec<Box<[u8]>>> = ManuallyDrop::new(ManuallyDrop::take(this).bufs);
        Cancellation::new(bufs.as_mut_ptr() as *mut (), bufs.capacity(), drop)
    }
}
