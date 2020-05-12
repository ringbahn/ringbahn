use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use crate::{OwnedBuffer, Event, Cancellation};

pub struct Read<'a, T, B> {
    pub io: &'a mut T,
    pub buf: B,
}

impl<'a, T: AsRawFd + Unpin, B: AsMut<[u8]> + OwnedBuffer> Read<'a, T, B> {
    pub fn new(io: &'a mut T, buf: B) -> Read<T, B> {
        Read { io, buf }
    }
}

impl<'a, T: AsRawFd + Unpin, B: AsMut<[u8]> + OwnedBuffer> Event for Read<'a, T, B> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_read(self.io.as_raw_fd(), self.buf.as_mut(), 0);
    }

    fn cancellation(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe {
            let mut buf: ManuallyDrop<B> = ManuallyDrop::new(ManuallyDrop::take(this).buf);
            let len = buf.as_mut().len();
            Cancellation::buffer::<B>(buf.as_mut(), len)
        }
    }
}
