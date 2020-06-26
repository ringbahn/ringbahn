use std::io::IoSliceMut; 
use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use super::{Event, Cancellation};

/// A `readv` event.
pub struct ReadV<'a, T> {
    pub io: &'a T,
    pub bufs: Vec<Vec<u8>>,
    pub iovecs: Vec<IoSliceMut<'a>>, 
    pub offset: usize
}

impl<'a, T: AsRawFd + Unpin> ReadV<'a, T> {
    pub fn new(io: &'a T, bufs: Vec<Vec<u8>>, offset: usize) -> ReadV<T> {
        let mut iovecs = Vec::with_capacity(bufs.len()); 
        for buf in bufs.iter() { 
            // Unsafe contract: transmute is defined behaviour because 
            // IoSliceMut is guaranteed ABI compatible with libc::iovec 
            // on Unix: https://doc.rust-lang.org/beta/std/io/struct.IoSliceMut.html
            unsafe { 
                iovecs.push(std::mem::transmute::<libc::iovec, IoSliceMut>(
                    libc::iovec { 
                        iov_base: buf.as_ptr() as *mut libc::c_void, 
                        iov_len: buf.len() as libc::size_t, 
                    }
                )); 
            }
        }
        ReadV { io, iovecs, bufs, offset }
    }
}


impl<'a, T: AsRawFd + Unpin> Event for ReadV<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        sqe.prep_read_vectored(self.io.as_raw_fd(), &mut self.iovecs[..], self.offset);
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        todo!(); 
    }
}
