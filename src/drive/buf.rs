use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::cmp;
use std::io;
use std::mem::{MaybeUninit, ManuallyDrop};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Poll, Context};
use std::slice;

use crate::Cancellation;
use super::{Drive, RegisterBuffer, RegisterBufferGroup};

/// A buffer type for read operations.
///
/// The most basic implementation is the [`HeapBuffer`] type, but drivers can also select the
/// [`RegisteredBuffer`] or [`GroupRegisteredBuffer`] types if the driver implements the necessary
/// extension trait.
pub trait ProvideReadBuf<D: Drive + ?Sized>: ProvideBufferSealed<D> { }

/// A buffer type for write operations.
///
/// The most basic implementation is the [`HeapBuffer`] type, but drivers can also select the
/// [`RegisteredBuffer`] type if the driver implements the necessary extension trait.
pub trait ProvideWriteBuf<D: Drive + ?Sized>: ProvideBufferSealed<D> { }

pub trait ProvideBufferSealed<D: Drive + ?Sized> {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<Self>>
    where Self: Sized;
    unsafe fn fill(&mut self, data: &[u8]);
    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>];
    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);
    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);
    unsafe fn access_from_group(&mut self, driver: Pin<&mut D>, idx: u16);
    unsafe fn return_to_group(&mut self, driver: Pin<&mut D>);
    fn cleanup(this: ManuallyDrop<Self>, driver: Pin<&mut D>) -> Cancellation;
}

/// A heap-allocated buffer for performing IO.
pub struct HeapBuffer {
    data: NonNull<MaybeUninit<u8>>,
    filled: u32,
    capacity: u32,
}

impl HeapBuffer {
    pub fn with_capacity(capacity: u32) -> HeapBuffer {
        unsafe {
            let layout = Layout::array::<MaybeUninit<u8>>(capacity as usize).unwrap();
            let ptr = alloc(layout);
            if ptr.is_null() {
                handle_alloc_error(layout);
            }
            HeapBuffer::from_raw(NonNull::new_unchecked(ptr).cast(), capacity)
        }
    }

    pub unsafe fn from_raw(data: NonNull<MaybeUninit<u8>>, capacity: u32) -> HeapBuffer {
        HeapBuffer {
            data, capacity,
            filled: 0,
        }
    }

    /// Returns the unitialized part of the buffer
    pub fn filled(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self.data.cast().as_ptr(), self.filled as usize)
        }
    }

    /// Returns the unitialized part of the buffer mutably
    pub fn filled_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self.data.cast().as_ptr(), self.filled as usize)
        }
    }
}

impl AsRef<[MaybeUninit<u8>]> for HeapBuffer {
    fn as_ref(&self) -> &[MaybeUninit<u8>] {
        &**self
    }
}

impl AsMut<[MaybeUninit<u8>]> for HeapBuffer {
    fn as_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        &mut **self
    }
}

impl AsRef<[u8]> for HeapBuffer {
    fn as_ref(&self) -> &[u8] {
        self.filled()
    }
}

impl AsMut<[u8]> for HeapBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.filled_mut()
    }
}

impl Deref for HeapBuffer {
    type Target = [MaybeUninit<u8>];
    fn deref(&self) -> &[MaybeUninit<u8>] {
        unsafe {
            slice::from_raw_parts(self.data.as_ptr(), self.capacity as usize)
        }
    }
}

impl DerefMut for HeapBuffer {
    fn deref_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe {
            slice::from_raw_parts_mut(self.data.as_ptr(), self.capacity as usize)
        }
    }
}

impl<D: Drive + ?Sized> ProvideReadBuf<D> for HeapBuffer { }
impl<D: Drive + ?Sized> ProvideWriteBuf<D> for HeapBuffer { }

impl<D: Drive + ?Sized> ProvideBufferSealed<D> for HeapBuffer {
    fn poll_provide(_: Pin<&mut D>, _: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<Self>>
    {
        Poll::Ready(Ok(HeapBuffer::with_capacity(capacity)))
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

/// A pre-registered buffer.
pub struct RegisteredBuffer {
    buf: HeapBuffer,
    idx: BufferId,
}

impl<D: RegisterBuffer + ?Sized> ProvideReadBuf<D> for RegisteredBuffer { }
impl<D: RegisterBuffer + ?Sized> ProvideWriteBuf<D> for RegisteredBuffer { }

impl<D: RegisterBuffer + ?Sized> ProvideBufferSealed<D> for RegisteredBuffer {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<Self>>
    {
        let (buf, idx) = ready!(driver.poll_provide_registered(ctx, capacity))?;
        Poll::Ready(Ok(RegisteredBuffer { buf, idx }))
    }

    unsafe fn fill(&mut self, data: &[u8]) {
        <HeapBuffer as ProvideBufferSealed<D>>::fill(&mut self.buf, data)
    }

    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>] {
        <HeapBuffer as ProvideBufferSealed<D>>::as_slice(&self.buf)
    }

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        <HeapBuffer as ProvideBufferSealed<D>>::prepare_read(&mut self.buf, sqe);
        sqe.raw_mut().opcode = uring_sys::IoRingOp::IORING_OP_READ_FIXED as _;
        sqe.raw_mut().buf_index.buf_index.index_or_group = self.idx.upper_idx as _;
    }

    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        <HeapBuffer as ProvideBufferSealed<D>>::prepare_write(&mut self.buf, sqe);
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

#[derive(Eq, PartialEq, Clone, Copy, Ord, PartialOrd, Hash)]
pub struct BufferId {
    pub upper_idx: u16,
    pub lower_idx: u16,
}

/// A group pre-registered buffer.
pub struct GroupRegisteredBuffer {
    group: u16,
    idx: u16,
    buf: Option<HeapBuffer>,
}

impl<D: RegisterBufferGroup + ?Sized> ProvideReadBuf<D> for GroupRegisteredBuffer { }

impl<D: RegisterBufferGroup + ?Sized> ProvideBufferSealed<D> for GroupRegisteredBuffer {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: u32)
        -> Poll<io::Result<Self>>
    {
        let group = ready!(driver.poll_provide_group(ctx, capacity))?;
        Poll::Ready(Ok(GroupRegisteredBuffer { group, idx: 0, buf: None, }))
    }

    unsafe fn fill(&mut self, _: &[u8]) {
        unreachable!()
    }

    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>] {
        self.buf.as_ref().map_or(&[], |buf| <HeapBuffer as ProvideBufferSealed<D>>::as_slice(buf))
    }

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let sqe = sqe.raw_mut();
        sqe.addr = 0;
        sqe.len = 0;
        sqe.opcode = uring_sys::IoRingOp::IORING_OP_READ_FIXED as u8;
        sqe.cmd_flags.rw_flags = 0;
        sqe.flags &= uring_sys::IOSQE_BUFFER_SELECT;
        sqe.buf_index.buf_index.index_or_group = self.group as _;
    }

    unsafe fn prepare_write(&mut self, _: &mut iou::SubmissionQueueEvent<'_>) {
        unreachable!()
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
            driver.cleanup_group_buffer(buf, idx);
        }
    }

    fn cleanup(mut this: ManuallyDrop<Self>, driver: Pin<&mut D>) -> Cancellation {
        if let Some(buf) = this.buf.take() {
            let buf = ManuallyDrop::new(buf);
            let idx = BufferId { upper_idx: this.group, lower_idx: this.idx };
            driver.cleanup_group_buffer(buf, idx);
            Cancellation::null()
        } else {
            driver.cancel_requested_buffer(this.group)
        }
    }
}
