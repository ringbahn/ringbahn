use std::io::IoSliceMut; 
use std::os::unix::io::RawFd;
use std::mem::ManuallyDrop;

use super::{Event, SQE, SQEs, Cancellation};

/// A `readv` event.
pub struct ReadV {
    pub fd: RawFd,
    pub bufs: Vec<Box<[u8]>>,
    pub offset: u64,
}

impl ReadV {
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


impl Event for ReadV {
    fn sqes_needed(&self) -> u32 { 1 }

    unsafe fn prepare<'sq>(&mut self, sqs: &mut SQEs<'sq>) -> SQE<'sq> {
        let mut sqe = sqs.single().unwrap();
        sqe.prep_read_vectored(self.fd, Self::as_iovecs(&mut self.bufs[..]), self.offset);
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
