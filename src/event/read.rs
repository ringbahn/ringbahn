use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use super::{Event, SQE, Cancellation};

/// A basic read event.
pub struct Read<'a, T> {
    pub io: &'a T,
    pub buf: Vec<u8>,
    pub offset: u64
}

impl<'a, T: AsRawFd + Unpin> Read<'a, T> {
    pub fn new(io: &'a T, buf: Vec<u8>, offset: u64) -> Read<T> {
        Read { io, buf, offset }
    }
}

impl<'a, T: AsRawFd + Unpin> Event for Read<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut SQE) {
        sqe.prep_read(self.io.as_raw_fd(), &mut self.buf[..], self.offset);
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        let mut buf: ManuallyDrop<Vec<u8>> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
        let cap = buf.capacity();
        Cancellation::buffer(buf.as_mut_ptr(), cap)
    }
}
