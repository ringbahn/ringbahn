use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use super::{Event, Cancellation};

/// A basic write event.
pub struct Write<'a, T> {
    pub io: &'a T,
    pub buf: Vec<u8>,
    pub offset: usize
}

impl<'a, T: AsRawFd + Unpin> Write<'a, T> {
    pub fn new(io: &'a T, buf: Vec<u8>, offset: usize) -> Write<T> {
        Write { io, buf, offset }
    }
}

impl<'a, T: AsRawFd + Unpin> Event for Write<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_write(self.io.as_raw_fd(), self.buf.as_ref(), self.offset);
    }

    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe {
            let mut buf: ManuallyDrop<Vec<u8>> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
            let cap = buf.capacity();
            Cancellation::buffer(buf.as_mut_ptr(), cap)
        }
    }
}
