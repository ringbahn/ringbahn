use std::io::IoSliceMut; 
use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use std::marker::Unpin;

use super::{Event, Cancellation};

/// A `readv` event.
pub struct ReadV<'a, T> {
    pub io: &'a T,
    pub bufs: Vec<Box<[u8]>>,
    pub offset: usize
}

impl<'a, T: AsRawFd + Unpin> ReadV<'a, T> {
    pub fn new(io: &'a T, bufs: Vec<Box<[u8]>>, offset: usize) -> ReadV<T> {
        ReadV { io, bufs, offset }
    }

    fn iovecs(&mut self) -> &mut [IoSliceMut] {
        // Unsafe contract:
        // This pointer cast is defined behaviour because Box<[u8]> (wide pointer)
        // is currently ABI compatible with libc::iovec.
        //
        // Then, libc::iovec is guaranteed ABI compatible with IoSliceMut on Unix:
        // https://doc.rust-lang.org/beta/std/io/struct.IoSliceMut.html
        //
        // We are relying on the internals of Box<[u8]>, but this is such a
        // foundational part of Rust it's unlikely the data layout would change
        // without warning.
        //
        // Pointer cast expression adapted from the "Turning a &mut T into an &mut U"
        // example of: https://doc.rust-lang.org/std/mem/fn.transmute.html#alternatives
        unsafe { &mut *(&mut self.bufs[..] as *mut [Box<[u8]>] as *mut [IoSliceMut]) }
    }
}


impl<'a, T: AsRawFd + Unpin> Event for ReadV<'a, T> {
    unsafe fn prepare(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let offset = self.offset;
        sqe.prep_read_vectored(self.io.as_raw_fd(), self.iovecs(), offset);
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        unsafe fn drop(data: *mut (), cap: usize) {
            std::mem::drop(Vec::from_raw_parts(data as *mut Box<[u8]>, cap, cap))
        }
        let mut bufs: ManuallyDrop<Vec<Box<[u8]>>> = ManuallyDrop::new(ManuallyDrop::take(this).bufs);
        Cancellation::new(bufs.as_mut_ptr() as *mut (), bufs.capacity(), drop)
    }
}
