use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::cmp;
use std::io;
use std::mem::{MaybeUninit, ManuallyDrop};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Poll, Context};
use std::slice;

use crate::Cancellation;
use super::Drive;

/// Provide buffers for use in IO with io-uring.
pub trait ProvideBuffer<D: Drive + ?Sized> {
    /// Constructor for a buffer that IO will be performed into. It is possible to implement
    /// backpressure or generate an IO error, depending on how your buffer management system works.
    ///
    /// The capacity argument is a request from the client for buffers of a particular capacity,
    /// but implementers are not required to return a buffer with that capacity. It's merely a hint
    /// at what the client would prefer.
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    where Self: Sized;

    /// Fill the buffer with data.
    ///
    /// ## Safety
    ///
    /// This API is unsafe. Callers must guarantee that this buffer has not been prepared into an
    /// active SQE when this method is called. The implementer can assume that it has not been.
    unsafe fn fill(&mut self, data: &[u8]);

    /// Return the underlying data in the buffer.
    ///
    /// ## Safety
    ///
    /// This API is unsafe. Callers must guarantee that this buffer has not been prepared into an
    /// active SQE when this method is called. The implementer can assume that it has not been.
    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>];

    /// Prepare a read using this buffer.
    ///
    /// The implementer is expected to set all of the data necessary to perform a read with this
    /// type of buffer. This include setting the addr, len, and op fields, for example.
    /// 
    /// ## Safety
    /// 
    /// Similar to `Event::prepare`, callers guarantee that this buffer will not be accessed again
    /// until after this SQE has been completed.
    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);

    /// Prepare a write using this buffer.
    ///
    /// The implementer is expected to set all of the data necessary to perform a write with this
    /// type of buffer. This include setting the addr, len, and op fields, for example.
    /// 
    /// ## Safety
    /// 
    /// Similar to `Event::prepare`, callers guarantee that this buffer will not be accessed again
    /// until after this SQE has been completed.
    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);

    /// Construct a cancellation to clean up this buffer.
    fn cleanup(this: ManuallyDrop<Self>) -> Cancellation;
}

pub struct HeapBuffer {
    data: NonNull<MaybeUninit<u8>>,
    filled: u32,
    capacity: u32,
}

impl<D: Drive + ?Sized> ProvideBuffer<D> for HeapBuffer {
    fn poll_provide(_: Pin<&mut D>, _: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    {
        let capacity = cmp::min(capacity, u32::MAX as usize);
        unsafe {
            let layout = Layout::array::<MaybeUninit<u8>>(capacity).unwrap();
            let ptr = alloc(layout);
            if ptr.is_null() {
                handle_alloc_error(layout);
            }
            Poll::Ready(Ok(HeapBuffer {
                data: NonNull::new_unchecked(ptr).cast(),
                capacity: capacity as u32,
                filled: 0,
            }))
        }
    }

    unsafe fn fill(&mut self, data: &[u8]) {
        let n = cmp::min(data.len(), self.capacity as usize);
        let slice = slice::from_raw_parts_mut(self.data.cast().as_ptr(), n);
        slice.copy_from_slice(&data[..n]);
        self.filled = n as u32;
    }

    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>] {
        slice::from_raw_parts(self.data.as_ptr(), self.capacity as usize)
    }

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let sqe = sqe.raw_mut();
        sqe.addr = self.data.as_ptr() as usize as u64;
        sqe.len = self.capacity;
        sqe.opcode = uring_sys::IoRingOp::IORING_OP_READ as u8;
        sqe.cmd_flags.rw_flags = 0;
    }

    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let sqe = sqe.raw_mut();
        sqe.addr = self.data.as_ptr() as usize as u64;
        sqe.len = self.filled;
        sqe.opcode = uring_sys::IoRingOp::IORING_OP_WRITE as u8;
        sqe.cmd_flags.rw_flags = 0;
    }

    fn cleanup(this: ManuallyDrop<Self>) -> Cancellation {
        unsafe { Cancellation::buffer(this.data.cast().as_ptr(), this.capacity as usize) }
    }
}

unsafe impl Send for HeapBuffer { }
unsafe impl Sync for HeapBuffer { }

impl Drop for HeapBuffer {
    fn drop(&mut self) {
        let layout = Layout::array::<MaybeUninit<u8>>(self.capacity as usize).unwrap();
        unsafe {
            dealloc(self.data.cast().as_ptr(), layout);
        }
    }
}
