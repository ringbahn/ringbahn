use std::io::IoSliceMut; 
use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

/// A `readv` event.
pub struct ReadVectored {
    pub fd: RawFd,
    pub bufs: Box<[Box<[u8]>]>,
    pub offset: u64,
}

impl ReadVectored {
    fn as_iovecs(buffers: &mut [Box<[u8]>]) -> &mut [IoSliceMut] {
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
        unsafe { &mut *(buffers as *mut [Box<[u8]>] as *mut [IoSliceMut]) }
    }
}


impl Event for ReadVectored {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_read_vectored(self.fd, Self::as_iovecs(&mut self.bufs[..]), self.offset);
        sqe
    }

    unsafe fn cancel(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(ManuallyDrop::take(this).bufs)
    }
}
