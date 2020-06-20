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

    unsafe fn access_from_group(&mut self, driver: Pin<&mut D>, idx: u16);
    unsafe fn return_to_group(&mut self, driver: Pin<&mut D>);

    /// Construct a cancellation to clean up this buffer.
    fn cleanup(this: ManuallyDrop<Self>, driver: Pin<&mut D>) -> Cancellation;
}

pub struct HeapBuffer {
    data: NonNull<MaybeUninit<u8>>,
    filled: u32,
    capacity: u32,
}

impl HeapBuffer {
    pub unsafe fn new(data: NonNull<MaybeUninit<u8>>, capacity: u32) -> HeapBuffer {
        HeapBuffer {
            data, capacity,
            filled: 0,
        }
    }
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
            Poll::Ready(Ok(HeapBuffer::new(NonNull::new_unchecked(ptr).cast(), capacity as u32)))
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

    unsafe fn access_from_group(&mut self, _: Pin<&mut D>, _: u16) { }
    unsafe fn return_to_group(&mut self, _: Pin<&mut D>) { }

    fn cleanup(this: ManuallyDrop<Self>, _: Pin<&mut D>) -> Cancellation {
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

use futures_core::ready;

pub struct RegisteredBuffer {
    buf: HeapBuffer,
    idx: BufferId,
}

impl<D: RegisterBuffer + ?Sized> ProvideBuffer<D> for RegisteredBuffer {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    {
        let (buf, idx) = ready!(driver.poll_provide_registered(ctx, capacity))?;
        Poll::Ready(Ok(RegisteredBuffer { buf, idx }))
    }

    unsafe fn fill(&mut self, data: &[u8]) {
        <HeapBuffer as ProvideBuffer<D>>::fill(&mut self.buf, data)
    }

    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>] {
        <HeapBuffer as ProvideBuffer<D>>::as_slice(&self.buf)
    }

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        <HeapBuffer as ProvideBuffer<D>>::prepare_read(&mut self.buf, sqe);
        sqe.raw_mut().opcode = uring_sys::IoRingOp::IORING_OP_READ_FIXED as _;
        sqe.raw_mut().buf_index.buf_index.index_or_group = self.idx.upper_idx as _;
    }

    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        <HeapBuffer as ProvideBuffer<D>>::prepare_write(&mut self.buf, sqe);
        sqe.raw_mut().opcode = uring_sys::IoRingOp::IORING_OP_WRITE_FIXED as _;
        sqe.raw_mut().buf_index.buf_index.index_or_group = self.idx.upper_idx as _;
    }

    unsafe fn access_from_group(&mut self, _: Pin<&mut D>, _: u16) { }
    unsafe fn return_to_group(&mut self, _: Pin<&mut D>) { }

    fn cleanup(this: ManuallyDrop<Self>, driver: Pin<&mut D>) -> Cancellation {
        let RegisteredBuffer { buf, idx } = ManuallyDrop::into_inner(this);
        let buf = ManuallyDrop::new(buf);
        driver.cleanup_registered(buf, idx)
    }
}

pub trait RegisterBuffer: Drive {
    fn poll_provide_registered(self: Pin<&mut Self>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<(HeapBuffer, BufferId)>>;
    fn cleanup_registered(self: Pin<&mut Self>, buf: ManuallyDrop<HeapBuffer>, idx: BufferId) -> Cancellation;
}

#[derive(Eq, PartialEq, Clone, Copy, Ord, PartialOrd, Hash)]
pub struct BufferId {
    pub upper_idx: u16,
    pub lower_idx: u16,
}

pub struct GroupRegisteredBuffer {
    group: u16,
    idx: u16,
    buf: Option<HeapBuffer>,
}

impl<D: RegisterBufferGroup + ?Sized> ProvideBuffer<D> for GroupRegisteredBuffer {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    {
        let group = ready!(driver.poll_provide_group(ctx, capacity))?;
        Poll::Ready(Ok(GroupRegisteredBuffer { group, idx: 0, buf: None, }))
    }

    unsafe fn fill(&mut self, _: &[u8]) {
        panic!("cannot fill group registered buffer");
    }

    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>] {
        self.buf.as_ref().map_or(&[], |buf| <HeapBuffer as ProvideBuffer<D>>::as_slice(buf))
    }

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let sqe = sqe.raw_mut();
        sqe.addr = 0;
        sqe.len = 0;
        sqe.opcode = uring_sys::IoRingOp::IORING_OP_READ_FIXED as u8;
        sqe.cmd_flags.rw_flags = 0;
        sqe.buf_index.buf_index.index_or_group = self.group as _;
    }

    unsafe fn prepare_write(&mut self, _: &mut iou::SubmissionQueueEvent<'_>) {
        panic!("cannot write into a pre-registered buffer group, only read")
    }

    unsafe fn access_from_group(&mut self, driver: Pin<&mut D>, idx: u16) {
        self.idx = idx;
        let idx = BufferId { upper_idx: self.group, lower_idx: idx };
        self.buf = Some(driver.access_buffer(idx));
    }

    unsafe fn return_to_group(&mut self, driver: Pin<&mut D>) {
        if let Some(buf) = self.buf.take() {
            let buf = ManuallyDrop::new(buf);
            let idx = BufferId { upper_idx: self.group, lower_idx: self.idx };
            drop(driver.cleanup_group_buffer(buf, idx));
        }
    }

    fn cleanup(mut this: ManuallyDrop<Self>, driver: Pin<&mut D>) -> Cancellation {
        if let Some(buf) = this.buf.take() {
            let buf = ManuallyDrop::new(buf);
            let idx = BufferId { upper_idx: this.group, lower_idx: this.idx };
            driver.cleanup_group_buffer(buf, idx)
        } else {
            driver.cancel_requested_buffer(this.group)
        }
    }
}

pub trait RegisterBufferGroup: Drive {
    fn poll_provide_group(self: Pin<&mut Self>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<u16>>;

    unsafe fn access_buffer(self: Pin<&mut Self>, idx: BufferId) -> HeapBuffer;

    fn cleanup_group_buffer(self: Pin<&mut Self>, buf: ManuallyDrop<HeapBuffer>, idx: BufferId) -> Cancellation;
    fn cancel_requested_buffer(self: Pin<&mut Self>, group: u16) -> Cancellation;
}
