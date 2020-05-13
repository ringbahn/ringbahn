use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use crate::{Event, Cancellation};

pub struct Read<'a, T> {
    pub io: &'a mut T,
    pub buf: Vec<u8>,
}

impl<'a, T: AsRawFd + Unpin> Read<'a, T> {
    pub fn new(io: &'a mut T, buf: Vec<u8>) -> Read<T> {
        Read { io, buf }
    }
}

impl<'a, T: AsRawFd + Unpin> Event for Read<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_read(self.io.as_raw_fd(), &mut self.buf[..], 0);
    }

    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe {
            let mut buf: ManuallyDrop<Vec<u8>> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
            let cap = buf.capacity();
            Cancellation::buffer(buf.as_mut_ptr(), cap)
        }
    }
}
