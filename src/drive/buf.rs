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

pub trait ProvideBuffer<D: Drive + ?Sized> {
    fn poll_provide(driver: Pin<&mut D>, ctx: &mut Context<'_>, capacity: usize)
        -> Poll<io::Result<Self>>
    where Self: Sized;

    unsafe fn fill(&mut self, data: &[u8]);
    unsafe fn as_slice(&self) -> &[MaybeUninit<u8>];

    unsafe fn prepare_read(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);
    unsafe fn prepare_write(&mut self, sqe: &mut iou::SubmissionQueueEvent<'_>);

    unsafe fn cleanup(this: &mut ManuallyDrop<Self>) -> Cancellation;
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
        assert!(capacity <= u32::MAX as usize);
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

    unsafe fn cleanup(this: &mut ManuallyDrop<Self>) -> Cancellation {
        Cancellation::buffer(this.data.cast().as_ptr(), this.capacity as usize)
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
