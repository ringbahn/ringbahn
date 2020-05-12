use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use crate::{OwnedBuffer, Event, Cancellation};

pub struct Write<'a, T, B> {
    pub io: &'a mut T,
    pub buf: B,
}

impl<'a, T: AsRawFd + Unpin, B: AsRef<[u8]> + OwnedBuffer> Write<'a, T, B> {
    pub fn new(io: &'a mut T, buf: B) -> Write<T, B> {
        Write { io, buf }
    }
}

impl<'a, T: AsRawFd + Unpin, B: AsRef<[u8]> + OwnedBuffer> Event for Write<'a, T, B> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_write(self.io.as_raw_fd(), self.buf.as_ref(), 0);
    }

    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe {
            let buf: ManuallyDrop<B> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
            let len = buf.as_ref().len();
            Cancellation::buffer::<B>(buf.as_ref() as *const [u8] as *mut [u8], len)
        }
    }
}
